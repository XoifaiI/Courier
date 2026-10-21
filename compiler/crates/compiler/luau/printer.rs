//! Prints the AST as Luau.

use std::fmt::Write as _;

use super::ast::{Arena, Entry, Expr, ExprId, Name, Parameter, Stmt, TypeBody};

const UNARY: u8 = 7;
const ATOM: u8 = 10;

pub fn print(arena: &Arena, block: &[Stmt]) -> String {
    let mut printer = Printer { arena, out: String::with_capacity(block.len() * 48) };
    printer.block(block, 0);
    printer.out
}

pub fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{}", value as i64) } else { format!("{value:?}") }
}

struct Printer<'p, 'a> {
    arena: &'p Arena<'a>,
    out: String,
}

impl Printer<'_, '_> {
    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.out.push('\t');
        }
    }

    fn name(&mut self, name: Name) {
        match name {
            Name::Fixed(text) => self.out.push_str(text),
            Name::Symbol(id) => self.out.push_str(&self.arena.symbols[id as usize]),
            Name::Temp(letter, number) => {
                let _ = write!(self.out, "{letter}{number}");
            }
        }
    }

    fn names(&mut self, names: &[Name]) {
        for (position, name) in names.iter().enumerate() {
            if position > 0 {
                self.out.push_str(", ");
            }
            self.name(*name);
        }
    }

    fn parameters(&mut self, parameters: &[Parameter]) {
        self.out.push('(');
        for (position, parameter) in parameters.iter().enumerate() {
            if position > 0 {
                self.out.push_str(", ");
            }
            self.name(parameter.name);
            let _ = write!(self.out, ": {}", parameter.ty);
        }
        self.out.push(')');
    }

    fn list(&mut self, items: &[ExprId], depth: usize) {
        for (position, item) in items.iter().enumerate() {
            if position > 0 {
                self.out.push_str(", ");
            }
            self.expr(*item, depth);
        }
    }

    fn block(&mut self, block: &[Stmt], depth: usize) {
        for statement in block {
            self.statement(statement, depth);
        }
    }

    fn body(&mut self, block: &[Stmt], depth: usize) {
        self.block(block, depth + 1);
        self.indent(depth);
        self.out.push_str("end\n");
    }

    fn statement(&mut self, statement: &Stmt, depth: usize) {
        if !matches!(statement, Stmt::Verbatim(_) | Stmt::Blank | Stmt::Slot(_)) {
            self.indent(depth);
        }
        match statement {
            Stmt::Local(name, ty, value) => {
                self.out.push_str("local ");
                self.name(*name);
                if let Some(ty) = ty {
                    let _ = write!(self.out, ": {ty}");
                }
                if let Some(value) = value {
                    self.out.push_str(" = ");
                    self.expr(*value, depth);
                }
                self.out.push('\n');
            }
            Stmt::Locals(names, ty, value) => {
                self.out.push_str("local ");
                match ty {
                    Some(ty) => {
                        for (position, name) in names.iter().enumerate() {
                            if position > 0 {
                                self.out.push_str(", ");
                            }
                            self.name(*name);
                            let _ = write!(self.out, ": {ty}");
                        }
                    }
                    None => self.names(names),
                }
                self.out.push_str(" = ");
                self.expr(*value, depth);
                self.out.push('\n');
            }
            Stmt::Declare(names) => {
                self.out.push_str("local ");
                for (position, (name, ty)) in names.iter().enumerate() {
                    if position > 0 {
                        self.out.push_str(", ");
                    }
                    self.name(*name);
                    let _ = write!(self.out, ": {ty}");
                }
                self.out.push('\n');
            }
            Stmt::Assign(targets, values) => {
                self.list(targets, depth);
                self.out.push_str(" = ");
                self.list(values, depth);
                self.out.push('\n');
            }
            Stmt::Compound(target, op, value) => {
                self.expr(*target, depth);
                let _ = write!(self.out, " {}= ", op.symbol());
                self.expr(*value, depth);
                self.out.push('\n');
            }
            Stmt::Call(call) => {
                self.expr(*call, depth);
                self.out.push('\n');
            }
            Stmt::CallLines(call) => self.call_lines(*call, depth),
            Stmt::If(branches, otherwise) => {
                for (position, (condition, body)) in branches.iter().enumerate() {
                    if position > 0 {
                        self.indent(depth);
                    }
                    self.out.push_str(if position == 0 { "if " } else { "elseif " });
                    self.expr(*condition, depth);
                    self.out.push_str(" then\n");
                    self.block(body, depth + 1);
                }
                if let Some(body) = otherwise {
                    self.indent(depth);
                    self.out.push_str("else\n");
                    self.block(body, depth + 1);
                }
                self.indent(depth);
                self.out.push_str("end\n");
            }
            Stmt::While(condition, body) => {
                self.out.push_str("while ");
                self.expr(*condition, depth);
                self.out.push_str(" do\n");
                self.body(body, depth);
            }
            Stmt::For(variable, from, to, body) => {
                self.out.push_str("for ");
                self.name(*variable);
                self.out.push_str(" = ");
                self.expr(*from, depth);
                self.out.push_str(", ");
                self.expr(*to, depth);
                self.out.push_str(" do\n");
                self.body(body, depth);
            }
            Stmt::ForIn(names, iterable, body) => {
                self.out.push_str("for ");
                self.names(names);
                self.out.push_str(" in ");
                self.list(iterable, depth);
                self.out.push_str(" do\n");
                self.body(body, depth);
            }
            Stmt::Do(body) => {
                self.out.push_str("do\n");
                self.body(body, depth);
            }
            Stmt::Return(values) => {
                self.out.push_str("return");
                if !values.is_empty() {
                    self.out.push(' ');
                    self.list(values, depth);
                }
                self.out.push('\n');
            }
            Stmt::Break => self.out.push_str("break\n"),
            Stmt::Function { name, parameters, returns, body } => {
                self.out.push_str("local function ");
                self.name(*name);
                self.parameters(parameters);
                let _ = writeln!(self.out, ": {returns}");
                self.body(body, depth);
            }
            Stmt::TypeAlias { name, generics, body } => match body {
                TypeBody::Inline(ty) => {
                    let _ = writeln!(self.out, "export type {name}{generics} = {ty}");
                }
                TypeBody::Fields(fields) => {
                    let _ = writeln!(self.out, "export type {name}{generics} = {{");
                    for (field, ty) in fields {
                        self.indent(depth + 1);
                        let _ = writeln!(self.out, "{field}: {ty},");
                    }
                    self.indent(depth);
                    self.out.push_str("}\n");
                }
            },
            Stmt::Verbatim(text) => self.out.push_str(text),
            Stmt::Blank => self.out.push('\n'),
            Stmt::Slot(_) => {}
        }
    }

    fn call_lines(&mut self, call: ExprId, depth: usize) {
        let arena = self.arena;
        let arguments = match arena.expr(call) {
            Expr::Call(callee, arguments) => {
                self.operand(*callee, ATOM, depth);
                arena.list(arguments)
            }
            Expr::Method(object, method, arguments) => {
                self.operand(*object, ATOM, depth);
                let _ = write!(self.out, ":{method}");
                arena.list(arguments)
            }
            _ => {
                self.expr(call, depth);
                self.out.push('\n');
                return;
            }
        };
        if arguments.is_empty() {
            self.out.push_str("()\n");
            return;
        }
        self.out.push_str("(\n");
        for (position, argument) in arguments.iter().enumerate() {
            self.indent(depth + 1);
            self.expr(*argument, depth + 1);
            self.out.push_str(if position + 1 < arguments.len() { ",\n" } else { "\n" });
        }
        self.indent(depth);
        self.out.push_str(")\n");
    }

    fn precedence(&self, id: ExprId) -> u8 {
        match self.arena.expr(id) {
            Expr::Binary(op, ..) => op.precedence(),
            Expr::Not(_) | Expr::Length(_) => UNARY,
            Expr::If(..) | Expr::Cast(..) | Expr::Function(_) => 0,
            Expr::Number(value) if *value < 0.0 => UNARY,
            _ => ATOM,
        }
    }

    fn operand(&mut self, id: ExprId, needed: u8, depth: usize) {
        if self.precedence(id) < needed {
            self.out.push('(');
            self.expr(id, depth);
            self.out.push(')');
        } else {
            self.expr(id, depth);
        }
    }

    fn expr(&mut self, id: ExprId, depth: usize) {
        let arena = self.arena;
        match arena.expr(id) {
            Expr::Name(name) => self.name(*name),
            Expr::Field(object, field) => {
                self.operand(*object, ATOM, depth);
                self.out.push('.');
                self.out.push_str(field);
            }
            Expr::Index(object, key) => {
                self.operand(*object, ATOM, depth);
                self.out.push('[');
                self.expr(*key, depth);
                self.out.push(']');
            }
            Expr::Call(callee, arguments) => {
                self.operand(*callee, ATOM, depth);
                self.out.push('(');
                self.list(arena.list(arguments), depth);
                self.out.push(')');
            }
            Expr::Method(object, method, arguments) => {
                self.operand(*object, ATOM, depth);
                let _ = write!(self.out, ":{method}(");
                self.list(arena.list(arguments), depth);
                self.out.push(')');
            }
            Expr::Number(value) => self.out.push_str(&number(*value)),
            Expr::Hex(value) => {
                let _ = write!(self.out, "0x{value:X}");
            }
            Expr::Str(text) => {
                self.out.push('"');
                for character in text.chars() {
                    if matches!(character, '"' | '\\') {
                        self.out.push('\\');
                    }
                    self.out.push(character);
                }
                self.out.push('"');
            }
            Expr::Nil => self.out.push_str("nil"),
            Expr::Bool(value) => self.out.push_str(if *value { "true" } else { "false" }),
            Expr::Vararg => self.out.push_str("..."),
            Expr::Binary(op, left, right) => {
                let own = op.precedence();
                let right_needed = if op.associative() { own } else { own + 1 };
                self.operand(*left, own, depth);
                let _ = write!(self.out, " {} ", op.symbol());
                self.operand(*right, right_needed, depth);
            }
            Expr::Not(inner) => {
                self.out.push_str("not ");
                self.operand(*inner, UNARY, depth);
            }
            Expr::Length(inner) => {
                self.out.push('#');
                self.operand(*inner, UNARY, depth);
            }
            Expr::If(condition, then, otherwise) => {
                self.out.push_str("if ");
                self.expr(*condition, depth);
                self.out.push_str(" then ");
                self.expr(*then, depth);
                let mut rest = *otherwise;
                while let Expr::If(condition, then, otherwise) = arena.expr(rest) {
                    self.out.push_str(" elseif ");
                    self.expr(*condition, depth);
                    self.out.push_str(" then ");
                    self.expr(*then, depth);
                    rest = *otherwise;
                }
                self.out.push_str(" else ");
                self.expr(rest, depth);
            }
            Expr::Table(entries, multiline) => self.table(arena.entries(entries), *multiline, depth),
            Expr::Cast(inner, ty) => {
                self.operand(*inner, 1, depth);
                let _ = write!(self.out, " :: {ty}");
            }
            Expr::Function(index) => {
                let function = &arena.functions[*index as usize];
                self.out.push_str("function");
                self.parameters(&function.parameters);
                if !function.returns.is_empty() {
                    let _ = write!(self.out, ": {}", function.returns);
                }
                self.out.push('\n');
                self.block(&function.body, depth + 1);
                self.indent(depth);
                self.out.push_str("end");
            }
        }
    }

    fn table(&mut self, entries: &[Entry], multiline: bool, depth: usize) {
        if entries.is_empty() {
            self.out.push_str("{}");
            return;
        }
        self.out.push_str(if multiline { "{\n" } else { "{ " });
        for (position, entry) in entries.iter().enumerate() {
            if multiline {
                self.indent(depth + 1);
            } else if position > 0 {
                self.out.push_str(", ");
            }
            if let Some(key) = &entry.key {
                let _ = write!(self.out, "{key} = ");
            }
            self.expr(entry.value, depth + 1);
            if multiline {
                self.out.push_str(",\n");
            }
        }
        if multiline {
            self.indent(depth);
            self.out.push('}');
        } else {
            self.out.push_str(" }");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luau::ast::{Ast, Entry, Parameter, local};

    fn printed(build: impl FnOnce(&Ast<'static>) -> Vec<Stmt<'static>>) -> String {
        let ast = Ast::default();
        let block = build(&ast);
        print(&ast.finish(), &block)
    }

    #[test]
    fn parentheses_follow_precedence() {
        let text = printed(|b| {
            let scaled = b.mul(b.add(b.fixed("A"), b.number(1)), b.number(8));
            let chain = b.sum([b.fixed("A"), b.mul(b.fixed("B"), b.number(4)), b.fixed("C")]);
            let nested = b.sub(b.fixed("A"), b.sub(b.fixed("B"), b.fixed("C")));
            vec![local(Name::Fixed("X"), scaled), local(Name::Fixed("Y"), chain), local(Name::Fixed("Z"), nested)]
        });
        assert_eq!(text, "local X = (A + 1) * 8\nlocal Y = A + B * 4 + C\nlocal Z = A - (B - C)\n");
    }

    #[test]
    fn negative_offsets_read_as_additions() {
        let text = printed(|b| {
            vec![
                local(Name::Temp('V', 1), b.minus(b.fixed("A"), -100.0)),
                local(Name::Temp('V', 2), b.minus(b.fixed("A"), 2.5)),
            ]
        });
        assert_eq!(text, "local V1 = A + 100\nlocal V2 = A - 2.5\n");
    }

    #[test]
    fn closures_indent_their_bodies() {
        let text = printed(|b| {
            let closure = |b: &Ast<'static>| {
                let parameters = vec![Parameter::new(Name::Fixed("A"), "number")];
                b.function(parameters, "", vec![Stmt::Return(vec![b.fixed("A")])])
            };
            let table = b.table([Entry { key: Some("F".into()), value: closure(b) }], true);
            let called = b.call(closure(b), []);
            vec![local(Name::Fixed("T"), table), local(Name::Fixed("X"), called)]
        });
        assert_eq!(
            text,
            "local T = {\n\tF = function(A: number)\n\t\treturn A\n\tend,\n}\nlocal X = (function(A: number)\n\treturn A\nend)()\n"
        );
    }

    #[test]
    fn methods_and_fields_print_on_their_object() {
        let text = printed(|b| {
            let handle = b.method(b.field(b.fixed("EVENTS"), "Hit"), "Handle", [b.fixed("Handler"), b.nil()]);
            vec![Stmt::Call(handle)]
        });
        assert_eq!(text, "EVENTS.Hit:Handle(Handler, nil)\n");
    }
}
