//! The syntax tree the parser builds and the code generator walks.
//!
//! Every node that produces code carries the *recorded line* the original
//! compiler stored when it reduced the matching production (see
//! `docs/compiler.md`, "Line numbers"); nodes that can fail a check also
//! carry their real position for error messages.

use crate::error::Pos;

/// A whole script.
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// The top-level items, in source order.
    pub items: Vec<TopLevel>,
    /// The recorded line at the end of the input: 1 plus the number of
    /// newlines of the main file. Used for the final `RET` and for the
    /// second and later names of a multi-name declaration.
    pub line: u32,
    /// The files opened while parsing: the main file, then the includes
    /// by the name in their directive; [`Pos::file`] indexes this.
    pub files: Vec<String>,
}

/// A top-level item.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    /// A `function` or `method`.
    Function(Function),
    /// A run of statements.
    Statements(Vec<Stmt>),
    /// An `on "..."` event handler.
    Event(Event),
    /// An `external` declaration.
    External(External),
}

/// A `function` or a `method`.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    /// The name.
    pub name: String,
    /// Where the name is, for errors.
    pub pos: Pos,
    /// The parameters, in order.
    pub params: Vec<Param>,
    /// The body.
    pub body: Vec<Stmt>,
    /// `method` rather than `function`.
    pub is_method: bool,
    /// Recorded line: that of the closing brace.
    pub line: u32,
}

/// A parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// The name.
    pub name: String,
    /// Where the name is, for errors.
    pub pos: Pos,
    /// Recorded line: that of the name.
    pub line: u32,
}

/// An event handler.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// The event name, as written between the quotes.
    pub name: Vec<u8>,
    /// The body: any single statement.
    pub body: Box<Stmt>,
    /// Recorded line: inherited from the body.
    pub line: u32,
}

/// Calling convention of an `external`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum CallConv {
    StdCall,
    CDecl,
    ThisCall,
}

/// Parameter or return type of an `external`, with the code written in
/// the externals table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ExtType {
    Void = 0,
    Bool = 1,
    Long = 2,
    Byte = 3,
    String = 4,
    Float = 5,
    Double = 6,
    MemBuffer = 7,
}

/// An `external "dll" [callconv] [type] name(types);` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct External {
    /// The DLL name.
    pub dll: Vec<u8>,
    /// The function name.
    pub name: String,
    /// Where the name is, for errors.
    pub pos: Pos,
    /// Calling convention (`stdcall` by default).
    pub call: CallConv,
    /// Return type (`void` by default).
    pub returns: ExtType,
    /// Parameter types (names are discarded).
    pub params: Vec<ExtType>,
}

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `;`
    Empty,
    /// `return;` or `return e;`
    Return {
        /// The value.
        value: Option<Expr>,
        /// Where the statement starts, for errors.
        pos: Pos,
        /// Recorded line: that of the `;`.
        line: u32,
    },
    /// `if (c) S [else S2]`
    If {
        /// The condition.
        cond: Expr,
        /// The `then` branch.
        then: Box<Stmt>,
        /// The `else` branch.
        otherwise: Option<Box<Stmt>>,
        /// Recorded line: without `else`, the token after the statement;
        /// with `else`, inherited from the `else` branch.
        line: u32,
    },
    /// `while (c) S`
    While {
        /// The condition.
        cond: Expr,
        /// The body.
        body: Box<Stmt>,
        /// Recorded line: inherited from the body.
        line: u32,
    },
    /// `for (inits; cond; updates) S`
    For {
        /// The initializers, in order (at least one).
        inits: Vec<ForInit>,
        /// The condition.
        cond: Expr,
        /// The updates, in order.
        updates: Vec<Expr>,
        /// The body.
        body: Box<Stmt>,
        /// Recorded line: inherited from the body.
        line: u32,
    },
    /// `{ ... }`
    Block(Vec<Stmt>),
    /// `var`, `global` or `const` declaration.
    Decl(Decl),
    /// An expression statement.
    Expr(Expr),
    /// `break;`
    Break {
        /// Where the token is, for errors.
        pos: Pos,
        /// Recorded line: that of the token.
        line: u32,
    },
    /// `continue;`
    Continue {
        /// Where the token is, for errors.
        pos: Pos,
        /// Recorded line: that of the token.
        line: u32,
    },
    /// `switch (e) { cases }`
    Switch {
        /// The value switched on.
        value: Expr,
        /// The cases, the `default` (if any) last.
        cases: Vec<Case>,
        /// Recorded line: that of the closing brace.
        line: u32,
    },
}

/// The kind of a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum DeclKind {
    Var,
    Global,
    Const,
}

/// A declaration `var a, b = e;` (one or more names, one shared
/// initializer).
#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    /// `var`, `global` or `const`.
    pub kind: DeclKind,
    /// Recorded line of the keyword.
    pub kind_line: u32,
    /// The names, with their positions for errors.
    pub names: Vec<(String, Pos)>,
    /// The initializer, assigned to every name.
    pub init: Option<Expr>,
    /// Recorded line of the declaration: the `;` for a statement, the
    /// token after the initializer for a `for` initializer. The second
    /// and later names use the script's final line instead.
    pub line: u32,
}

/// An initializer of a `for` loop.
#[derive(Debug, Clone, PartialEq)]
pub enum ForInit {
    /// `var x [= e]` (one name).
    Decl(Decl),
    /// An expression.
    Expr(Expr),
}

/// A `case` or the `default` of a `switch`.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The label; `None` for `default`.
    pub label: Option<Expr>,
    /// The body.
    pub body: Vec<Stmt>,
    /// Recorded line: the token after the body.
    pub line: u32,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    And,
    Or,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Gt,
    Le,
    Ge,
}

/// An lvalue: `a`, `a.b` or `a[e]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Lvalue {
    /// The variable.
    pub name: String,
    /// Where the name is, for errors.
    pub pos: Pos,
    /// The member: a string constant for `a.b`, any expression for `a[e]`.
    pub member: Option<Box<Expr>>,
    /// Recorded line: the token after the lvalue.
    pub line: u32,
}

/// An expression with its recorded line.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    /// What it is.
    pub kind: ExprKind,
    /// Recorded line.
    pub line: u32,
}

/// The kinds of expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    /// An integer constant.
    Int(i32),
    /// `true` or `false`.
    Bool(bool),
    /// A float constant.
    Float(f64),
    /// A string constant (raw bytes).
    Str(Vec<u8>),
    /// `null`
    Null,
    /// `-e`
    Neg(Box<Expr>),
    /// `!e`
    Not(Box<Expr>),
    /// `(var) e`: parsed, generates nothing.
    Cast(Box<Expr>),
    /// A variable or member read.
    Var(Lvalue),
    /// `lvalue = e`
    Assign {
        /// The target.
        target: Lvalue,
        /// The value.
        value: Box<Expr>,
    },
    /// `l op r`
    Binary {
        /// The operator.
        op: BinOp,
        /// The left operand.
        left: Box<Expr>,
        /// The right operand.
        right: Box<Expr>,
    },
    /// `f(args)`
    Call {
        /// The function name.
        name: String,
        /// Where the name is, for errors.
        pos: Pos,
        /// The arguments, in source order.
        args: Vec<Expr>,
        /// Set by the symbol pass: the callee is a native or an
        /// `external`, so the call is an `EXTERNAL_CALL`.
        external: bool,
    },
    /// `o.m(args)` or `o[e](args)`
    MethodCall {
        /// The object variable.
        object: String,
        /// Where the object name is, for errors.
        pos: Pos,
        /// The method: a string constant for `o.m`, any expression for
        /// `o[e]`.
        method: Box<Expr>,
        /// The arguments, in source order.
        args: Vec<Expr>,
    },
    /// `new C` or `new C(args)`
    New {
        /// The class (constructor) name.
        name: String,
        /// Where the name is, for errors.
        pos: Pos,
        /// The arguments, in source order (empty without parentheses).
        args: Vec<Expr>,
        /// Set by the symbol pass, as for [`ExprKind::Call`].
        external: bool,
    },
}
