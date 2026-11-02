//! The checks between parsing and code generation: placement of `break`,
//! `continue` and `return` (the original's "weed" pass), then the two
//! symbol passes that validate names and decide which calls are
//! `EXTERNAL_CALL`s.
//!
//! Written with reference to `compiler/weed.c` and `compiler/symbol.c` of
//! Wintermute Engine 1.x (MIT License, Copyright (c) 2003-2013 Jan Nedoma
//! and contributors); see `NOTICE`. Specified in `docs/compiler.md`,
//! "Symbols, scoping and checks".

use std::collections::HashMap;

use crate::Natives;
use crate::ast::*;
use crate::error::{Error, Pos, Result};

/// What a name stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Variable,
    Function,
}

#[derive(Debug, Clone, Copy)]
struct Symbol {
    kind: Kind,
    /// A native or an `external`: calls compile to `EXTERNAL_CALL`.
    external: bool,
}

/// A symbol table with a parent: the root (natives and top-level names),
/// and one child per function, method or event body. Blocks open no
/// scope.
struct Table {
    names: HashMap<String, Symbol>,
    parent: Option<usize>,
}

struct Checker {
    files: Vec<String>,
    tables: Vec<Table>,
}

/// Runs the checks on `script`, marking the calls that are external.
pub fn check(script: &mut Script, natives: &Natives) -> Result<()> {
    weed(script)?;
    let mut root = Table {
        names: HashMap::new(),
        parent: None,
    };
    for name in &natives.variables {
        root.names.insert(
            name.clone(),
            Symbol {
                kind: Kind::Variable,
                external: true,
            },
        );
    }
    for name in &natives.functions {
        root.names.insert(
            name.clone(),
            Symbol {
                kind: Kind::Function,
                external: true,
            },
        );
    }
    let mut checker = Checker {
        files: script.files.clone(),
        tables: vec![root],
    };
    // Pass 1: declarations in source order, variable references checked as
    // they are met. One child table per body, remembered by item.
    let mut bodies: Vec<Option<usize>> = Vec::with_capacity(script.items.len());
    for item in &script.items {
        bodies.push(checker.pass1_item(item)?);
    }
    // Pass 2: calls, constructors and method receivers, which may name
    // anything declared anywhere.
    for (item, table) in script.items.iter_mut().zip(&bodies) {
        match item {
            TopLevel::Function(f) => checker.pass2_stmts(&mut f.body, table.expect("a body"))?,
            TopLevel::Event(e) => checker.pass2_stmt(&mut e.body, table.expect("a body"))?,
            TopLevel::Statements(stmts) => checker.pass2_stmts(stmts, 0)?,
            TopLevel::External(_) => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- weed

/// "Invalid break", "Invalid continue" and "Event handler cannot return a
/// value", purely syntactic.
fn weed(script: &Script) -> Result<()> {
    for item in &script.items {
        match item {
            TopLevel::Function(f) => weed_stmts(script, &f.body, false, false, false)?,
            TopLevel::Event(e) => weed_stmt(script, &e.body, false, false, true)?,
            TopLevel::Statements(stmts) => weed_stmts(script, stmts, false, false, false)?,
            TopLevel::External(_) => {}
        }
    }
    Ok(())
}

fn weed_stmts(
    script: &Script,
    stmts: &[Stmt],
    in_loop: bool,
    in_switch: bool,
    in_event: bool,
) -> Result<()> {
    for stmt in stmts {
        weed_stmt(script, stmt, in_loop, in_switch, in_event)?;
    }
    Ok(())
}

fn weed_stmt(
    script: &Script,
    stmt: &Stmt,
    in_loop: bool,
    in_switch: bool,
    in_event: bool,
) -> Result<()> {
    match stmt {
        Stmt::Return {
            value: Some(_),
            pos,
            ..
        } if in_event => Err(error(
            &script.files,
            pos,
            "Event handler cannot return a value",
        )),
        Stmt::Break { pos, .. } if !in_loop && !in_switch => {
            Err(error(&script.files, pos, "Invalid break"))
        }
        Stmt::Continue { pos, .. } if !in_loop => {
            Err(error(&script.files, pos, "Invalid continue"))
        }
        Stmt::If {
            then, otherwise, ..
        } => {
            weed_stmt(script, then, in_loop, in_switch, in_event)?;
            if let Some(otherwise) = otherwise {
                weed_stmt(script, otherwise, in_loop, in_switch, in_event)?;
            }
            Ok(())
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            weed_stmt(script, body, true, in_switch, in_event)
        }
        Stmt::Block(stmts) => weed_stmts(script, stmts, in_loop, in_switch, in_event),
        Stmt::Switch { cases, .. } => {
            for case in cases {
                weed_stmts(script, &case.body, in_loop, true, in_event)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn error(files: &[String], pos: &Pos, message: impl Into<String>) -> Error {
    Error {
        file: files[pos.file].clone(),
        line: pos.line,
        column: pos.column,
        message: message.into(),
    }
}

// ---------------------------------------------------------------- symbols

impl Checker {
    fn error(&self, pos: &Pos, message: impl Into<String>) -> Error {
        error(&self.files, pos, message)
    }

    fn lookup(&self, mut table: usize, name: &str) -> Option<Symbol> {
        loop {
            if let Some(symbol) = self.tables[table].names.get(name) {
                return Some(*symbol);
            }
            table = self.tables[table].parent?;
        }
    }

    fn child(&mut self, parent: usize) -> usize {
        self.tables.push(Table {
            names: HashMap::new(),
            parent: Some(parent),
        });
        self.tables.len() - 1
    }

    fn declare(&mut self, table: usize, name: &str, kind: Kind, external: bool) {
        self.tables[table]
            .names
            .insert(name.to_string(), Symbol { kind, external });
    }

    /// Pass 1 over one top-level item; returns the table of its body.
    fn pass1_item(&mut self, item: &TopLevel) -> Result<Option<usize>> {
        match item {
            TopLevel::Function(f) => {
                if self.tables[0].names.contains_key(&f.name) {
                    return Err(self.error(
                        &f.pos,
                        format!("Duplicate declaration of function '{}'", f.name),
                    ));
                }
                self.declare(0, &f.name, Kind::Function, false);
                let table = self.child(0);
                for param in &f.params {
                    if self.tables[table].names.contains_key(&param.name) {
                        return Err(self.error(
                            &param.pos,
                            format!("Duplicate declaration of formal '{}'", param.name),
                        ));
                    }
                    self.declare(table, &param.name, Kind::Variable, false);
                }
                self.pass1_stmts(&f.body, table)?;
                Ok(Some(table))
            }
            TopLevel::Event(e) => {
                let table = self.child(0);
                self.pass1_stmt(&e.body, table)?;
                Ok(Some(table))
            }
            TopLevel::External(e) => {
                if self.tables[0].names.contains_key(&e.name) {
                    return Err(self.error(
                        &e.pos,
                        format!("Duplicate declaration of function '{}'", e.name),
                    ));
                }
                self.declare(0, &e.name, Kind::Function, true);
                Ok(None)
            }
            TopLevel::Statements(stmts) => {
                self.pass1_stmts(stmts, 0)?;
                Ok(None)
            }
        }
    }

    fn pass1_stmts(&mut self, stmts: &[Stmt], table: usize) -> Result<()> {
        for stmt in stmts {
            self.pass1_stmt(stmt, table)?;
        }
        Ok(())
    }

    fn pass1_stmt(&mut self, stmt: &Stmt, table: usize) -> Result<()> {
        match stmt {
            Stmt::Empty | Stmt::Break { .. } | Stmt::Continue { .. } => Ok(()),
            Stmt::Return { value, .. } => value
                .as_ref()
                .map_or(Ok(()), |value| self.pass1_expr(value, table)),
            Stmt::Expr(expr) => self.pass1_expr(expr, table),
            Stmt::Decl(decl) => self.pass1_decl(decl, table),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.pass1_expr(cond, table)?;
                self.pass1_stmt(then, table)?;
                if let Some(otherwise) = otherwise {
                    self.pass1_stmt(otherwise, table)?;
                }
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                self.pass1_expr(cond, table)?;
                self.pass1_stmt(body, table)
            }
            Stmt::For {
                inits,
                cond,
                updates,
                body,
                ..
            } => {
                for init in inits {
                    match init {
                        ForInit::Decl(decl) => self.pass1_decl(decl, table)?,
                        ForInit::Expr(expr) => self.pass1_expr(expr, table)?,
                    }
                }
                self.pass1_expr(cond, table)?;
                for update in updates {
                    self.pass1_expr(update, table)?;
                }
                self.pass1_stmt(body, table)
            }
            Stmt::Block(stmts) => self.pass1_stmts(stmts, table),
            Stmt::Switch { value, cases, .. } => {
                self.pass1_expr(value, table)?;
                for case in cases {
                    if let Some(label) = &case.label {
                        self.pass1_expr(label, table)?;
                    }
                    self.pass1_stmts(&case.body, table)?;
                }
                Ok(())
            }
        }
    }

    /// A declaration: for each name, the initializer is checked first (so
    /// that `var i = i;` fails), then the name is declared.
    fn pass1_decl(&mut self, decl: &Decl, table: usize) -> Result<()> {
        for (name, pos) in &decl.names {
            if let Some(init) = &decl.init {
                self.pass1_expr(init, table)?;
            }
            if self.tables[table].names.contains_key(name) {
                return Err(self.error(pos, format!("Duplicate declaration of variable '{name}'")));
            }
            self.declare(table, name, Kind::Variable, false);
        }
        Ok(())
    }

    fn pass1_lvalue(&mut self, lvalue: &Lvalue, table: usize) -> Result<()> {
        match self.lookup(table, &lvalue.name) {
            None => Err(self.error(
                &lvalue.pos,
                format!("Variable '{}' is referenced but not defined", lvalue.name),
            )),
            Some(Symbol {
                kind: Kind::Function,
                ..
            }) => Err(self.error(
                &lvalue.pos,
                format!("Identifier '{}' is not a variable", lvalue.name),
            )),
            Some(_) => match &lvalue.member {
                Some(member) => self.pass1_expr(member, table),
                None => Ok(()),
            },
        }
    }

    fn pass1_expr(&mut self, expr: &Expr, table: usize) -> Result<()> {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Null => Ok(()),
            ExprKind::Neg(e) | ExprKind::Not(e) | ExprKind::Cast(e) => self.pass1_expr(e, table),
            ExprKind::Var(lvalue) => self.pass1_lvalue(lvalue, table),
            ExprKind::Assign { target, value } => {
                self.pass1_lvalue(target, table)?;
                self.pass1_expr(value, table)
            }
            ExprKind::Binary { left, right, .. } => {
                self.pass1_expr(left, table)?;
                self.pass1_expr(right, table)
            }
            ExprKind::Call { args, .. }
            | ExprKind::New { args, .. }
            | ExprKind::MethodCall { args, .. } => {
                for arg in args {
                    self.pass1_expr(arg, table)?;
                }
                Ok(())
            }
        }
    }

    fn pass2_stmts(&self, stmts: &mut [Stmt], table: usize) -> Result<()> {
        for stmt in stmts {
            self.pass2_stmt(stmt, table)?;
        }
        Ok(())
    }

    fn pass2_stmt(&self, stmt: &mut Stmt, table: usize) -> Result<()> {
        match stmt {
            Stmt::Empty | Stmt::Break { .. } | Stmt::Continue { .. } => Ok(()),
            Stmt::Return { value, .. } => value
                .as_mut()
                .map_or(Ok(()), |value| self.pass2_expr(value, table)),
            Stmt::Expr(expr) => self.pass2_expr(expr, table),
            Stmt::Decl(decl) => decl
                .init
                .as_mut()
                .map_or(Ok(()), |init| self.pass2_expr(init, table)),
            Stmt::If {
                cond,
                then,
                otherwise,
                ..
            } => {
                self.pass2_expr(cond, table)?;
                self.pass2_stmt(then, table)?;
                if let Some(otherwise) = otherwise {
                    self.pass2_stmt(otherwise, table)?;
                }
                Ok(())
            }
            Stmt::While { cond, body, .. } => {
                self.pass2_expr(cond, table)?;
                self.pass2_stmt(body, table)
            }
            Stmt::For {
                inits,
                cond,
                updates,
                body,
                ..
            } => {
                for init in inits {
                    match init {
                        ForInit::Decl(decl) => {
                            if let Some(init) = &mut decl.init {
                                self.pass2_expr(init, table)?;
                            }
                        }
                        ForInit::Expr(expr) => self.pass2_expr(expr, table)?,
                    }
                }
                self.pass2_expr(cond, table)?;
                for update in updates {
                    self.pass2_expr(update, table)?;
                }
                self.pass2_stmt(body, table)
            }
            Stmt::Block(stmts) => self.pass2_stmts(stmts, table),
            Stmt::Switch { value, cases, .. } => {
                self.pass2_expr(value, table)?;
                for case in cases {
                    if let Some(label) = &mut case.label {
                        self.pass2_expr(label, table)?;
                    }
                    self.pass2_stmts(&mut case.body, table)?;
                }
                Ok(())
            }
        }
    }

    fn pass2_expr(&self, expr: &mut Expr, table: usize) -> Result<()> {
        match &mut expr.kind {
            ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::Null => Ok(()),
            ExprKind::Neg(e) | ExprKind::Not(e) | ExprKind::Cast(e) => self.pass2_expr(e, table),
            ExprKind::Var(lvalue) => match &mut lvalue.member {
                Some(member) => self.pass2_expr(member, table),
                None => Ok(()),
            },
            ExprKind::Assign { target, value } => {
                // The original resolves nothing in the target, so a call
                // in `a[f()] = e` crashes it; resolve it here instead.
                if let Some(member) = &mut target.member {
                    self.pass2_expr(member, table)?;
                }
                self.pass2_expr(value, table)
            }
            ExprKind::Binary { left, right, .. } => {
                self.pass2_expr(left, table)?;
                self.pass2_expr(right, table)
            }
            ExprKind::Call {
                name,
                pos,
                args,
                external,
            } => {
                *external = match self.lookup(table, name) {
                    None => {
                        return Err(self.error(
                            pos,
                            format!("Function '{name}' is referenced but not defined"),
                        ));
                    }
                    Some(Symbol {
                        kind: Kind::Variable,
                        ..
                    }) => {
                        return Err(
                            self.error(pos, format!("Trying to use variable {name} as a function"))
                        );
                    }
                    Some(symbol) => symbol.external,
                };
                for arg in args {
                    self.pass2_expr(arg, table)?;
                }
                Ok(())
            }
            ExprKind::New {
                name,
                pos,
                args,
                external,
            } => {
                *external = match self.lookup(table, name) {
                    None => {
                        return Err(self.error(
                            pos,
                            format!("Object '{name}' is referenced but no constuctor is defined"),
                        ));
                    }
                    Some(Symbol {
                        kind: Kind::Variable,
                        ..
                    }) => {
                        return Err(
                            self.error(pos, format!("Trying to use variable {name} as an object"))
                        );
                    }
                    Some(symbol) => symbol.external,
                };
                for arg in args {
                    self.pass2_expr(arg, table)?;
                }
                Ok(())
            }
            ExprKind::MethodCall {
                object,
                pos,
                method,
                args,
            } => {
                match self.lookup(table, object) {
                    None => {
                        return Err(self.error(
                            pos,
                            format!("Method is called for '{object}' which is not defined"),
                        ));
                    }
                    Some(Symbol {
                        kind: Kind::Function,
                        ..
                    }) => {
                        return Err(self.error(
                            pos,
                            format!("Trying to call method of a non-variable {object}"),
                        ));
                    }
                    Some(_) => {}
                }
                // The original checks the arguments but not the method
                // expression, so a call inside `o[f()]()` would compile
                // with its callee unresolved; check it too.
                self.pass2_expr(method, table)?;
                for arg in args {
                    self.pass2_expr(arg, table)?;
                }
                Ok(())
            }
        }
    }
}
