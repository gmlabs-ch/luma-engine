//! The parser: tokens to the syntax tree, recording lines the way the
//! original's LALR(1) parser did.
//!
//! Written with reference to `compiler/lang.y` and `compiler/tree.c` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma
//! and contributors); see `NOTICE`. The grammar and the "lateness" of each
//! production are specified in `docs/compiler.md`.
//!
//! The original stores the scanner's line counter in a node when its
//! production is reduced; a production reduced in a state that needs the
//! lookahead sees the line of the token *after* the construct, one reduced
//! by a default reduction sees the line of its last token. This hand-written
//! parser reproduces that by fetching the next token ([`Lexer::peek`]) only
//! where the original automaton does, and reading [`Lexer::line`] when it
//! builds a node. The comments below name the productions of `lang.y` and
//! say which case each node is.

use crate::ast::*;
use crate::error::{Pos, Result};
use crate::lexer::{Includes, Keyword, Lexer, Punct, Token};

/// Parses `source`, named `name`, expanding includes through `includes`.
pub fn parse(source: Vec<u8>, name: &str, includes: &mut dyn Includes) -> Result<Script> {
    let mut parser = Parser {
        lexer: Lexer::new(source, name, includes),
        statements: 0,
        expressions: 0,
        operators: 0,
    };
    let items = parser.script()?;
    // `scriptcollection: toplevels` reduces after the end of input has
    // been fetched: the line is the final value of the counter.
    let line = parser.lexer.line();
    Ok(Script {
        items,
        line,
        files: parser.lexer.files().to_vec(),
    })
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    /// How many statements enclose the one being parsed.
    statements: u32,
    /// How many expressions and unary operators enclose the one being
    /// parsed.
    expressions: u32,
    /// The binary operators of the statement so far.
    operators: u32,
}

// A script may come from a game's files and be compiled as the game
// runs. The parser, the code generator and the release of the tree all
// recurse on the nesting, so the nesting is bounded: far above anything a
// person writes, far below what the stack holds.
const MAX_STATEMENT_DEPTH: u32 = 100;
const MAX_EXPRESSION_DEPTH: u32 = 64;
const MAX_OPERATORS: u32 = 1000;

impl Parser<'_> {
    fn line(&self) -> u32 {
        self.lexer.line()
    }

    fn peek(&mut self) -> Result<&Token> {
        self.lexer.peek()
    }

    fn advance(&mut self) -> Result<Token> {
        self.lexer.advance()
    }

    fn at_punct(&mut self, punct: Punct) -> Result<bool> {
        Ok(self.peek()? == &Token::Punct(punct))
    }

    fn at_keyword(&mut self, keyword: Keyword) -> Result<bool> {
        Ok(self.peek()? == &Token::Keyword(keyword))
    }

    /// Consumes the next token if it is `punct`.
    fn eat_punct(&mut self, punct: Punct) -> Result<bool> {
        if self.at_punct(punct)? {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect_punct(&mut self, punct: Punct) -> Result<()> {
        let token = self.advance()?;
        if token == Token::Punct(punct) {
            return Ok(());
        }
        Err(self.syntax_error(&token, &format!("`{}`", punct.text())))
    }

    fn expect_ident(&mut self) -> Result<(String, Pos)> {
        let token = self.advance()?;
        match token {
            Token::Ident(name) => Ok((name, self.lexer.last_pos())),
            other => Err(self.syntax_error(&other, "an identifier")),
        }
    }

    fn expect_string(&mut self) -> Result<Vec<u8>> {
        let token = self.advance()?;
        match token {
            Token::Str(bytes) => Ok(bytes),
            other => Err(self.syntax_error(&other, "a string constant")),
        }
    }

    /// "syntax error" of the original, with what was found and expected.
    fn syntax_error(&self, found: &Token, expected: &str) -> crate::Error {
        self.lexer.error(
            &self.lexer.last_pos(),
            format!(
                "syntax error: expected {expected}, found {}",
                found.describe()
            ),
        )
    }

    /// Consumes the next token and reports it as unexpected.
    fn unexpected(&mut self, expected: &str) -> Result<crate::Error> {
        let token = self.advance()?;
        Ok(self.syntax_error(&token, expected))
    }

    // `scriptcollection: toplevels`, `netoplevels: toplevel netoplevels`.
    fn script(&mut self) -> Result<Vec<TopLevel>> {
        let mut items = Vec::new();
        loop {
            match self.peek()? {
                Token::Eof => return Ok(items),
                Token::Keyword(Keyword::Function) | Token::Keyword(Keyword::Method) => {
                    items.push(TopLevel::Function(self.function()?));
                }
                Token::Keyword(Keyword::On) => items.push(TopLevel::Event(self.event()?)),
                Token::Keyword(Keyword::External) => {
                    items.push(TopLevel::External(self.external()?));
                }
                _ => {
                    // `toplevel: nestms`: a run of statements, as long as
                    // possible (the automaton shifts).
                    let mut stmts = Vec::new();
                    loop {
                        stmts.push(self.statement()?);
                        if matches!(
                            self.peek()?,
                            Token::Eof
                                | Token::Keyword(
                                    Keyword::Function
                                        | Keyword::Method
                                        | Keyword::On
                                        | Keyword::External
                                )
                        ) {
                            break;
                        }
                    }
                    items.push(TopLevel::Statements(stmts));
                }
            }
        }
    }

    // `function: tFUNCTION tIDENTIFIER '(' formals ')' compoundstm`
    // (and `tMETHOD`): default reduction, the line is the closing brace's.
    fn function(&mut self) -> Result<Function> {
        let is_method = self.advance()? == Token::Keyword(Keyword::Method);
        let (name, pos) = self.expect_ident()?;
        self.expect_punct(Punct::LParen)?;
        let mut params = Vec::new();
        if !self.at_punct(Punct::RParen)? {
            loop {
                // `formal: optvar tIDENTIFIER`: default reduction, the
                // line is the name's.
                if self.at_keyword(Keyword::Var)? {
                    self.advance()?;
                }
                let (name, pos) = self.expect_ident()?;
                let line = self.line();
                params.push(Param { name, pos, line });
                if !self.eat_punct(Punct::Comma)? {
                    break;
                }
            }
        }
        self.expect_punct(Punct::RParen)?;
        let body = self.block_body()?;
        let line = self.line();
        Ok(Function {
            name,
            pos,
            params,
            body,
            is_method,
            line,
        })
    }

    // `event: tON tSTRINGCONST stm`: default reduction, the line is
    // inherited from the statement.
    fn event(&mut self) -> Result<Event> {
        self.advance()?;
        let name = self.expect_string()?;
        let body = Box::new(self.statement()?);
        let line = self.line();
        Ok(Event { name, body, line })
    }

    // `external: tEXTERNAL tSTRINGCONST call_type ext_returns tIDENTIFIER
    //            '(' ext_formals ')' ';'`
    fn external(&mut self) -> Result<External> {
        self.advance()?;
        let dll = self.expect_string()?;
        let call = match self.peek()? {
            Token::Keyword(Keyword::StdCall) => {
                self.advance()?;
                CallConv::StdCall
            }
            Token::Keyword(Keyword::CDecl) => {
                self.advance()?;
                CallConv::CDecl
            }
            Token::Keyword(Keyword::ThisCall) => {
                self.advance()?;
                CallConv::ThisCall
            }
            _ => CallConv::StdCall,
        };
        let returns = match ext_type(self.peek()?) {
            Some(kind) => {
                self.advance()?;
                kind
            }
            None => ExtType::Void,
        };
        let (name, pos) = self.expect_ident()?;
        self.expect_punct(Punct::LParen)?;
        let mut params = Vec::new();
        if !self.at_punct(Punct::RParen)? {
            loop {
                let token = self.advance()?;
                let Some(kind) = ext_type(&token) else {
                    return Err(self.syntax_error(&token, "a parameter type"));
                };
                params.push(kind);
                if let Token::Ident(_) = self.peek()? {
                    self.advance()?;
                }
                if !self.eat_punct(Punct::Comma)? {
                    break;
                }
            }
        }
        self.expect_punct(Punct::RParen)?;
        self.expect_punct(Punct::Semi)?;
        Ok(External {
            dll,
            name,
            pos,
            call,
            returns,
            params,
        })
    }

    // `compoundstm: '{' '}' | '{' nestms '}'`: default reduction after the
    // closing brace, which is therefore the line the caller reads.
    fn block_body(&mut self) -> Result<Vec<Stmt>> {
        self.expect_punct(Punct::LBrace)?;
        let mut stmts = Vec::new();
        while !self.eat_punct(Punct::RBrace)? {
            stmts.push(self.statement()?);
        }
        Ok(stmts)
    }

    // `stm`
    fn statement(&mut self) -> Result<Stmt> {
        if self.statements >= MAX_STATEMENT_DEPTH {
            return Err(self.too_deep("statements"));
        }
        self.statements += 1;
        self.operators = 0;
        let statement = self.statement_body();
        self.statements -= 1;
        statement
    }

    fn too_deep(&self, what: &str) -> crate::Error {
        self.lexer
            .error(&self.lexer.last_pos(), format!("{what} nested too deeply"))
    }

    fn statement_body(&mut self) -> Result<Stmt> {
        match self.peek()? {
            Token::Punct(Punct::Semi) => {
                self.advance()?;
                Ok(Stmt::Empty)
            }
            Token::Keyword(Keyword::Return) => {
                self.advance()?;
                let pos = self.lexer.last_pos();
                // `tRETURN ';'` and `tRETURN exp ';'`: default reductions,
                // the line is the semicolon's.
                let value = if self.at_punct(Punct::Semi)? {
                    None
                } else {
                    Some(self.expression()?)
                };
                self.expect_punct(Punct::Semi)?;
                let line = self.line();
                Ok(Stmt::Return { value, pos, line })
            }
            Token::Keyword(Keyword::If) => {
                self.advance()?;
                self.expect_punct(Punct::LParen)?;
                let cond = self.expression()?;
                self.expect_punct(Punct::RParen)?;
                let then = Box::new(self.statement()?);
                // The automaton must look past the statement for `else`:
                // without one, the line is the next token's; with one, the
                // `ifelse` production is a default reduction and inherits
                // the line of the `else` branch.
                let otherwise = if self.at_keyword(Keyword::Else)? {
                    self.advance()?;
                    Some(Box::new(self.statement()?))
                } else {
                    None
                };
                let line = self.line();
                Ok(Stmt::If {
                    cond,
                    then,
                    otherwise,
                    line,
                })
            }
            Token::Keyword(Keyword::While) => {
                self.advance()?;
                self.expect_punct(Punct::LParen)?;
                let cond = self.expression()?;
                self.expect_punct(Punct::RParen)?;
                let body = Box::new(self.statement()?);
                // Default reduction: inherits the body's line.
                let line = self.line();
                Ok(Stmt::While { cond, body, line })
            }
            Token::Keyword(Keyword::For) => self.for_statement(),
            Token::Punct(Punct::LBrace) => Ok(Stmt::Block(self.block_body()?)),
            Token::Keyword(Keyword::Var | Keyword::Global | Keyword::Const) => {
                Ok(Stmt::Decl(self.declaration()?))
            }
            Token::Break => {
                self.advance()?;
                let pos = self.lexer.last_pos();
                let line = self.line();
                Ok(Stmt::Break { pos, line })
            }
            Token::Continue => {
                self.advance()?;
                let pos = self.lexer.last_pos();
                let line = self.line();
                Ok(Stmt::Continue { pos, line })
            }
            Token::Keyword(Keyword::Switch) => self.switch_statement(),
            Token::Eof => Err(self.unexpected("a statement")?),
            _ => {
                // `stm: exp ';'`: default reduction after the semicolon.
                let expr = self.expression()?;
                self.expect_punct(Punct::Semi)?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    // `decl: type identifiers initialization ';'`: default reduction, the
    // line is the semicolon's; `type` reduces right after its keyword.
    fn declaration(&mut self) -> Result<Decl> {
        let kind = match self.advance()? {
            Token::Keyword(Keyword::Var) => DeclKind::Var,
            Token::Keyword(Keyword::Global) => DeclKind::Global,
            Token::Keyword(Keyword::Const) => DeclKind::Const,
            _ => unreachable!("checked by the caller"),
        };
        let kind_line = self.line();
        let mut names = Vec::new();
        loop {
            names.push(self.expect_ident()?);
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        let init = if self.eat_punct(Punct::Assign)? {
            Some(self.expression()?)
        } else {
            None
        };
        self.expect_punct(Punct::Semi)?;
        let line = self.line();
        Ok(Decl {
            kind,
            kind_line,
            names,
            init,
            line,
        })
    }

    // `stm: tFOR '(' forinits ';' exp ';' exps ')' stm`: default
    // reduction, inherits the body's line.
    fn for_statement(&mut self) -> Result<Stmt> {
        self.advance()?;
        let pos = self.lexer.last_pos();
        self.expect_punct(Punct::LParen)?;
        if self.at_punct(Punct::Semi)? {
            // The original accepts an empty initializer list and then
            // dereferences a null pointer; there is no reference output.
            return Err(self
                .lexer
                .error(&pos, "for loop without initializer (unsupported)"));
        }
        let mut inits = Vec::new();
        loop {
            if self.at_keyword(Keyword::Var)? {
                self.advance()?;
                let (name, name_pos) = self.expect_ident()?;
                let init = if self.eat_punct(Punct::Assign)? {
                    Some(self.expression()?)
                } else {
                    None
                };
                // `simpledecl: tVAR tIDENTIFIER initialization` reduces by
                // default, but `initialization` needed the lookahead: the
                // line is the token after the initializer (or the name).
                // The keyword's line comes from the same reduction.
                let line = self.line();
                inits.push(ForInit::Decl(Decl {
                    kind: DeclKind::Var,
                    kind_line: line,
                    names: vec![(name, name_pos)],
                    init,
                    line,
                }));
            } else {
                inits.push(ForInit::Expr(self.expression()?));
            }
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.expect_punct(Punct::Semi)?;
        let cond = self.expression()?;
        self.expect_punct(Punct::Semi)?;
        let mut updates = Vec::new();
        if !self.at_punct(Punct::RParen)? {
            loop {
                updates.push(self.expression()?);
                if !self.eat_punct(Punct::Comma)? {
                    break;
                }
            }
        }
        self.expect_punct(Punct::RParen)?;
        let body = Box::new(self.statement()?);
        let line = self.line();
        Ok(Stmt::For {
            inits,
            cond,
            updates,
            body,
            line,
        })
    }

    // `stm: tSWITCH '(' exp ')' '{' dcases '}'`: default reduction, the
    // line is the closing brace's.
    fn switch_statement(&mut self) -> Result<Stmt> {
        self.advance()?;
        self.expect_punct(Punct::LParen)?;
        let value = self.expression()?;
        self.expect_punct(Punct::RParen)?;
        self.expect_punct(Punct::LBrace)?;
        let mut cases = Vec::new();
        loop {
            match self.peek()? {
                Token::Keyword(Keyword::Case) => {
                    self.advance()?;
                    // `case_branch: tCASE unarypostfixexp ':' nestms`: the
                    // label is a postfix expression only; the body runs
                    // until the next `case`, `default` or `}`, which must
                    // be fetched, so the line is that token's.
                    let label = self.postfix()?.0;
                    self.expect_punct(Punct::Colon)?;
                    let mut body = Vec::new();
                    while !matches!(
                        self.peek()?,
                        Token::Keyword(Keyword::Case | Keyword::Default)
                            | Token::Punct(Punct::RBrace)
                    ) {
                        body.push(self.statement()?);
                    }
                    let line = self.line();
                    cases.push(Case {
                        label: Some(label),
                        body,
                        line,
                    });
                }
                Token::Keyword(Keyword::Default) => {
                    self.advance()?;
                    self.expect_punct(Punct::Colon)?;
                    // `default_branch: tDEFAULT ':' nestms`: at least one
                    // statement, and nothing but `}` may follow.
                    let mut body = vec![self.statement()?];
                    while !self.at_punct(Punct::RBrace)? {
                        body.push(self.statement()?);
                    }
                    let line = self.line();
                    cases.push(Case {
                        label: None,
                        body,
                        line,
                    });
                }
                Token::Punct(Punct::RBrace) => {
                    self.advance()?;
                    break;
                }
                _ => return Err(self.unexpected("`case`, `default` or `}`")?),
            }
        }
        let line = self.line();
        Ok(Stmt::Switch { value, cases, line })
    }

    // `exp`: the binary-operator levels, lowest first. `&&` and `||`
    // share a level, as do all comparisons; `%` binds tighter than `*`
    // and `/`.
    fn expression(&mut self) -> Result<Expr> {
        if self.expressions >= MAX_EXPRESSION_DEPTH {
            return Err(self.too_deep("expressions"));
        }
        self.expressions += 1;
        let expression = self.binary(Level::Logical);
        self.expressions -= 1;
        expression
    }

    fn binary(&mut self, level: Level) -> Result<Expr> {
        let mut left = match level.tighter() {
            Some(tighter) => self.binary(tighter)?,
            None => self.operand()?,
        };
        loop {
            let Some(op) = level.operator(self.peek()?) else {
                return Ok(left);
            };
            self.advance()?;
            self.operators += 1;
            if self.operators > MAX_OPERATORS {
                return Err(self.too_deep("operators"));
            }
            let right = match level.tighter() {
                Some(tighter) => self.binary(tighter)?,
                None => self.operand()?,
            };
            // Every binary production but `%` reduces in a state that can
            // still shift an operator, so the lookahead has been fetched
            // (the tighter level peeked at it): the line is the token after
            // the right operand. `exp '%' exp` is a default reduction: no
            // peek happened since the operand, whose line it inherits.
            let line = self.line();
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                line,
            };
        }
    }

    /// An operand of a binary operator: `null` (an `exp` alternative), an
    /// assignment `lvalue = exp`, or a `unaryexp`.
    fn operand(&mut self) -> Result<Expr> {
        if self.at_keyword(Keyword::Null)? {
            self.advance()?;
            // `exp: tNULL`: default reduction, the token's line.
            let line = self.line();
            return Ok(Expr {
                kind: ExprKind::Null,
                line,
            });
        }
        let (expr, bare_lvalue) = self.unary()?;
        if bare_lvalue && self.at_punct(Punct::Assign)? {
            self.advance()?;
            let ExprKind::Var(target) = expr.kind else {
                unreachable!("a bare lvalue");
            };
            // `exp: lvalue '=' exp`: the right-hand side is a whole
            // expression (every operator binds tighter than `=`); the
            // production reduces with the lookahead fetched.
            let value = Box::new(self.expression()?);
            let line = self.line();
            return Ok(Expr {
                kind: ExprKind::Assign { target, value },
                line,
            });
        }
        Ok(expr)
    }

    /// `unaryexp`. The flag says whether the result is an `lvalue` as
    /// such (rule `unarypostfixexp: lvalue`), the only thing `=` accepts
    /// on its left.
    fn unary(&mut self) -> Result<(Expr, bool)> {
        if self.expressions >= MAX_EXPRESSION_DEPTH {
            return Err(self.too_deep("expressions"));
        }
        self.expressions += 1;
        let unary = self.unary_body();
        self.expressions -= 1;
        unary
    }

    fn unary_body(&mut self) -> Result<(Expr, bool)> {
        match self.peek()? {
            Token::Punct(Punct::Minus) => {
                self.advance()?;
                let (operand, _) = self.unary()?;
                // `'-' unaryexp`: default reduction, inherits the line.
                let line = self.line();
                Ok((
                    Expr {
                        kind: ExprKind::Neg(Box::new(operand)),
                        line,
                    },
                    false,
                ))
            }
            Token::Punct(Punct::Not) => {
                self.advance()?;
                let (operand, _) = self.unary()?;
                let line = self.line();
                Ok((
                    Expr {
                        kind: ExprKind::Not(Box::new(operand)),
                        line,
                    },
                    false,
                ))
            }
            Token::Punct(Punct::LParen) => {
                self.advance()?;
                if let Token::Keyword(Keyword::Var | Keyword::Global | Keyword::Const) =
                    self.peek()?
                {
                    // `'(' type ')' unaryexp`: a cast, which generates
                    // nothing.
                    self.advance()?;
                    self.expect_punct(Punct::RParen)?;
                    let (operand, _) = self.unary()?;
                    let line = self.line();
                    return Ok((
                        Expr {
                            kind: ExprKind::Cast(Box::new(operand)),
                            line,
                        },
                        false,
                    ));
                }
                // `'(' exp ')'`: the inner node keeps its own line.
                let inner = self.expression()?;
                self.expect_punct(Punct::RParen)?;
                Ok((inner, false))
            }
            _ => self.postfix(),
        }
    }

    /// `unarypostfixexp`, with the lvalue flag of [`Parser::unary`].
    fn postfix(&mut self) -> Result<(Expr, bool)> {
        let token = self.advance()?;
        let pos = self.lexer.last_pos();
        // Constants reduce by default: their own line.
        let line = self.line();
        let kind = match token {
            Token::Int(v) => ExprKind::Int(v),
            Token::Bool(v) => ExprKind::Bool(v),
            Token::Float(v) => ExprKind::Float(v),
            Token::Str(v) => ExprKind::Str(v),
            Token::Punct(Punct::LParen) => {
                // Only reached for `case` labels; elsewhere `unary` handles
                // the parenthesis (and the cast).
                let inner = self.expression()?;
                self.expect_punct(Punct::RParen)?;
                return Ok((inner, false));
            }
            Token::Keyword(Keyword::New) => {
                let (name, pos) = self.expect_ident()?;
                // `tNEW tIDENTIFIER` must look for a `(`: without one the
                // line is the next token's; `tNEW tIDENTIFIER '(' exps ')'`
                // reduces by default after the parenthesis.
                let args = if self.eat_punct(Punct::LParen)? {
                    let args = self.arguments()?;
                    self.expect_punct(Punct::RParen)?;
                    args
                } else {
                    Vec::new()
                };
                let line = self.line();
                return Ok((
                    Expr {
                        kind: ExprKind::New {
                            name,
                            pos,
                            args,
                            external: false,
                        },
                        line,
                    },
                    false,
                ));
            }
            Token::Ident(name) => return self.postfix_identifier(name, pos),
            other => return Err(self.syntax_error(&other, "an expression")),
        };
        Ok((Expr { kind, line }, false))
    }

    /// What follows an identifier: a call, a method call, a member access
    /// or the variable itself.
    fn postfix_identifier(&mut self, name: String, pos: Pos) -> Result<(Expr, bool)> {
        match self.peek()? {
            Token::Punct(Punct::LParen) => {
                self.advance()?;
                let args = self.arguments()?;
                self.expect_punct(Punct::RParen)?;
                // `tIDENTIFIER '(' exps ')'`: default reduction, the line
                // of the closing parenthesis.
                let line = self.line();
                Ok((
                    Expr {
                        kind: ExprKind::Call {
                            name,
                            pos,
                            args,
                            external: false,
                        },
                        line,
                    },
                    false,
                ))
            }
            Token::Punct(Punct::Dot) => {
                self.advance()?;
                let (member, _) = self.expect_ident()?;
                if self.eat_punct(Punct::LParen)? {
                    let args = self.arguments()?;
                    self.expect_punct(Punct::RParen)?;
                    // `tIDENTIFIER '.' tIDENTIFIER '(' exps ')'`: default
                    // reduction; the method name constant is made in the
                    // same reduction, with the same line.
                    let line = self.line();
                    let method = Box::new(Expr {
                        kind: ExprKind::Str(member.into_bytes()),
                        line,
                    });
                    return Ok((
                        Expr {
                            kind: ExprKind::MethodCall {
                                object: name,
                                pos,
                                method,
                                args,
                            },
                            line,
                        },
                        false,
                    ));
                }
                // `lvalue: tIDENTIFIER '.' tIDENTIFIER` had to look for a
                // `(`: the line is the next token's, for the member constant
                // too.
                let line = self.line();
                let member = Box::new(Expr {
                    kind: ExprKind::Str(member.into_bytes()),
                    line,
                });
                Ok((
                    Expr {
                        kind: ExprKind::Var(Lvalue {
                            name,
                            pos,
                            member: Some(member),
                            line,
                        }),
                        line,
                    },
                    true,
                ))
            }
            Token::Punct(Punct::LBracket) => {
                self.advance()?;
                let index = Box::new(self.expression()?);
                self.expect_punct(Punct::RBracket)?;
                if self.eat_punct(Punct::LParen)? {
                    let args = self.arguments()?;
                    self.expect_punct(Punct::RParen)?;
                    let line = self.line();
                    return Ok((
                        Expr {
                            kind: ExprKind::MethodCall {
                                object: name,
                                pos,
                                method: index,
                                args,
                            },
                            line,
                        },
                        false,
                    ));
                }
                // `lvalue: tIDENTIFIER '[' exp ']'`: late, like `a.b`.
                let line = self.line();
                Ok((
                    Expr {
                        kind: ExprKind::Var(Lvalue {
                            name,
                            pos,
                            member: Some(index),
                            line,
                        }),
                        line,
                    },
                    true,
                ))
            }
            _ => {
                // `lvalue: tIDENTIFIER`: the automaton needed the lookahead
                // to rule out `.`, `[` and `(`: the line is the next
                // token's.
                let line = self.line();
                Ok((
                    Expr {
                        kind: ExprKind::Var(Lvalue {
                            name,
                            pos,
                            member: None,
                            line,
                        }),
                        line,
                    },
                    true,
                ))
            }
        }
    }

    // `exps: /* empty */ | neexps`, `neexps: exp | exp ',' neexps`.
    fn arguments(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();
        if self.at_punct(Punct::RParen)? {
            return Ok(args);
        }
        loop {
            args.push(self.expression()?);
            if !self.eat_punct(Punct::Comma)? {
                return Ok(args);
            }
        }
    }
}

/// The binary-operator precedence levels, from the loosest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// `&&` `||`
    Logical,
    /// `==` `!=` `===` `!==` `<` `>` `<=` `>=`
    Comparison,
    /// `+` `-`
    Additive,
    /// `*` `/`
    Multiplicative,
    /// `%`
    Modulo,
}

impl Level {
    fn tighter(self) -> Option<Level> {
        match self {
            Level::Logical => Some(Level::Comparison),
            Level::Comparison => Some(Level::Additive),
            Level::Additive => Some(Level::Multiplicative),
            Level::Multiplicative => Some(Level::Modulo),
            Level::Modulo => None,
        }
    }

    fn operator(self, token: &Token) -> Option<BinOp> {
        let Token::Punct(punct) = token else {
            return None;
        };
        let op = match (self, punct) {
            (Level::Logical, Punct::AndAnd) => BinOp::And,
            (Level::Logical, Punct::OrOr) => BinOp::Or,
            (Level::Comparison, Punct::EqEq) => BinOp::Eq,
            (Level::Comparison, Punct::Ne) => BinOp::Ne,
            (Level::Comparison, Punct::EqEqEq) => BinOp::StrictEq,
            (Level::Comparison, Punct::NeEq) => BinOp::StrictNe,
            (Level::Comparison, Punct::Lt) => BinOp::Lt,
            (Level::Comparison, Punct::Gt) => BinOp::Gt,
            (Level::Comparison, Punct::Le) => BinOp::Le,
            (Level::Comparison, Punct::Ge) => BinOp::Ge,
            (Level::Additive, Punct::Plus) => BinOp::Add,
            (Level::Additive, Punct::Minus) => BinOp::Sub,
            (Level::Multiplicative, Punct::Star) => BinOp::Mul,
            (Level::Multiplicative, Punct::Slash) => BinOp::Div,
            (Level::Modulo, Punct::Percent) => BinOp::Mod,
            _ => return None,
        };
        Some(op)
    }
}

fn ext_type(token: &Token) -> Option<ExtType> {
    Some(match token {
        Token::Keyword(Keyword::Bool) => ExtType::Bool,
        Token::Keyword(Keyword::Long) => ExtType::Long,
        Token::Keyword(Keyword::Byte) => ExtType::Byte,
        Token::Keyword(Keyword::String) => ExtType::String,
        Token::Keyword(Keyword::Float) => ExtType::Float,
        Token::Keyword(Keyword::Double) => ExtType::Double,
        Token::Keyword(Keyword::MemBuffer) => ExtType::MemBuffer,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(source: &str) -> Result<Script> {
        let mut includes = |_: &[u8]| -> Option<Vec<u8>> { None };
        parse(source.as_bytes().to_vec(), "t.script", &mut includes)
    }

    fn only_expr(source: &str) -> Expr {
        let script = parse_str(source).unwrap();
        match &script.items[0] {
            TopLevel::Statements(stmts) => match &stmts[0] {
                Stmt::Expr(e) => e.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    fn op(e: &Expr) -> BinOp {
        match &e.kind {
            ExprKind::Binary { op, .. } => *op,
            other => panic!("{other:?}"),
        }
    }

    fn operands(e: &Expr) -> (&Expr, &Expr) {
        match &e.kind {
            ExprKind::Binary { left, right, .. } => (left, right),
            other => panic!("{other:?}"),
        }
    }

    fn too_deep(source: &str) {
        let error = parse_str(source).expect_err("too deep");
        assert!(
            error.message.contains("nested too deeply"),
            "{}",
            error.message
        );
    }

    // Hostile sources must end in an error, not in a stack overflow.
    #[test]
    fn unary_nesting_is_bounded() {
        too_deep(&format!("var a = {}1;", "!".repeat(100_000)));
        too_deep(&format!("var a = {}1;", "-".repeat(100_000)));
    }

    #[test]
    fn parenthesis_nesting_is_bounded() {
        too_deep(&format!("var a = {}1;", "(".repeat(100_000)));
    }

    #[test]
    fn block_nesting_is_bounded() {
        too_deep(&format!("on \"e\" {}", "{".repeat(100_000)));
    }

    #[test]
    fn operator_chains_are_bounded() {
        too_deep(&format!("var a = 1{};", " + 1".repeat(100_000)));
    }

    #[test]
    fn else_chains_are_bounded() {
        too_deep(&format!(
            "on \"e\" {{ if (a) a = 1; {} }}",
            "else if (a) a = 1; ".repeat(100_000)
        ));
    }

    #[test]
    fn what_a_person_writes_is_inside_the_bounds() {
        let chain = format!("if (a) a = 1; {}", "else if (a) a = 1; ".repeat(80));
        assert!(parse_str(&format!("on \"e\" {{ {chain} }}")).is_ok());
        let sum = format!("on \"e\" {{ var a = 1{}; }}", " + 1".repeat(500));
        assert!(parse_str(&sum).is_ok());
    }

    #[test]
    fn precedence_levels_are_the_originals() {
        // `a || b && c` is `(a || b) && c`.
        let e = only_expr("a || b && c;");
        assert_eq!(op(&e), BinOp::And);
        assert_eq!(op(operands(&e).0), BinOp::Or);
        // `a == b < c` is `(a == b) < c`.
        let e = only_expr("a == b < c;");
        assert_eq!(op(&e), BinOp::Lt);
        assert_eq!(op(operands(&e).0), BinOp::Eq);
        // `a * b % c` is `a * (b % c)`; `a % b * c` is `(a % b) * c`.
        let e = only_expr("a * b % c;");
        assert_eq!(op(&e), BinOp::Mul);
        assert_eq!(op(operands(&e).1), BinOp::Mod);
        let e = only_expr("a % b * c;");
        assert_eq!(op(&e), BinOp::Mul);
        assert_eq!(op(operands(&e).0), BinOp::Mod);
        // `a + b = c` is `a + (b = c)`.
        let e = only_expr("a + b = c;");
        assert_eq!(op(&e), BinOp::Add);
        assert!(matches!(operands(&e).1.kind, ExprKind::Assign { .. }));
        // `-a * b` is `(-a) * b`.
        let e = only_expr("-a * b;");
        assert_eq!(op(&e), BinOp::Mul);
        assert!(matches!(operands(&e).0.kind, ExprKind::Neg(_)));
        // `a = b = c` nests to the right.
        let e = only_expr("a = b = c;");
        let ExprKind::Assign { value, .. } = &e.kind else {
            panic!()
        };
        assert!(matches!(value.kind, ExprKind::Assign { .. }));
    }

    #[test]
    fn null_is_an_exp_not_a_unary() {
        assert!(parse_str("x = null;").is_ok());
        assert!(parse_str("x = a + null;").is_ok());
        assert!(parse_str("f(null);").is_ok());
        assert!(parse_str("x = (null);").is_ok());
        assert!(parse_str("x = -null;").is_err());
        assert!(parse_str("x = !null;").is_err());
        assert!(parse_str("switch (a) { case null: ; }").is_err());
    }

    #[test]
    fn lvalues_are_one_level() {
        assert!(parse_str("a.b = 1;").is_ok());
        assert!(parse_str("a[1] = 1;").is_ok());
        assert!(parse_str("a.b.c = 1;").is_err());
        assert!(parse_str("a[1][2] = 1;").is_err());
        assert!(parse_str("a.b[1] = 1;").is_err());
        assert!(parse_str("f().x = 1;").is_err());
        assert!(parse_str("(a) = 1;").is_err());
        assert!(parse_str("-a = 1;").is_err());
        assert!(parse_str("a.b.M();").is_err());
        assert!(parse_str("a.M(1, 2);").is_ok());
        assert!(parse_str("a[\"M\"](1, 2);").is_ok());
        assert!(parse_str("x.string = 1;").is_err());
    }

    #[test]
    fn case_labels_are_postfix_expressions() {
        assert!(
            parse_str("switch (a) { case 1: case x: case x.y: case f(): case (0-1): ; }").is_ok()
        );
        assert!(parse_str("switch (a) { case -1: ; }").is_err());
        assert!(parse_str("switch (a) { default: }").is_err());
        assert!(parse_str("switch (a) { default: ; case 1: ; }").is_err());
        assert!(parse_str("switch (a) { }").is_ok());
        assert!(parse_str("switch (a) { case 1: default: ; }").is_ok());
    }

    #[test]
    fn for_needs_an_initializer_and_a_condition() {
        assert!(parse_str("for (var i = 0, j = 1, k; i < 1; i = i + 1, j = 0) ;").is_ok());
        assert!(parse_str("for (i = 0; i < 1; ) ;").is_ok());
        assert!(parse_str("for (; i < 1; ) ;").is_err());
        assert!(parse_str("for (i = 0; ; ) ;").is_err());
    }

    #[test]
    fn no_do_while_increment_or_compound_assignment() {
        assert!(parse_str("do { } while (a);").is_err());
        assert!(parse_str("a++;").is_err());
        assert!(parse_str("a += 1;").is_err());
        assert!(parse_str("break").is_err());
    }

    #[test]
    fn events_functions_and_externals() {
        let script =
            parse_str("on \"X\" Sleep(1);\nfunction f(var a, b) { }\nmethod m() { }\nexternal \"k.dll\" cdecl long f2(long, string s);\nexternal \"k.dll\" g();")
                .unwrap();
        assert_eq!(script.items.len(), 5);
        let TopLevel::External(e) = &script.items[3] else {
            panic!()
        };
        assert_eq!(e.call, CallConv::CDecl);
        assert_eq!(e.returns, ExtType::Long);
        assert_eq!(e.params, vec![ExtType::Long, ExtType::String]);
        let TopLevel::External(e) = &script.items[4] else {
            panic!()
        };
        assert_eq!(e.call, CallConv::StdCall);
        assert_eq!(e.returns, ExtType::Void);
        assert!(e.params.is_empty());
    }

    #[test]
    fn syntax_errors_carry_file_line_and_column() {
        let err = parse_str("var x = 1;\n  x = ;\n").unwrap_err();
        assert_eq!(err.file, "t.script");
        assert_eq!((err.line, err.column), (2, 7));
        assert!(err.message.starts_with("syntax error"), "{err}");
    }
}
