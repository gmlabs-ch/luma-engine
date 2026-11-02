//! Code generation and the file writer.
//!
//! Written with reference to `compiler/opcode.c` of Wintermute Engine 1.x
//! (MIT License, Copyright (c) 2003-2013 Jan Nedoma and contributors); see
//! `NOTICE`. Specified in `docs/compiler.md`, "Output layout" and "Code
//! generation": the output is byte for byte what the original writes,
//! including the quirks that document names (no short circuit, `JMP 0` for
//! a `break` after a nested loop, tables in reverse source order).

use std::collections::HashMap;

use luma_script::{HEADER_SIZE, MAGIC, Opcode, VERSION};

use crate::ast::*;

/// The line that writes no `DBG_LINE` (the original passes `-1`).
const NO_LINE: u32 = 0;

/// A finished loop or `switch`, for resolving `break` and `continue`.
struct LoopPos {
    nest: u32,
    level: u32,
    start: u32,
    end: u32,
}

/// A `break` (`to_end`) or `continue` jump waiting for its target.
struct LoopReq {
    nest: u32,
    level: u32,
    to_end: bool,
    operand: usize,
}

struct Generator {
    out: Vec<u8>,
    symbols: Vec<String>,
    symbol_index: HashMap<String, u32>,
    /// `CALL` operands to patch, with the callee's name.
    call_requests: Vec<(usize, String)>,
    /// Functions and methods in definition order: position, name, method.
    functions: Vec<(u32, String, bool)>,
    /// Events in definition order: position, name.
    events: Vec<(u32, Vec<u8>)>,
    externals: Vec<External>,
    loops: Vec<LoopPos>,
    loop_requests: Vec<LoopReq>,
    /// Incremented for every loop or `switch` entered, never decremented.
    loop_nest: u32,
    /// The current nesting depth of loops and `switch`es.
    loop_level: u32,
    /// The line of the last `DBG_LINE`; never reset.
    current_line: u32,
    /// The script's final line, for the extra names of a declaration.
    final_line: u32,
}

/// Generates the compiled file for `script`, named `name` in its header.
pub fn generate(script: &Script, name: &str) -> Vec<u8> {
    let mut g = Generator {
        out: Vec::with_capacity(4096),
        symbols: Vec::new(),
        symbol_index: HashMap::new(),
        call_requests: Vec::new(),
        functions: Vec::new(),
        events: Vec::new(),
        externals: Vec::new(),
        loops: Vec::new(),
        loop_requests: Vec::new(),
        loop_nest: 0,
        loop_level: 0,
        current_line: u32::MAX,
        final_line: script.line,
    };
    g.put_u32(MAGIC);
    g.put_u32(VERSION);
    for _ in 0..6 {
        g.put_u32(0);
    }
    g.put_bytes(name.as_bytes());
    debug_assert_eq!(g.out.len(), HEADER_SIZE + name.len() + 1);
    let code_start = g.ip();
    g.patch(8, code_start);

    // Top-level statements first, then functions and methods, then events,
    // each in source order; externals only fill their table.
    for item in &script.items {
        if let TopLevel::Statements(stmts) = item {
            g.stmts(stmts, false);
        }
    }
    for item in &script.items {
        if let TopLevel::Function(f) = item {
            g.function(f);
        }
    }
    for item in &script.items {
        if let TopLevel::Event(e) = item {
            g.event(e);
        }
    }
    for item in &script.items {
        if let TopLevel::External(e) = item {
            g.externals.push(e.clone());
        }
    }
    g.emit(script.line, Opcode::Ret);

    g.resolve_calls();
    g.resolve_loops();
    g.write_tables();
    g.out
}

impl Generator {
    fn ip(&self) -> u32 {
        self.out.len() as u32
    }

    fn put_u32(&mut self, value: u32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn put_f64(&mut self, value: f64) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    fn put_bytes(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
        self.out.push(0);
    }

    fn patch(&mut self, at: usize, value: u32) {
        self.out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// Writes a placeholder operand and returns where to patch it.
    fn placeholder(&mut self) -> usize {
        let at = self.out.len();
        self.put_u32(0);
        at
    }

    /// Writes an opcode, preceded by `DBG_LINE line` when the line is set
    /// and differs from the last one written.
    fn emit(&mut self, line: u32, opcode: Opcode) {
        if line != NO_LINE && line != self.current_line {
            self.current_line = line;
            self.put_u32(Opcode::DbgLine.code());
            self.put_u32(line);
        }
        self.put_u32(opcode.code());
    }

    /// The index of a symbol, assigned on first use.
    fn symbol(&mut self, name: &str) -> u32 {
        if let Some(index) = self.symbol_index.get(name) {
            return *index;
        }
        let index = self.symbols.len() as u32;
        self.symbols.push(name.to_string());
        self.symbol_index.insert(name.to_string(), index);
        index
    }

    fn emit_symbol(&mut self, line: u32, opcode: Opcode, name: &str) {
        self.emit(line, opcode);
        let index = self.symbol(name);
        self.put_u32(index);
    }

    // ---------------------------------------------------------- bodies

    fn function(&mut self, f: &Function) {
        self.emit(f.line, Opcode::Jmp);
        let over = self.placeholder();
        self.functions
            .push((self.ip(), f.name.clone(), f.is_method));
        self.emit(f.line, Opcode::Scope);
        self.emit(f.line, Opcode::CorrectStack);
        self.put_u32(f.params.len() as u32);
        for param in &f.params {
            self.emit_symbol(param.line, Opcode::DefVar, &param.name);
            self.emit_symbol(param.line, Opcode::PopVar, &param.name);
        }
        self.stmts(&f.body, false);
        self.emit(f.line, Opcode::PushNull);
        self.emit(f.line, Opcode::Ret);
        let end = self.ip();
        self.patch(over, end);
    }

    fn event(&mut self, e: &Event) {
        self.emit(e.line, Opcode::Jmp);
        let over = self.placeholder();
        self.events.push((self.ip(), e.name.clone()));
        self.emit(e.line, Opcode::Scope);
        self.stmt(&e.body, true);
        self.emit(e.line, Opcode::RetEvent);
        let end = self.ip();
        self.patch(over, end);
    }

    // ------------------------------------------------------ statements

    fn stmts(&mut self, stmts: &[Stmt], in_event: bool) {
        for stmt in stmts {
            self.stmt(stmt, in_event);
        }
    }

    fn stmt(&mut self, stmt: &Stmt, in_event: bool) {
        match stmt {
            Stmt::Empty => {}
            Stmt::Expr(expr) => self.expr(expr, false),
            Stmt::Decl(decl) => self.decl(decl),
            Stmt::Return { value, line, .. } => {
                if in_event {
                    self.emit(*line, Opcode::RetEvent);
                } else {
                    match value {
                        Some(value) => self.expr(value, true),
                        None => self.emit(*line, Opcode::PushNull),
                    }
                    self.emit(*line, Opcode::Ret);
                }
            }
            Stmt::If {
                cond,
                then,
                otherwise,
                line,
            } => {
                self.expr(cond, true);
                self.emit(*line, Opcode::JmpFalse);
                let to_else = self.placeholder();
                self.stmt(then, in_event);
                match otherwise {
                    None => {
                        let end = self.ip();
                        self.patch(to_else, end);
                    }
                    Some(otherwise) => {
                        self.emit(*line, Opcode::Jmp);
                        let to_end = self.placeholder();
                        let else_start = self.ip();
                        self.patch(to_else, else_start);
                        self.stmt(otherwise, in_event);
                        let end = self.ip();
                        self.patch(to_end, end);
                    }
                }
            }
            Stmt::While { cond, body, line } => {
                self.loop_nest += 1;
                self.loop_level += 1;
                let nest = self.loop_nest;
                let start = self.ip();
                self.expr(cond, true);
                self.emit(NO_LINE, Opcode::JmpFalse);
                let to_end = self.placeholder();
                self.stmt(body, in_event);
                self.emit(*line, Opcode::Jmp);
                self.put_u32(start);
                let end = self.ip();
                self.patch(to_end, end);
                self.loops.push(LoopPos {
                    nest,
                    level: self.loop_level,
                    start,
                    end,
                });
                self.loop_level -= 1;
            }
            Stmt::For {
                inits,
                cond,
                updates,
                body,
                line,
            } => {
                self.loop_nest += 1;
                self.loop_level += 1;
                let nest = self.loop_nest;
                for init in inits {
                    match init {
                        ForInit::Decl(decl) => self.decl(decl),
                        ForInit::Expr(expr) => self.expr(expr, true),
                    }
                }
                let start = self.ip();
                self.expr(cond, true);
                self.emit(NO_LINE, Opcode::JmpFalse);
                let to_end = self.placeholder();
                self.stmt(body, in_event);
                let update = self.ip();
                for expr in updates {
                    self.expr(expr, true);
                }
                self.emit(*line, Opcode::Jmp);
                self.put_u32(start);
                let end = self.ip();
                self.patch(to_end, end);
                self.loops.push(LoopPos {
                    nest,
                    level: self.loop_level,
                    start: update,
                    end,
                });
                self.loop_level -= 1;
            }
            Stmt::Switch { value, cases, line } => {
                self.loop_nest += 1;
                self.loop_level += 1;
                let nest = self.loop_nest;
                let start = self.ip();
                self.expr(value, true);
                self.emit(*line, Opcode::PopReg1);
                // The jump at the end of a case body lands at the start of
                // the next body (fall-through), or at the end of the switch.
                let mut pending_end: Option<usize> = None;
                for case in cases {
                    match &case.label {
                        Some(label) => {
                            self.expr(label, true);
                            self.emit(case.line, Opcode::PushReg1);
                            self.emit(case.line, Opcode::CmpEq);
                            self.emit(case.line, Opcode::JmpFalse);
                            let to_next = self.placeholder();
                            if let Some(at) = pending_end.take() {
                                let here = self.ip();
                                self.patch(at, here);
                            }
                            self.stmts(&case.body, in_event);
                            self.emit(case.line, Opcode::Jmp);
                            pending_end = Some(self.placeholder());
                            let here = self.ip();
                            self.patch(to_next, here);
                        }
                        None => {
                            if let Some(at) = pending_end.take() {
                                let here = self.ip();
                                self.patch(at, here);
                            }
                            self.stmts(&case.body, in_event);
                        }
                    }
                }
                let end = self.ip();
                self.loops.push(LoopPos {
                    nest,
                    level: self.loop_level,
                    start,
                    end,
                });
                if let Some(at) = pending_end {
                    self.patch(at, end);
                }
                self.loop_level -= 1;
            }
            Stmt::Block(stmts) => self.stmts(stmts, in_event),
            Stmt::Break { line, .. } => {
                self.emit(*line, Opcode::Jmp);
                let operand = self.placeholder();
                self.loop_requests.push(LoopReq {
                    nest: self.loop_nest,
                    level: self.loop_level,
                    to_end: true,
                    operand,
                });
            }
            Stmt::Continue { line, .. } => {
                self.emit(*line, Opcode::Jmp);
                let operand = self.placeholder();
                self.loop_requests.push(LoopReq {
                    nest: self.loop_nest,
                    level: self.loop_level,
                    to_end: false,
                    operand,
                });
            }
        }
    }

    /// A declaration: one `DEF_*` per name, each followed by the
    /// initializer (generated again) and a `POP_VAR`. The original splits a
    /// multi-name declaration after parsing, so the second and later names
    /// carry the script's final line.
    fn decl(&mut self, decl: &Decl) {
        let opcode = match decl.kind {
            DeclKind::Var => Opcode::DefVar,
            DeclKind::Global => Opcode::DefGlobVar,
            DeclKind::Const => Opcode::DefConstVar,
        };
        for (i, (name, _)) in decl.names.iter().enumerate() {
            self.emit_symbol(decl.kind_line, opcode, name);
            if let Some(init) = &decl.init {
                self.expr(init, true);
                let line = if i == 0 { decl.line } else { self.final_line };
                self.emit_symbol(line, Opcode::PopVar, name);
            }
        }
    }

    // ----------------------------------------------------- expressions

    /// Generates `expr`; with `value` false (an expression statement) a
    /// call or `new` drops its result.
    fn expr(&mut self, expr: &Expr, value: bool) {
        let line = expr.line;
        match &expr.kind {
            ExprKind::Int(v) => {
                self.emit(line, Opcode::PushInt);
                self.put_u32(*v as u32);
            }
            ExprKind::Bool(v) => {
                self.emit(line, Opcode::PushBool);
                self.put_u32(u32::from(*v));
            }
            ExprKind::Float(v) => {
                self.emit(line, Opcode::PushFloat);
                self.put_f64(*v);
            }
            ExprKind::Str(s) => {
                self.emit(line, Opcode::PushString);
                self.put_bytes(s);
            }
            ExprKind::Null => self.emit(line, Opcode::PushNull),
            ExprKind::Neg(operand) => {
                self.emit(line, Opcode::PushInt);
                self.put_u32(0);
                self.expr(operand, true);
                self.emit(line, Opcode::Sub);
            }
            ExprKind::Not(operand) => {
                self.expr(operand, true);
                self.emit(line, Opcode::Not);
            }
            // The original's generator has no case for a cast: neither the
            // cast nor its operand produces code.
            ExprKind::Cast(_) => {}
            ExprKind::Var(lvalue) => match &lvalue.member {
                None => self.emit_symbol(line, Opcode::PushVar, &lvalue.name),
                Some(member) => {
                    self.emit_symbol(line, Opcode::PushVarRef, &lvalue.name);
                    self.expr(member, true);
                    self.emit(line, Opcode::PushByExp);
                }
            },
            ExprKind::Assign { target, value } => {
                self.expr(value, true);
                match &target.member {
                    None => self.emit_symbol(target.line, Opcode::PopVar, &target.name),
                    Some(member) => {
                        self.emit_symbol(target.line, Opcode::PushVarRef, &target.name);
                        self.expr(member, true);
                        self.emit(target.line, Opcode::PopByExp);
                    }
                }
            }
            ExprKind::Binary { op, left, right } => {
                self.expr(left, true);
                self.expr(right, true);
                let opcode = match op {
                    BinOp::Add => Opcode::Add,
                    BinOp::Sub => Opcode::Sub,
                    BinOp::Mul => Opcode::Mul,
                    BinOp::Div => Opcode::Div,
                    BinOp::Mod => Opcode::Modulo,
                    BinOp::And => Opcode::And,
                    BinOp::Or => Opcode::Or,
                    BinOp::Eq => Opcode::CmpEq,
                    BinOp::Ne => Opcode::CmpNe,
                    BinOp::StrictEq => Opcode::CmpStrictEq,
                    BinOp::StrictNe => Opcode::CmpStrictNe,
                    BinOp::Lt => Opcode::CmpL,
                    BinOp::Gt => Opcode::CmpG,
                    BinOp::Le => Opcode::CmpLe,
                    BinOp::Ge => Opcode::CmpGe,
                };
                self.emit(line, opcode);
            }
            ExprKind::Call {
                name,
                args,
                external,
                ..
            } => {
                self.arguments(args);
                self.call(line, name, *external, args.len());
                if !value {
                    self.emit(line, Opcode::PopEmpty);
                }
            }
            ExprKind::New {
                name,
                args,
                external,
                ..
            } => {
                self.emit(line, Opcode::CreateObject);
                self.emit(line, Opcode::PushThisFromStack);
                self.arguments(args);
                self.call(line, name, *external, args.len());
                self.emit(line, Opcode::PopThis);
                // The constructor's result is never used...
                self.emit(line, Opcode::PopEmpty);
                // ...and the object itself only when the expression has a
                // value.
                if !value {
                    self.emit(line, Opcode::PopEmpty);
                }
            }
            ExprKind::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                self.emit_symbol(line, Opcode::PushThis, object);
                self.arguments(args);
                self.emit(line, Opcode::PushInt);
                self.put_u32(args.len() as u32);
                self.emit_symbol(line, Opcode::PushVar, object);
                self.expr(method, true);
                self.emit(line, Opcode::CallByExp);
                if !value {
                    self.emit(line, Opcode::PopEmpty);
                }
                self.emit(line, Opcode::PopThis);
            }
        }
    }

    /// Pushes the arguments right to left.
    fn arguments(&mut self, args: &[Expr]) {
        for arg in args.iter().rev() {
            self.expr(arg, true);
        }
    }

    /// `PUSH_INT argc` then `EXTERNAL_CALL name`, or `CALL` with an operand
    /// patched once every function's position is known.
    fn call(&mut self, line: u32, name: &str, external: bool, argc: usize) {
        self.emit(line, Opcode::PushInt);
        self.put_u32(argc as u32);
        if external {
            self.emit_symbol(line, Opcode::ExternalCall, name);
        } else {
            self.emit(line, Opcode::Call);
            let operand = self.placeholder();
            self.call_requests.push((operand, name.to_string()));
        }
    }

    fn resolve_calls(&mut self) {
        for (operand, name) in std::mem::take(&mut self.call_requests) {
            // The most recently defined function with that name wins.
            if let Some((position, _, _)) = self.functions.iter().rev().find(|(_, n, _)| n == &name)
            {
                let position = *position;
                self.patch(operand, position);
            }
        }
    }

    fn resolve_loops(&mut self) {
        for request in &self.loop_requests {
            // Pairs are unique, so order does not matter; a jump whose pair
            // matches no loop keeps its zero operand, as in the original.
            if let Some(found) = self
                .loops
                .iter()
                .find(|l| l.nest == request.nest && l.level == request.level)
            {
                let target = if request.to_end {
                    found.end
                } else {
                    found.start
                };
                self.out[request.operand..request.operand + 4]
                    .copy_from_slice(&target.to_le_bytes());
            }
        }
    }

    /// The five tables, each prepended to a list by the original and so
    /// written in reverse definition order (symbols in index order).
    fn write_tables(&mut self) {
        let functions_at = self.ip();
        self.patch(12, functions_at);
        let functions: Vec<(u32, String)> = self
            .functions
            .iter()
            .rev()
            .filter(|(_, _, method)| !method)
            .map(|(pos, name, _)| (*pos, name.clone()))
            .collect();
        self.put_u32(functions.len() as u32);
        for (position, name) in &functions {
            self.put_u32(*position);
            self.put_bytes(name.as_bytes());
        }

        let symbols_at = self.ip();
        self.patch(16, symbols_at);
        let symbols = std::mem::take(&mut self.symbols);
        self.put_u32(symbols.len() as u32);
        for (index, name) in symbols.iter().enumerate() {
            self.put_u32(index as u32);
            self.put_bytes(name.as_bytes());
        }

        let events_at = self.ip();
        self.patch(20, events_at);
        let events = std::mem::take(&mut self.events);
        self.put_u32(events.len() as u32);
        for (position, name) in events.iter().rev() {
            self.put_u32(*position);
            self.put_bytes(name);
        }

        let externals_at = self.ip();
        self.patch(24, externals_at);
        let externals = std::mem::take(&mut self.externals);
        self.put_u32(externals.len() as u32);
        for e in externals.iter().rev() {
            self.put_bytes(&e.dll);
            self.put_bytes(e.name.as_bytes());
            self.put_u32(match e.call {
                CallConv::StdCall => 0,
                CallConv::CDecl => 1,
                CallConv::ThisCall => 2,
            });
            self.put_u32(e.returns as u32);
            self.put_u32(e.params.len() as u32);
            for param in &e.params {
                self.put_u32(*param as u32);
            }
        }

        let methods_at = self.ip();
        self.patch(28, methods_at);
        let methods: Vec<(u32, String)> = self
            .functions
            .iter()
            .rev()
            .filter(|(_, _, method)| *method)
            .map(|(pos, name, _)| (*pos, name.clone()))
            .collect();
        self.put_u32(methods.len() as u32);
        for (position, name) in &methods {
            self.put_u32(*position);
            self.put_bytes(name.as_bytes());
        }
    }
}
