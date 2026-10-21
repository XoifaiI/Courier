//! Shared state while one module is being emitted.

use std::cell::{Cell, RefCell};

use crate::layout::{Layout, PathId};
use crate::luau::{Ast, Block, Expr, ExprId, Name, Stmt, local};
use crate::schema::Schema;

#[derive(Debug, Clone, Copy)]
pub enum Base<'a> {
    Parameters,
    Local(Name<'a>),
}

#[derive(Debug, Clone, Copy)]
struct Read {
    path: PathId,
    expr: ExprId,
    scope: u32,
    alive: bool,
}

#[derive(Debug, Clone, Copy)]
struct Scope {
    parent: Option<u32>,
    base: PathId,
}

pub struct Emitter<'e, 'a> {
    pub schema: &'a Schema<'a>,
    pub layout: &'e Layout<'a>,
    pub b: &'e Ast<'a>,
    temps: Cell<u32>,
    bases: RefCell<Vec<(PathId, Base<'a>)>>,
    reads: RefCell<Vec<Read>>,
    scopes: RefCell<Vec<Scope>>,
    current: Cell<Option<u32>>,
}

impl<'e, 'a> Emitter<'e, 'a> {
    pub fn new(schema: &'a Schema<'a>, layout: &'e Layout<'a>, b: &'e Ast<'a>) -> Emitter<'e, 'a> {
        Emitter {
            schema,
            layout,
            b,
            temps: Cell::new(0),
            bases: RefCell::default(),
            reads: RefCell::default(),
            scopes: RefCell::default(),
            current: Cell::new(None),
        }
    }

    pub fn temp(&self, letter: char) -> Name<'a> {
        let next = self.temps.get() + 1;
        self.temps.set(next);
        Name::Temp(letter, next)
    }

    pub fn fixed(&self, name: &'a str) -> ExprId {
        self.b.fixed(name)
    }

    pub fn begin_function(&self) {
        self.temps.set(0);
        self.bases.borrow_mut().clear();
        self.reads.borrow_mut().clear();
        self.scopes.borrow_mut().clear();
        self.current.set(None);
    }

    pub fn bind(&self, path: PathId, base: Base<'a>) {
        self.bases.borrow_mut().push((path, base));
    }

    fn base(&self, path: PathId) -> Option<Base<'a>> {
        self.bases.borrow().iter().rev().find(|(bound, _)| *bound == path).map(|(_, base)| *base)
    }

    pub fn read(&self, path: PathId) -> ExprId {
        if let Some(Base::Local(name)) = self.base(path) {
            return self.b.name(name);
        }
        let step = self.layout.path(path);
        let (Some(parent), Some(field)) = (step.parent, step.field) else {
            panic!("path {path:?} is a node's value with nothing bound to it");
        };
        if let Some(Base::Parameters) = self.base(parent) {
            return self.b.fixed(field);
        }
        let object = self.read(parent);
        let expr = self.b.field(object, field);
        if let Some(scope) = self.current.get() {
            self.reads.borrow_mut().push(Read { path, expr, scope, alive: true });
        }
        expr
    }

    pub fn scope(&self, base: PathId, emit: impl FnOnce() -> Block<'a>) -> Block<'a> {
        let slot = self.scopes.borrow().len() as u32;
        let parent = self.current.get();
        self.scopes.borrow_mut().push(Scope { parent, base });
        self.current.set(Some(slot));
        let mut block = vec![Stmt::Slot(slot)];
        block.extend(emit());
        self.current.set(parent);
        block
    }

    fn within(&self, mut path: PathId, ancestor: PathId) -> bool {
        loop {
            if path == ancestor {
                return true;
            }
            match self.layout.path(path).parent {
                Some(parent) => path = parent,
                None => return false,
            }
        }
    }

    fn depth(&self, mut path: PathId) -> usize {
        let mut depth = 0;
        while let Some(parent) = self.layout.path(path).parent {
            path = parent;
            depth += 1;
        }
        depth
    }

    fn owner(&self, read: &Read) -> u32 {
        let scopes = self.scopes.borrow();
        let mut owner = read.scope;
        let mut passed = read.scope;
        while let Some(parent) = scopes[passed as usize].parent {
            let inner_base = scopes[passed as usize].base;
            let blocks = inner_base != read.path && self.within(read.path, inner_base);
            if blocks || !self.within(read.path, scopes[parent as usize].base) {
                break;
            }
            owner = parent;
            passed = parent;
        }
        owner
    }

    pub fn hoist(&self, body: &mut Block<'a>) {
        let scope_count = self.scopes.borrow().len();
        let mut filled: Vec<Block<'a>> = vec![Vec::new(); scope_count];
        let owners: Vec<u32> = self.reads.borrow().iter().map(|read| self.owner(read)).collect();
        for scope in 0..scope_count as u32 {
            let mut paths: Vec<PathId> = Vec::new();
            for (read, owner) in self.reads.borrow().iter().zip(&owners) {
                if *owner == scope && read.alive && !paths.contains(&read.path) {
                    paths.push(read.path);
                }
            }
            paths.sort_by_key(|path| self.depth(*path));
            for path in paths {
                let uses: Vec<usize> = (0..owners.len())
                    .filter(|index| {
                        let read = self.reads.borrow()[*index];
                        owners[*index] == scope && read.alive && read.path == path
                    })
                    .collect();
                if uses.len() < 2 {
                    continue;
                }
                let name = self.temp('R');
                let first = self.reads.borrow()[uses[0]].expr;
                let definition = self.b.push(self.b.get(first));
                for index in uses {
                    let mut reads = self.reads.borrow_mut();
                    reads[index].alive = false;
                    self.b.replace(reads[index].expr, Expr::Name(name));
                }
                filled[scope as usize].push(local(name, definition));
            }
        }
        fill_slots(body, &mut filled);
    }
}

fn fill_slots<'a>(block: &mut Block<'a>, filled: &mut [Block<'a>]) {
    let mut index = 0;
    while index < block.len() {
        if let Stmt::Slot(slot) = block[index] {
            let statements = std::mem::take(&mut filled[slot as usize]);
            let count = statements.len();
            block.splice(index..=index, statements);
            index += count;
            continue;
        }
        match &mut block[index] {
            Stmt::If(branches, otherwise) => {
                branches.iter_mut().for_each(|(_, body)| fill_slots(body, filled));
                if let Some(body) = otherwise {
                    fill_slots(body, filled);
                }
            }
            Stmt::While(_, body) | Stmt::For(_, _, _, body) | Stmt::ForIn(_, _, body) | Stmt::Do(body) => {
                fill_slots(body, filled);
            }
            Stmt::Function { body, .. } => fill_slots(body, filled),
            _ => {}
        }
        index += 1;
    }
}
