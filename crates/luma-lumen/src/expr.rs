//! Expressions of `~` lines, conditions and `${...}` insertions: a small
//! language of literals, variables, operators and function calls.

use std::fmt;

use crate::ast::Span;

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `true` or `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A number with a decimal point.
    Float(f64),
    /// `"text"`.
    Str(String),
    /// A variable, or a name given to a function such as `has(item)`.
    Var(String, Span),
    /// `name(args)`.
    Call(String, Vec<Expr>, Span),
    /// `not x`.
    Not(Box<Expr>),
    /// `-x`.
    Neg(Box<Expr>),
    /// `a op b`.
    Binary(BinOp, Box<Expr>, Box<Expr>),
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Rem,
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
    /// `and`
    And,
    /// `or`
    Or,
}

/// Functions whose argument is a name rather than a value: `has(item)`,
/// `visits(knot)`, `seen(knot)`, `picked(tag)`.
pub const NAME_FUNCTIONS: &[&str] = &["has", "visits", "seen", "picked"];

/// Functions of values: name, number of arguments.
pub const VALUE_FUNCTIONS: &[(&str, usize)] = &[("random", 2), ("chance", 1)];

/// A value at run time.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Str(String),
}

/// The type of a value, for the checker's messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    /// `bool`
    Bool,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `string`
    Str,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Type::Bool => "bool",
            Type::Int => "int",
            Type::Float => "float",
            Type::Str => "string",
        })
    }
}

impl Value {
    /// The value's type.
    pub fn kind(&self) -> Type {
        match self {
            Value::Bool(_) => Type::Bool,
            Value::Int(_) => Type::Int,
            Value::Float(_) => Type::Float,
            Value::Str(_) => Type::Str,
        }
    }

    /// The value as a condition: booleans as they are, numbers when not
    /// zero, strings when not empty.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Str(s) => f.write_str(s),
        }
    }
}

/// What an expression is evaluated against.
pub trait Env {
    /// A variable's value, `None` when it does not exist.
    fn var(&self, name: &str) -> Option<Value>;
    /// A function of a name (`has`, `visits`, `seen`, `picked`).
    fn query(&mut self, function: &str, name: &str) -> Result<Value, String>;
    /// A function of values (`random`, `chance`, a module's).
    fn call(&mut self, function: &str, args: Vec<Value>) -> Result<Value, String>;
}

/// Evaluates an expression.
pub fn eval(expr: &Expr, env: &mut dyn Env) -> Result<Value, String> {
    match expr {
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Int(i) => Ok(Value::Int(*i)),
        Expr::Float(f) => Ok(Value::Float(*f)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Var(name, _) => env
            .var(name)
            .ok_or_else(|| format!("unknown variable `{name}`")),
        Expr::Call(name, args, _) => {
            if NAME_FUNCTIONS.contains(&name.as_str()) {
                let target = match args.as_slice() {
                    [Expr::Var(n, _)] | [Expr::Str(n)] => n.clone(),
                    _ => return Err(format!("`{name}` takes one name")),
                };
                env.query(name, &target)
            } else {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(eval(a, env)?);
                }
                env.call(name, values)
            }
        }
        Expr::Not(x) => Ok(Value::Bool(!eval(x, env)?.truthy())),
        Expr::Neg(x) => match eval(x, env)? {
            Value::Int(i) => Ok(Value::Int(-i)),
            Value::Float(f) => Ok(Value::Float(-f)),
            v => Err(format!("cannot negate a {}", v.kind())),
        },
        Expr::Binary(BinOp::And, a, b) => Ok(Value::Bool(
            eval(a, env)?.truthy() && eval(b, env)?.truthy(),
        )),
        Expr::Binary(BinOp::Or, a, b) => Ok(Value::Bool(
            eval(a, env)?.truthy() || eval(b, env)?.truthy(),
        )),
        Expr::Binary(op, a, b) => binary(*op, eval(a, env)?, eval(b, env)?),
    }
}

/// Applies an arithmetic or comparison operator.
pub fn binary(op: BinOp, a: Value, b: Value) -> Result<Value, String> {
    use Value::*;
    let incompatible = || {
        Err(format!(
            "cannot apply `{}` to {} and {}",
            op,
            a.kind(),
            b.kind()
        ))
    };
    match op {
        BinOp::Add => match (&a, &b) {
            (Int(x), Int(y)) => Ok(Int(x.wrapping_add(*y))),
            (Str(x), _) => Ok(Str(format!("{x}{b}"))),
            (_, Str(y)) => Ok(Str(format!("{a}{y}"))),
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Ok(Float(x + y)),
                _ => incompatible(),
            },
        },
        BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => match (&a, &b) {
            (Int(x), Int(y)) => match op {
                BinOp::Sub => Ok(Int(x.wrapping_sub(*y))),
                BinOp::Mul => Ok(Int(x.wrapping_mul(*y))),
                BinOp::Div if *y == 0 => Err("division by zero".to_string()),
                BinOp::Div => Ok(Int(x.wrapping_div(*y))),
                _ if *y == 0 => Err("division by zero".to_string()),
                _ => Ok(Int(x.wrapping_rem(*y))),
            },
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Ok(Float(match op {
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => x / y,
                    _ => x % y,
                })),
                _ => incompatible(),
            },
        },
        BinOp::Eq => Ok(Bool(equal(&a, &b))),
        BinOp::Ne => Ok(Bool(!equal(&a, &b))),
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
            let ordering = match (&a, &b) {
                (Str(x), Str(y)) => x.cmp(y),
                _ => match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                    _ => return incompatible(),
                },
            };
            Ok(Bool(match op {
                BinOp::Lt => ordering.is_lt(),
                BinOp::Gt => ordering.is_gt(),
                BinOp::Le => ordering.is_le(),
                _ => ordering.is_ge(),
            }))
        }
        BinOp::And | BinOp::Or => Ok(Bool(match op {
            BinOp::And => a.truthy() && b.truthy(),
            _ => a.truthy() || b.truthy(),
        })),
    }
}

fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
    }
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        })
    }
}

/// A parse error: the message and the character offset in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What is wrong.
    pub message: String,
    /// Offset of the offending token in the source, in characters.
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
}

/// Parses an expression. `base` is where the source starts in its line
/// (its `col` is the column of the first character), so that variable
/// and call spans point into the file.
pub fn parse(src: &str, base: Span) -> Result<Expr, ParseError> {
    let tokens = lex(src)?;
    let mut p = ExprParser {
        tokens,
        pos: 0,
        base,
    };
    let expr = p.parse_or()?;
    if p.pos < p.tokens.len() {
        return Err(ParseError {
            message: format!("unexpected `{}`", p.describe(p.pos)),
            offset: p.tokens[p.pos].1,
        });
    }
    Ok(expr)
}

fn lex(src: &str) -> Result<Vec<(Tok, u32)>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i as u32;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let mut j = i;
            let mut float = false;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                if chars[j] == '.' {
                    if float || j + 1 >= chars.len() || !chars[j + 1].is_ascii_digit() {
                        break;
                    }
                    float = true;
                }
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            let tok = if float {
                Tok::Float(text.parse().map_err(|_| ParseError {
                    message: format!("bad number `{text}`"),
                    offset: start,
                })?)
            } else {
                Tok::Int(text.parse().map_err(|_| ParseError {
                    message: format!("bad number `{text}`"),
                    offset: start,
                })?)
            };
            out.push((tok, start));
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            out.push((Tok::Ident(chars[i..j].iter().collect()), start));
            i = j;
        } else if c == '"' {
            let mut j = i + 1;
            let mut s = String::new();
            loop {
                if j >= chars.len() {
                    return Err(ParseError {
                        message: "unterminated string".to_string(),
                        offset: start,
                    });
                }
                if chars[j] == '\\' && j + 1 < chars.len() {
                    s.push(chars[j + 1]);
                    j += 2;
                    continue;
                }
                if chars[j] == '"' {
                    break;
                }
                s.push(chars[j]);
                j += 1;
            }
            out.push((Tok::Str(s), start));
            i = j + 1;
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            let op = match two.as_str() {
                "==" => Some("=="),
                "!=" => Some("!="),
                "<=" => Some("<="),
                ">=" => Some(">="),
                _ => None,
            };
            if let Some(op) = op {
                out.push((Tok::Op(op), start));
                i += 2;
                continue;
            }
            let op = match c {
                '+' => "+",
                '-' => "-",
                '*' => "*",
                '/' => "/",
                '%' => "%",
                '<' => "<",
                '>' => ">",
                '(' => "(",
                ')' => ")",
                ',' => ",",
                _ => {
                    return Err(ParseError {
                        message: format!("unexpected character `{c}`"),
                        offset: start,
                    });
                }
            };
            out.push((Tok::Op(op), start));
            i += 1;
        }
    }
    Ok(out)
}

struct ExprParser {
    tokens: Vec<(Tok, u32)>,
    pos: usize,
    base: Span,
}

impl ExprParser {
    fn describe(&self, at: usize) -> String {
        match &self.tokens[at].0 {
            Tok::Int(i) => i.to_string(),
            Tok::Float(f) => f.to_string(),
            Tok::Str(s) => format!("\"{s}\""),
            Tok::Ident(s) => s.clone(),
            Tok::Op(o) => o.to_string(),
        }
    }

    fn span_at(&self, offset: u32, len: usize) -> Span {
        Span {
            col: self.base.col + offset,
            len: len.max(1) as u32,
            ..self.base
        }
    }

    fn peek_op(&self, op: &str) -> bool {
        matches!(self.tokens.get(self.pos), Some((Tok::Op(o), _)) if *o == op)
    }

    fn peek_ident(&self, word: &str) -> bool {
        matches!(self.tokens.get(self.pos), Some((Tok::Ident(w), _)) if w == word)
    }

    fn end_error(&self) -> ParseError {
        ParseError {
            message: "the expression ends too early".to_string(),
            offset: self
                .tokens
                .last()
                .map_or(0, |(t, o)| o + token_len(t) as u32),
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_and()?;
        while self.peek_ident("or") {
            self.pos += 1;
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_not()?;
        while self.peek_ident("and") {
            self.pos += 1;
            let right = self.parse_not()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.peek_ident("not") {
            self.pos += 1;
            return Ok(Expr::Not(Box::new(self.parse_not()?)));
        }
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_additive()?;
        let op = match self.tokens.get(self.pos) {
            Some((Tok::Op("=="), _)) => BinOp::Eq,
            Some((Tok::Op("!="), _)) => BinOp::Ne,
            Some((Tok::Op("<"), _)) => BinOp::Lt,
            Some((Tok::Op(">"), _)) => BinOp::Gt,
            Some((Tok::Op("<="), _)) => BinOp::Le,
            Some((Tok::Op(">="), _)) => BinOp::Ge,
            _ => return Ok(left),
        };
        self.pos += 1;
        let right = self.parse_additive()?;
        Ok(Expr::Binary(op, Box::new(left), Box::new(right)))
    }

    fn parse_additive(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_term()?;
        loop {
            let op = if self.peek_op("+") {
                BinOp::Add
            } else if self.peek_op("-") {
                BinOp::Sub
            } else {
                return Ok(left);
            };
            self.pos += 1;
            let right = self.parse_term()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.parse_unary()?;
        loop {
            let op = if self.peek_op("*") {
                BinOp::Mul
            } else if self.peek_op("/") {
                BinOp::Div
            } else if self.peek_op("%") {
                BinOp::Rem
            } else {
                return Ok(left);
            };
            self.pos += 1;
            let right = self.parse_unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.peek_op("-") {
            self.pos += 1;
            return Ok(Expr::Neg(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let Some((tok, offset)) = self.tokens.get(self.pos).cloned() else {
            return Err(self.end_error());
        };
        self.pos += 1;
        match tok {
            Tok::Int(i) => Ok(Expr::Int(i)),
            Tok::Float(f) => Ok(Expr::Float(f)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Ident(w) if w == "true" => Ok(Expr::Bool(true)),
            Tok::Ident(w) if w == "false" => Ok(Expr::Bool(false)),
            Tok::Ident(w) if w == "and" || w == "or" || w == "not" => Err(ParseError {
                message: format!("unexpected `{w}`"),
                offset,
            }),
            Tok::Ident(name) => {
                let span = self.span_at(offset, name.chars().count());
                if self.peek_op("(") {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if !self.peek_op(")") {
                        loop {
                            args.push(self.parse_or()?);
                            if self.peek_op(",") {
                                self.pos += 1;
                                continue;
                            }
                            break;
                        }
                    }
                    if !self.peek_op(")") {
                        return Err(match self.tokens.get(self.pos) {
                            Some((_, o)) => ParseError {
                                message: format!("expected `)`, got `{}`", self.describe(self.pos)),
                                offset: *o,
                            },
                            None => ParseError {
                                message: "expected `)`".to_string(),
                                offset: self.end_error().offset,
                            },
                        });
                    }
                    self.pos += 1;
                    Ok(Expr::Call(name, args, span))
                } else {
                    Ok(Expr::Var(name, span))
                }
            }
            Tok::Op("(") => {
                let inner = self.parse_or()?;
                if !self.peek_op(")") {
                    return Err(ParseError {
                        message: "expected `)`".to_string(),
                        offset: self
                            .tokens
                            .get(self.pos)
                            .map_or(self.end_error().offset, |(_, o)| *o),
                    });
                }
                self.pos += 1;
                Ok(inner)
            }
            Tok::Op(o) => Err(ParseError {
                message: format!("unexpected `{o}`"),
                offset,
            }),
        }
    }
}

fn token_len(t: &Tok) -> usize {
    match t {
        Tok::Int(i) => i.to_string().len(),
        Tok::Float(f) => f.to_string().len(),
        Tok::Str(s) => s.chars().count() + 2,
        Tok::Ident(s) => s.chars().count(),
        Tok::Op(o) => o.len(),
    }
}

/// Every variable and call in an expression, in order.
pub fn walk<'a>(expr: &'a Expr, visit: &mut dyn FnMut(&'a Expr)) {
    visit(expr);
    match expr {
        Expr::Call(_, args, _) => {
            for a in args {
                walk(a, visit);
            }
        }
        Expr::Not(x) | Expr::Neg(x) => walk(x, visit),
        Expr::Binary(_, a, b) => {
            walk(a, visit);
            walk(b, visit);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoEnv;
    impl Env for NoEnv {
        fn var(&self, name: &str) -> Option<Value> {
            match name {
                "n" => Some(Value::Int(3)),
                "s" => Some(Value::Str("ab".to_string())),
                _ => None,
            }
        }
        fn query(&mut self, function: &str, name: &str) -> Result<Value, String> {
            Ok(Value::Bool(function == "has" && name == "key"))
        }
        fn call(&mut self, function: &str, args: Vec<Value>) -> Result<Value, String> {
            match function {
                "random" => Ok(args[0].clone()),
                _ => Err(format!("unknown function `{function}`")),
            }
        }
    }

    fn ev(src: &str) -> Value {
        eval(&parse(src, Span::default()).unwrap(), &mut NoEnv).unwrap()
    }

    #[test]
    fn precedence_and_values() {
        assert_eq!(ev("1 + 2 * 3"), Value::Int(7));
        assert_eq!(ev("(1 + 2) * 3"), Value::Int(9));
        assert_eq!(ev("7 / 2"), Value::Int(3));
        assert_eq!(ev("7 / 2.0"), Value::Float(3.5));
        assert_eq!(ev("-n + 1"), Value::Int(-2));
        assert_eq!(ev("n >= 3 and not false"), Value::Bool(true));
        assert_eq!(ev("n > 3 or s == \"ab\""), Value::Bool(true));
        assert_eq!(ev("s + n"), Value::Str("ab3".to_string()));
        assert_eq!(ev("has(key) and not has(gem)"), Value::Bool(true));
        assert_eq!(ev("random(1, 6)"), Value::Int(1));
        assert_eq!(ev("10 % 4 == 2"), Value::Bool(true));
    }

    #[test]
    fn errors_point_at_the_token() {
        let err = parse("1 + ", Span::default()).unwrap_err();
        assert_eq!(err.message, "the expression ends too early");
        let err = parse("a b", Span::default()).unwrap_err();
        assert_eq!((err.message.as_str(), err.offset), ("unexpected `b`", 2));
        let err = parse("x == $", Span::default()).unwrap_err();
        assert_eq!(err.offset, 5);
        assert!(
            eval(&parse("1 / 0", Span::default()).unwrap(), &mut NoEnv)
                .unwrap_err()
                .contains("division by zero")
        );
        assert!(
            eval(&parse("zz", Span::default()).unwrap(), &mut NoEnv)
                .unwrap_err()
                .contains("unknown variable")
        );
    }

    #[test]
    fn spans_follow_the_base_column() {
        let base = Span {
            file: 0,
            line: 4,
            col: 10,
            len: 0,
        };
        let Expr::Binary(_, a, _) = parse("foo > bar(1)", base).unwrap() else {
            panic!()
        };
        assert_eq!(
            *a,
            Expr::Var(
                "foo".to_string(),
                Span {
                    file: 0,
                    line: 4,
                    col: 10,
                    len: 3
                }
            )
        );
    }
}
