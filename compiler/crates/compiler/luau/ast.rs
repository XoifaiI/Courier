//! The Luau AST the generator builds.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Name<'a> {
    Fixed(&'a str),
    Symbol(u32),
    Temp(char, u32),
}

pub type TypeText<'a> = Cow<'a, str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Lt,
    Le,
    Gt,
    Eq,
    Ne,
    Add,
    Sub,
    Mul,
    FloorDiv,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Or => "or",
            BinOp::And => "and",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Eq => "==",
            BinOp::Ne => "~=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::FloorDiv => "//",
        }
    }

    pub fn precedence(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Eq | BinOp::Ne => 3,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::FloorDiv => 6,
        }
    }

    pub fn associative(self) -> bool {
        matches!(self, BinOp::Add | BinOp::Mul | BinOp::Or | BinOp::And)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr<'a> {
    Name(Name<'a>),
    Field(ExprId, &'a str),
    Index(ExprId, ExprId),
    Call(ExprId, Range<u32>),
    Method(ExprId, &'a str, Range<u32>),
    Number(f64),
    Hex(u64),
    Str(Cow<'a, str>),
    Nil,
    Bool(bool),
    Vararg,
    Binary(BinOp, ExprId, ExprId),
    Not(ExprId),
    Length(ExprId),
    If(ExprId, ExprId, ExprId),
    Table(Range<u32>, bool),
    Cast(ExprId, TypeText<'a>),
    Function(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    pub key: Option<Cow<'a, str>>,
    pub value: ExprId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter<'a> {
    pub name: Name<'a>,
    pub ty: TypeText<'a>,
}

impl<'a> Parameter<'a> {
    pub fn new(name: Name<'a>, ty: impl Into<TypeText<'a>>) -> Parameter<'a> {
        Parameter { name, ty: ty.into() }
    }

    pub fn vararg() -> Parameter<'a> {
        Parameter { name: Name::Fixed("..."), ty: Cow::Borrowed("any") }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function<'a> {
    pub parameters: Vec<Parameter<'a>>,
    pub returns: TypeText<'a>,
    pub body: Block<'a>,
}

pub type Block<'a> = Vec<Stmt<'a>>;

#[derive(Debug, Clone, PartialEq)]
pub enum TypeBody<'a> {
    Inline(TypeText<'a>),
    Fields(Vec<(Cow<'a, str>, TypeText<'a>)>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt<'a> {
    Local(Name<'a>, Option<TypeText<'a>>, Option<ExprId>),
    Locals(Vec<Name<'a>>, Option<TypeText<'a>>, ExprId),
    Declare(Vec<(Name<'a>, TypeText<'a>)>),
    Assign(Vec<ExprId>, Vec<ExprId>),
    Compound(ExprId, BinOp, ExprId),
    Call(ExprId),
    CallLines(ExprId),
    If(Vec<(ExprId, Block<'a>)>, Option<Block<'a>>),
    While(ExprId, Block<'a>),
    For(Name<'a>, ExprId, ExprId, Block<'a>),
    ForIn(Vec<Name<'a>>, Vec<ExprId>, Block<'a>),
    Do(Block<'a>),
    Return(Vec<ExprId>),
    Break,
    Function { name: Name<'a>, parameters: Vec<Parameter<'a>>, returns: TypeText<'a>, body: Block<'a> },
    TypeAlias { name: Cow<'a, str>, generics: &'static str, body: TypeBody<'a> },
    Verbatim(Cow<'a, str>),
    Blank,
    Slot(u32),
}

#[derive(Debug, Default)]
pub struct Ast<'a> {
    exprs: RefCell<Vec<Expr<'a>>>,
    lists: RefCell<Vec<ExprId>>,
    entries: RefCell<Vec<Entry<'a>>>,
    functions: RefCell<Vec<Function<'a>>>,
    symbols: RefCell<Vec<String>>,
    symbol_ids: RefCell<HashMap<String, u32>>,
}

#[derive(Debug, Default)]
pub struct Arena<'a> {
    pub exprs: Vec<Expr<'a>>,
    pub lists: Vec<ExprId>,
    pub entries: Vec<Entry<'a>>,
    pub functions: Vec<Function<'a>>,
    pub symbols: Vec<String>,
}

impl<'a> Arena<'a> {
    pub fn expr(&self, id: ExprId) -> &Expr<'a> {
        &self.exprs[id.0 as usize]
    }

    pub fn list(&self, range: &Range<u32>) -> &[ExprId] {
        &self.lists[range.start as usize..range.end as usize]
    }

    pub fn entries(&self, range: &Range<u32>) -> &[Entry<'a>] {
        &self.entries[range.start as usize..range.end as usize]
    }
}

impl<'a> Ast<'a> {
    pub fn finish(self) -> Arena<'a> {
        Arena {
            exprs: self.exprs.into_inner(),
            lists: self.lists.into_inner(),
            entries: self.entries.into_inner(),
            functions: self.functions.into_inner(),
            symbols: self.symbols.into_inner(),
        }
    }

    pub fn symbol(&self, text: String) -> Name<'a> {
        if let Some(id) = self.symbol_ids.borrow().get(&text) {
            return Name::Symbol(*id);
        }
        let mut symbols = self.symbols.borrow_mut();
        let id = symbols.len() as u32;
        self.symbol_ids.borrow_mut().insert(text.clone(), id);
        symbols.push(text);
        Name::Symbol(id)
    }

    pub fn get(&self, id: ExprId) -> Expr<'a> {
        self.exprs.borrow()[id.0 as usize].clone()
    }

    pub fn replace(&self, id: ExprId, expr: Expr<'a>) {
        self.exprs.borrow_mut()[id.0 as usize] = expr;
    }

    pub fn push(&self, expr: Expr<'a>) -> ExprId {
        let mut exprs = self.exprs.borrow_mut();
        exprs.push(expr);
        ExprId(exprs.len() as u32 - 1)
    }

    fn list(&self, items: impl IntoIterator<Item = ExprId>) -> Range<u32> {
        let mut lists = self.lists.borrow_mut();
        let start = lists.len() as u32;
        lists.extend(items);
        start..lists.len() as u32
    }

    pub fn name(&self, name: Name<'a>) -> ExprId {
        self.push(Expr::Name(name))
    }

    pub fn fixed(&self, name: &'a str) -> ExprId {
        self.push(Expr::Name(Name::Fixed(name)))
    }

    pub fn field(&self, object: ExprId, field: &'a str) -> ExprId {
        self.push(Expr::Field(object, field))
    }

    pub fn index(&self, object: ExprId, key: ExprId) -> ExprId {
        self.push(Expr::Index(object, key))
    }

    pub fn call(&self, callee: ExprId, arguments: impl IntoIterator<Item = ExprId>) -> ExprId {
        let arguments = self.list(arguments);
        self.push(Expr::Call(callee, arguments))
    }

    pub fn invoke(&self, global: &'a str, arguments: impl IntoIterator<Item = ExprId>) -> ExprId {
        let callee = self.fixed(global);
        self.call(callee, arguments)
    }

    pub fn method(&self, object: ExprId, method: &'a str, arguments: impl IntoIterator<Item = ExprId>) -> ExprId {
        let arguments = self.list(arguments);
        self.push(Expr::Method(object, method, arguments))
    }

    pub fn number(&self, value: impl Into<f64>) -> ExprId {
        self.push(Expr::Number(value.into()))
    }

    pub fn hex(&self, value: u64) -> ExprId {
        self.push(Expr::Hex(value))
    }

    pub fn string(&self, text: impl Into<Cow<'a, str>>) -> ExprId {
        self.push(Expr::Str(text.into()))
    }

    pub fn nil(&self) -> ExprId {
        self.push(Expr::Nil)
    }

    pub fn boolean(&self, value: bool) -> ExprId {
        self.push(Expr::Bool(value))
    }

    pub fn vararg(&self) -> ExprId {
        self.push(Expr::Vararg)
    }

    pub fn binary(&self, op: BinOp, left: ExprId, right: ExprId) -> ExprId {
        self.push(Expr::Binary(op, left, right))
    }

    pub fn add(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Add, left, right)
    }

    pub fn sub(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Sub, left, right)
    }

    pub fn mul(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Mul, left, right)
    }

    pub fn floor_div(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::FloorDiv, left, right)
    }

    pub fn gt(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Gt, left, right)
    }

    pub fn lt(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Lt, left, right)
    }

    pub fn eq(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Eq, left, right)
    }

    pub fn ne(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Ne, left, right)
    }

    pub fn or(&self, left: ExprId, right: ExprId) -> ExprId {
        self.binary(BinOp::Or, left, right)
    }

    pub fn not(&self, inner: ExprId) -> ExprId {
        self.push(Expr::Not(inner))
    }

    pub fn length(&self, inner: ExprId) -> ExprId {
        self.push(Expr::Length(inner))
    }

    pub fn if_else(&self, condition: ExprId, then: ExprId, otherwise: ExprId) -> ExprId {
        self.push(Expr::If(condition, then, otherwise))
    }

    pub fn cast(&self, inner: ExprId, ty: impl Into<TypeText<'a>>) -> ExprId {
        self.push(Expr::Cast(inner, ty.into()))
    }

    pub fn table(&self, entries: impl IntoIterator<Item = Entry<'a>>, multiline: bool) -> ExprId {
        let mut table = self.entries.borrow_mut();
        let start = table.len() as u32;
        table.extend(entries);
        let range = start..table.len() as u32;
        drop(table);
        self.push(Expr::Table(range, multiline))
    }

    pub fn function(
        &self,
        parameters: Vec<Parameter<'a>>,
        returns: impl Into<TypeText<'a>>,
        body: Block<'a>,
    ) -> ExprId {
        let mut functions = self.functions.borrow_mut();
        functions.push(Function { parameters, returns: returns.into(), body });
        let index = functions.len() as u32 - 1;
        drop(functions);
        self.push(Expr::Function(index))
    }

    pub fn minus(&self, base: ExprId, offset: f64) -> ExprId {
        if offset == 0.0 {
            base
        } else if offset < 0.0 {
            let offset = self.number(-offset);
            self.add(base, offset)
        } else {
            let offset = self.number(offset);
            self.sub(base, offset)
        }
    }

    pub fn sum(&self, terms: impl IntoIterator<Item = ExprId>) -> ExprId {
        terms.into_iter().reduce(|total, term| self.add(total, term)).unwrap_or_else(|| self.number(0))
    }
}

pub fn local<'a>(name: Name<'a>, value: ExprId) -> Stmt<'a> {
    Stmt::Local(name, None, Some(value))
}

pub fn assign<'a>(target: ExprId, value: ExprId) -> Stmt<'a> {
    Stmt::Assign(vec![target], vec![value])
}

pub fn when<'a>(condition: ExprId, body: Block<'a>) -> Stmt<'a> {
    Stmt::If(vec![(condition, body)], None)
}

pub fn increase<'a>(target: ExprId, by: ExprId) -> Stmt<'a> {
    Stmt::Compound(target, BinOp::Add, by)
}

pub fn decrease<'a>(target: ExprId, by: ExprId) -> Stmt<'a> {
    Stmt::Compound(target, BinOp::Sub, by)
}
