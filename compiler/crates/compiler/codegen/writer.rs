//! Writers. Packs each message into the stream with constant shifts, bits from the front and
//! bytes from the back.

use super::emitter::{Base, Emitter};
use super::types::{enum_table, fields_typed, parameters, screaming};
use crate::layout::{Chunk, Codec, Count, EventLayout, LeafKind, NodeId, PathId, Tail, bits_for, mask};
use crate::luau::builtins::*;
use std::cell::{Cell, RefCell};

use crate::luau::{BinOp, Block, ExprId, Name, Parameter, Stmt, assign, decrease, increase, local, when};
use crate::schema::UNION_TAG;

const PACKED_BITS: u32 = 21;

const BOOLS_PER_ADD: u32 = 16;

pub const MAX_CHAIN: usize = 8;

const CARRY: f64 = 1.0 / 4_294_967_296.0;

struct Locals<'a> {
    leaves: Vec<Name<'a>>,
    counts: Vec<Option<Name<'a>>>,
    casts: Vec<Option<Name<'a>>>,
}

impl<'a> Locals<'a> {
    fn new(e: &Emitter<'_, 'a>, node: NodeId) -> Locals<'a> {
        let leaves = e.layout.leaves(node).iter().map(|_| e.temp('V')).collect();
        let tails = e.layout.tails(node);
        let counts = tails
            .iter()
            .map(|tail| match tail {
                Tail::Array { count, .. } | Tail::Map { count, .. } | Tail::Bytes { count, .. }
                    if !matches!(count, Count::Fixed(_)) =>
                {
                    Some(e.temp('C'))
                }
                _ => None,
            })
            .collect();
        let casts = tails.iter().map(|tail| matches!(tail, Tail::Union { .. }).then(|| e.temp('U'))).collect();
        Locals { leaves, counts, casts }
    }

    fn count(&self, e: &Emitter<'_, 'a>, tail: usize, count: &Count) -> ExprId {
        match count {
            Count::Fixed(fixed) => e.b.number(*fixed),
            Count::Head { .. } | Count::Coded { .. } => {
                e.b.name(self.counts[tail].expect("a counted tail has a count local"))
            }
        }
    }
}

struct Writer<'e, 'f, 'a> {
    e: &'f Emitter<'e, 'a>,
    again: ExprId,
    event: &'a str,
    trail: RefCell<Vec<String>>,
    aligned: Cell<bool>,
}

impl<'a> Writer<'_, '_, 'a> {
    fn place(&self, path: PathId) -> String {
        let mut steps = Vec::new();
        let mut at = Some(path);
        while let Some(id) = at {
            let step = self.e.layout.path(id);
            steps.extend(step.field);
            at = step.parent;
        }
        steps.reverse();
        let mut place = format!("{}.{}{}", self.event, self.trail.borrow().concat(), steps.join("."));
        if place.ends_with('.') {
            place.pop();
        }
        place
    }

    fn within<T>(&self, path: PathId, step: &str, body: impl FnOnce() -> T) -> T {
        let entered = format!("{}{step}", self.place(path).split_once('.').map_or("", |(_, rest)| rest));
        let saved = std::mem::replace(&mut *self.trail.borrow_mut(), vec![entered]);
        let out = body();
        *self.trail.borrow_mut() = saved;
        out
    }

    fn fail(&self, message: String) -> Stmt<'a> {
        Stmt::Call(self.e.b.invoke(ERROR, [self.e.b.string(message)]))
    }

    fn stream(&self) -> ExprId {
        self.e.fixed("Stream")
    }

    fn cursor(&self) -> ExprId {
        self.e.fixed("Cursor")
    }

    fn room(&self, bits: Option<ExprId>) -> Stmt<'a> {
        let b = self.e.b;
        let end = match bits {
            Some(bits) => b.add(self.cursor(), bits),
            None => self.cursor(),
        };
        when(b.gt(end, b.fixed("Limit")), vec![Stmt::Return(vec![self.again])])
    }

    fn header(&self, offset: u32) -> ExprId {
        let b = self.e.b;
        b.invoke(READU32, [self.stream(), b.number(offset)])
    }

    fn write_header(&self, offset: u32, value: ExprId) -> Stmt<'a> {
        let b = self.e.b;
        Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.number(offset), value]))
    }

    fn flag_bit(&self, node: NodeId, slot: usize) -> ExprId {
        self.e.b.hex(1 << (self.e.layout.leaves(node)[slot].position % 32))
    }

    fn enum_index(&self, id: crate::schema::EnumId, value: ExprId, offset: u32, path: PathId) -> ExprId {
        let b = self.e.b;
        let declared = self.e.schema.enumeration(id);
        let names: Vec<&str> = declared.values.iter().map(|name| name.as_ref()).collect();
        if !self.chained(id) {
            let table = b.name(b.symbol(enum_table(declared)));
            let index = b.index(table, value);
            return if offset == 0 { index } else { b.add(index, b.number(offset)) };
        }
        let message = format!("Courier: {} is not a name of enum {}", self.place(path), declared.name);
        self.chain(&names, value, offset, message)
    }

    fn chained(&self, id: crate::schema::EnumId) -> bool {
        self.e.schema.enumeration(id).values.len() <= MAX_CHAIN
    }

    fn chain(&self, names: &[&str], value: ExprId, offset: u32, message: String) -> ExprId {
        let b = self.e.b;
        let mut out = b.invoke(ERROR, [b.string(message)]);
        for (index, name) in names.iter().enumerate().rev() {
            out = b.if_else(b.eq(value, b.string((*name).to_string())), b.number(offset + index as u32), out);
        }
        out
    }

    fn union_prelude(&self, node: NodeId, locals: &Locals<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        for (index, tail) in e.layout.tails(node).iter().enumerate() {
            let Tail::Union { path, union, tag, optional, .. } = tail else { continue };
            let cast = locals.casts[index].expect("a union tail has a cast local");
            out.push(local(cast, b.cast(e.read(*path), "any")));
            let declared = e.schema.union(*union);
            let offset = u32::from(*optional);
            let value = if declared.variants.len() > MAX_CHAIN {
                let table = b.name(b.symbol(format!("{}_INDEX", screaming(&declared.name))));
                let position = b.index(table, b.field(b.name(cast), UNION_TAG));
                if *optional {
                    let nil = b.eq(e.read(*path), b.nil());
                    b.if_else(nil, b.number(0), b.add(position, b.number(1)))
                } else {
                    position
                }
            } else {
                let kind = e.temp('K');
                let field = b.field(b.name(cast), UNION_TAG);
                let read = if *optional { b.if_else(b.eq(b.name(cast), b.nil()), b.nil(), field) } else { field };
                out.push(local(kind, read));
                let names: Vec<&str> = declared.variants.iter().map(|variant| variant.name.as_ref()).collect();
                let message = format!(
                    "Courier: {} names a variant that union {} does not have",
                    self.place(*path),
                    declared.name
                );
                let chain = self.chain(&names, b.name(kind), offset, message);
                if *optional { b.if_else(b.eq(b.name(kind), b.nil()), b.number(0), chain) } else { chain }
            };
            out.push(number(locals.leaves[tag.0 as usize], value));
        }
        out
    }

    fn early(&self, node: NodeId, locals: &Locals<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = self.union_prelude(node, locals);
        for (slot, leaf) in e.layout.leaves(node).iter().enumerate() {
            let value = match leaf.kind {
                LeafKind::Value(Codec::Bool) => b.if_else(e.read(leaf.path), self.flag_bit(node, slot), b.number(0)),
                LeafKind::Presence => {
                    let present = b.ne(e.read(leaf.path), b.nil());
                    b.if_else(present, self.flag_bit(node, slot), b.number(0))
                }
                LeafKind::Value(Codec::Enum { id, optional: true }) => {
                    let nil = b.eq(e.read(leaf.path), b.nil());
                    b.if_else(nil, b.number(0), self.enum_index(id, e.read(leaf.path), 1, leaf.path))
                }
                LeafKind::Value(Codec::Enum { id, optional: false }) if self.chained(id) => {
                    self.enum_index(id, e.read(leaf.path), 0, leaf.path)
                }
                _ => continue,
            };
            out.push(number(locals.leaves[slot], value));
        }
        out
    }

    fn length(&self, value: ExprId, buffer: bool) -> ExprId {
        let b = self.e.b;
        if buffer { b.invoke(LEN, [value]) } else { b.length(value) }
    }

    fn finite(&self, bits: ExprId, exponent: u64, path: PathId) -> Stmt<'a> {
        let b = self.e.b;
        let message = format!("Courier: {} must be a finite number, not NaN or infinity", self.place(path));
        when(b.eq(b.invoke(BAND, [bits, b.hex(exponent)]), b.hex(exponent)), vec![self.fail(message)])
    }

    fn too_short(&self, count: ExprId, min: u32, path: PathId, unit: &str) -> Stmt<'a> {
        let b = self.e.b;
        let message = format!("Courier: {} has fewer {unit} than its schema minimum of {min}", self.place(path));
        when(b.lt(count, b.number(min)), vec![self.fail(message)])
    }

    fn counts(&self, node: NodeId, locals: &Locals<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        for (index, tail) in e.layout.tails(node).iter().enumerate() {
            let (path, count, buffer) = match tail {
                Tail::Array { path, count, .. } => (*path, *count, false),
                Tail::Bytes { path, count, buffer } => (*path, *count, *buffer),
                Tail::Map { path, count, .. } => {
                    out.extend(self.map_count(*path, *count, locals.counts[index]));
                    continue;
                }
                Tail::Optional { .. } | Tail::Union { .. } | Tail::Instance { .. } => continue,
            };
            let unit = match tail {
                Tail::Array { .. } => "items",
                _ => "bytes",
            };
            let total = self.length(e.read(path), buffer);
            match count {
                Count::Fixed(0) => {}
                Count::Fixed(fixed) => out.push(self.too_short(total, fixed, path, unit)),
                Count::Head { min, max, .. } => {
                    let held = locals.counts[index].expect("a head count has a local");
                    out.push(local(held, b.invoke(MIN, [total, b.number(max)])));
                    if min > 0 {
                        out.push(self.too_short(b.name(held), min, path, unit));
                    }
                }
                Count::Coded { max, .. } => {
                    let held = locals.counts[index].expect("a coded count has a local");
                    let value = match max {
                        Some(max) => b.invoke(MIN, [total, b.number(max)]),
                        None => total,
                    };
                    out.push(local(held, value));
                }
            }
        }
        out
    }

    fn map_count(&self, path: PathId, count: Count, held: Option<Name<'a>>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let total = held.unwrap_or_else(|| e.temp('C'));
        let (min, max) = match count {
            Count::Fixed(fixed) => (fixed, Some(fixed)),
            Count::Head { min, max, .. } => (min, Some(max)),
            Count::Coded { max, .. } => (0, max),
        };
        let mut out = vec![
            Stmt::Local(total, Some("number".into()), Some(b.number(0))),
            Stmt::ForIn(
                vec![Name::Fixed("_")],
                vec![b.fixed("next"), e.read(path)],
                vec![increase(b.name(total), b.number(1))],
            ),
        ];
        if min > 0 {
            out.push(self.too_short(b.name(total), min, path, "entries"));
        }
        if let Some(max) = max {
            let message = format!("Courier: {} has more entries than its schema maximum of {max}", self.place(path));
            out.push(when(b.gt(b.name(total), b.number(max)), vec![self.fail(message)]));
        }
        out
    }

    fn clamp(&self, value: ExprId, max: u64) -> ExprId {
        let b = self.e.b;
        b.invoke(MIN, [b.invoke(MAX, [b.number(0), value]), b.number(max as f64)])
    }

    fn cframe_axes(&self, node: NodeId) -> Vec<Option<(usize, usize)>> {
        let leaves = self.e.layout.leaves(node);
        let mut axes = vec![None; leaves.len()];
        for (slot, leaf) in leaves.iter().enumerate() {
            if let LeafKind::Rotation { part: 0, .. } = leaf.kind {
                let children = leaves[..slot]
                    .iter()
                    .enumerate()
                    .filter(|(_, axis)| self.e.layout.path(axis.path).parent == Some(leaf.path));
                for (axis, (other, _)) in children.enumerate() {
                    axes[other] = Some((slot, axis));
                }
            }
        }
        axes
    }

    fn stores(&self, node: NodeId, locals: &Locals<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let leaves = e.layout.leaves(node);
        let axes = self.cframe_axes(node);
        let mut positions: Vec<Option<[Name<'a>; 3]>> = vec![None; leaves.len()];
        let pack = |rotation: usize, position: [Name<'a>; 3]| {
            let LeafKind::Rotation { steps, .. } = leaves[rotation].kind else {
                panic!("a rotation starts at its first part");
            };
            let mut names = position.to_vec();
            names.extend_from_slice(&locals.leaves[rotation..rotation + 4]);
            let packed = b.invoke("PackCFrame", [e.read(leaves[rotation].path), b.number(steps)]);
            Stmt::Locals(names, Some("number".into()), packed)
        };
        let mut out = Vec::new();
        for (slot, leaf) in leaves.iter().enumerate() {
            if matches!(leaf.kind, LeafKind::Group { .. }) {
                continue;
            }
            let value = match axes[slot] {
                Some((rotation, axis)) => {
                    if axis == 0 {
                        let position = [e.temp('K'), e.temp('K'), e.temp('K')];
                        out.push(pack(rotation, position));
                        positions[rotation] = Some(position);
                    }
                    let position = positions[rotation].expect("a position is packed at its first axis");
                    b.name(position[axis])
                }
                None => e.read(leaf.path),
            };
            let stored = match leaf.kind {
                LeafKind::Value(codec) => match codec {
                    Codec::Bool | Codec::Enum { optional: true, .. } => continue,
                    Codec::Enum { id, .. } if self.chained(id) => continue,
                    Codec::Int { min, span, wraps: true } => b.invoke(BAND, [b.minus(value, min as f64), b.hex(span)]),
                    Codec::Int { min, span, wraps: false } => {
                        let clamped = self.clamp(b.minus(value, min as f64), span);
                        b.invoke(BAND, [clamped, b.hex(mask(bits_for(span)))])
                    }
                    Codec::Float { min, step, steps } => {
                        let scaled = b.mul(b.minus(value, min), b.number(1.0 / step));
                        b.invoke(ROUND, [self.clamp(scaled, steps)])
                    }
                    Codec::Enum { id, optional: false } => self.enum_index(id, value, 0, leaf.path),
                    Codec::F32 => {
                        out.push(number(locals.leaves[slot], b.invoke("F32Bits", [value])));
                        out.push(self.finite(b.name(locals.leaves[slot]), 0x7F80_0000, leaf.path));
                        continue;
                    }
                    Codec::F64 { high: false } => {
                        let names = vec![locals.leaves[slot], locals.leaves[slot + 1]];
                        out.push(Stmt::Locals(names, Some("number".into()), b.invoke("F64Bits", [value])));
                        out.push(self.finite(b.name(locals.leaves[slot + 1]), 0x7FF0_0000, leaf.path));
                        continue;
                    }
                    Codec::F64 { high: true } => continue,
                },
                LeafKind::Count { min } => {
                    let held = self.count_local(node, slot, locals);
                    b.minus(b.name(held), f64::from(min))
                }
                LeafKind::Rotation { part: 0, .. } if positions[slot].is_none() => {
                    out.push(pack(slot, [Name::Fixed("_"); 3]));
                    continue;
                }
                LeafKind::Presence | LeafKind::Tag { .. } | LeafKind::Rotation { .. } | LeafKind::Group { .. } => {
                    continue;
                }
            };
            out.push(number(locals.leaves[slot], stored));
        }
        out.extend(self.groups(node, locals));
        out
    }

    fn groups(&self, node: NodeId, locals: &Locals<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        for group in e.layout.groups(node) {
            let terms: Vec<ExprId> = group
                .members
                .iter()
                .map(|member| {
                    let value = b.name(locals.leaves[member.0 as usize]);
                    let digit = e.layout.leaf(node, *member).digit.expect("a group's member is a digit");
                    if digit.place == 1 { value } else { b.mul(value, b.number(digit.place as f64)) }
                })
                .collect();
            let packed = b.sum(terms);
            let low = group.low;
            match group.high {
                None => out.push(number(locals.leaves[low.0 as usize], packed)),
                Some(high) => {
                    let held = e.temp('R');
                    out.push(number(held, packed));
                    let high_value = b.invoke(FLOOR, [b.mul(b.name(held), b.number(CARRY))]);
                    out.push(number(locals.leaves[high.0 as usize], high_value));
                    let low_value =
                        b.sub(b.name(held), b.mul(b.name(locals.leaves[high.0 as usize]), b.number(4_294_967_296.0)));
                    out.push(number(locals.leaves[low.0 as usize], low_value));
                }
            }
        }
        out
    }

    fn count_local(&self, node: NodeId, slot: usize, locals: &Locals<'a>) -> Name<'a> {
        self.e
            .layout
            .tails(node)
            .iter()
            .enumerate()
            .find_map(|(index, tail)| match tail {
                Tail::Array { count: Count::Head { leaf, .. }, .. }
                | Tail::Map { count: Count::Head { leaf, .. }, .. }
                | Tail::Bytes { count: Count::Head { leaf, .. }, .. }
                    if leaf.0 as usize == slot =>
                {
                    locals.counts[index]
                }
                _ => None,
            })
            .expect("a count leaf belongs to a counted tail")
    }

    fn words(&self, node: NodeId, locals: &Locals<'a>, id: Option<u32>) -> Vec<ExprId> {
        let (e, b) = (self.e, self.e.b);
        let mut parts: Vec<Vec<ExprId>> = vec![Vec::new(); e.layout.node(node).words() as usize];
        if let Some(id) = id {
            parts[0].push(b.number(id));
        }
        for (slot, leaf) in e.layout.leaves(node).iter().enumerate() {
            if !leaf.is_placed() {
                continue;
            }
            let at = (leaf.position / 32) as usize;
            let offset = leaf.position % 32;
            let value = b.name(locals.leaves[slot]);
            if leaf.is_flag() || offset == 0 && leaf.bits <= 32 {
                parts[at].push(value);
            } else if offset + leaf.bits <= 32 {
                parts[at].push(b.mul(value, b.number((1u64 << offset) as f64)));
            } else {
                parts[at].push(b.invoke(LSHIFT, [value, b.number(offset)]));
            }
            if !leaf.is_flag() && offset + leaf.bits > 32 {
                parts[at + 1].push(b.invoke(RSHIFT, [b.name(locals.leaves[slot]), b.number(32 - offset)]));
            }
        }
        parts.into_iter().map(|terms| b.sum(terms)).collect()
    }

    fn merge(&self, node: NodeId, locals: &Locals<'a>, id: Option<u32>) -> Block<'a> {
        let b = self.e.b;
        let bits = self.e.layout.node(node).bits;
        let words = self.e.layout.node(node).words();
        let byte = || b.fixed("Byte");
        let at_byte = |offset: u32| if offset == 0 { byte() } else { b.add(byte(), b.number(offset)) };
        let scaled = |at: u32| b.name(Name::Temp('M', at));
        let carry = |at: u32| b.invoke(FLOOR, [b.mul(scaled(at), b.number(CARRY))]);

        let mut body: Block<'a> = self
            .words(node, locals, id)
            .into_iter()
            .enumerate()
            .map(|(at, value)| local(Name::Temp('W', at as u32), value))
            .collect();
        body.push(local(Name::Fixed("Byte"), b.floor_div(self.cursor(), b.number(8))));
        if self.aligned.get() {
            for at in 0..words {
                let write = match bits - at * 32 {
                    8 => WRITEU8,
                    16 => WRITEU16,
                    _ => WRITEU32,
                };
                let value = b.name(Name::Temp('W', at));
                body.push(Stmt::Call(b.invoke(write, [self.stream(), at_byte(at * 4), value])));
            }
            return vec![Stmt::Do(body), increase(self.cursor(), b.number(bits))];
        }
        let shift = b.invoke(BAND, [self.cursor(), b.number(7)]);
        body.push(local(Name::Fixed("P"), b.invoke(LSHIFT, [b.number(1), shift])));
        for at in 0..words {
            body.push(local(Name::Temp('M', at), b.mul(b.name(Name::Temp('W', at)), b.fixed("P"))));
        }
        if bits + 7 > words * 32 {
            body.push(Stmt::Call(b.invoke(WRITEU8, [self.stream(), at_byte(words * 4), carry(words - 1)])));
        }
        for at in (1..words).rev() {
            let value = b.add(scaled(at), carry(at - 1));
            body.push(Stmt::Call(b.invoke(WRITEU32, [self.stream(), at_byte(at * 4), value])));
        }
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), byte()]), b.sub(b.fixed("P"), b.number(1))]);
        body.push(Stmt::Call(b.invoke(WRITEU32, [self.stream(), byte(), b.add(scaled(0), kept)])));
        vec![Stmt::Do(body), increase(self.cursor(), b.number(bits))]
    }

    fn id_merge(&self, code: u32) -> Stmt<'a> {
        let b = self.e.b;
        let start = b.fixed("Start");
        let scale = b.invoke(LSHIFT, [b.number(1), b.invoke(BAND, [start, b.number(7)])]);
        let byte = b.floor_div(start, b.number(8));
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), byte]), b.sub(scale, b.number(1))]);
        let value = b.add(b.mul(b.number(code), scale), kept);
        Stmt::Call(b.invoke(WRITEU32, [self.stream(), byte, value]))
    }

    fn coded(&self, count: &Count, held: Option<Name<'a>>) -> Block<'a> {
        let Count::Coded { k, .. } = *count else { return Vec::new() };
        let (e, b) = (self.e, self.e.b);
        let held = held.expect("a coded count has a local");
        let (shifted, top, byte) = (e.temp('H'), e.temp('N'), e.temp('B'));
        let low = || b.invoke(BAND, [self.cursor(), b.number(7)]);
        let scale = b.invoke(LSHIFT, [b.number(1), b.sub(b.add(low(), b.name(top)), b.number(k))]);
        let code = b.sub(
            b.add(b.add(b.name(shifted), b.name(shifted)), b.number(1)),
            b.invoke(LSHIFT, [b.number(2), b.name(top)]),
        );
        let kept = b.invoke(
            BAND,
            [
                b.invoke(READU8, [self.stream(), b.name(byte)]),
                b.sub(b.invoke(LSHIFT, [b.number(1), low()]), b.number(1)),
            ],
        );
        let inline = vec![
            local(byte, b.floor_div(self.cursor(), b.number(8))),
            Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.add(b.mul(code, scale), kept)])),
            increase(self.cursor(), b.sub(b.add(b.name(top), b.name(top)), b.number(k - 1))),
        ];
        let long = vec![assign(
            self.cursor(),
            b.invoke("WriteCoded", [self.stream(), self.cursor(), b.name(held), b.number(k)]),
        )];
        let fits = b.lt(b.add(b.name(top), b.name(top)), b.number(25 + k));
        vec![Stmt::Do(vec![
            local(shifted, b.add(b.name(held), b.number(1u32 << k))),
            local(top, b.sub(b.number(31), b.invoke(COUNTLZ, [b.name(shifted)]))),
            self.room(Some(b.sub(b.add(b.name(top), b.name(top)), b.number(k - 1)))),
            Stmt::If(vec![(fits, inline)], Some(long)),
        ])]
    }

    fn by_variant(
        &self,
        node: NodeId,
        tail: usize,
        locals: &Locals<'a>,
        mut body: impl FnMut(NodeId) -> Block<'a>,
    ) -> Option<Stmt<'a>> {
        let (e, b) = (self.e, self.e.b);
        let Tail::Union { tag, variants, optional, .. } = &e.layout.tails(node)[tail] else {
            return None;
        };
        let cast = locals.casts[tail].expect("a union tail has a cast local");
        let first = u32::from(*optional);
        let mut branches = Vec::new();
        for (position, variant) in e.layout.variant_nodes(variants).iter().enumerate() {
            e.bind(e.layout.node(*variant).base, Base::Local(cast));
            let statements = body(*variant);
            if !is_empty(&statements) {
                let test = b.eq(b.name(locals.leaves[tag.0 as usize]), b.number(first + position as u32));
                branches.push((test, statements));
            }
        }
        (!branches.is_empty()).then_some(Stmt::If(branches, None))
    }

    fn tails(&self, node: NodeId, locals: &Locals<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        for (index, tail) in e.layout.tails(node).iter().enumerate() {
            match tail {
                Tail::Optional { path, node: inner, .. } => {
                    out.push(when(b.ne(e.read(*path), b.nil()), self.node_body(*inner, true)));
                }
                Tail::Array { path, count, element, chunk, padded } => {
                    out.extend(self.coded(count, locals.counts[index]));
                    let total = locals.count(e, index, count);
                    if *padded {
                        let past = b.add(self.cursor(), b.number(7));
                        out.push(assign(self.cursor(), b.invoke(BAND, [past, b.hex(0xFFFF_FFF8)])));
                    }
                    let aligned = self.aligned.replace(*padded);
                    out.extend(self.within(*path, "[].", || self.array(*path, count, *element, *chunk, total)));
                    self.aligned.set(aligned);
                }
                Tail::Map { path, count, key, value } => {
                    out.extend(self.coded(count, locals.counts[index]));
                    let (held_key, held_value) = (e.temp('K'), e.temp('X'));
                    e.bind(e.layout.node(*key).base, Base::Local(held_key));
                    e.bind(e.layout.node(*value).base, Base::Local(held_value));
                    let mut body = self.within(*path, "[key].", || self.node_body(*key, true));
                    body.extend(self.within(*path, "[value].", || self.node_body(*value, true)));
                    let iterable = vec![b.fixed("next"), b.cast(e.read(*path), "any")];
                    out.push(Stmt::ForIn(vec![held_key, held_value], iterable, body));
                }
                Tail::Bytes { path, count, buffer } => {
                    let total = locals.count(e, index, count);
                    let lower = vec![
                        decrease(b.fixed("Top"), total),
                        assign(b.fixed("Limit"), b.sub(b.mul(b.fixed("Top"), b.number(8)), b.fixed("GAP"))),
                    ];
                    match count {
                        Count::Fixed(0) => {}
                        Count::Fixed(_) => out.extend(lower),
                        _ => out.push(when(b.gt(total, b.number(0)), lower)),
                    }
                    if matches!(count, Count::Coded { .. }) {
                        out.extend(self.coded(count, locals.counts[index]));
                    } else {
                        out.push(self.room(None));
                    }
                    let copy = if *buffer {
                        b.invoke(COPY, [self.stream(), b.fixed("Top"), e.read(*path), b.number(0), total])
                    } else {
                        b.invoke(WRITESTRING, [self.stream(), b.fixed("Top"), e.read(*path), total])
                    };
                    out.push(Stmt::Call(copy));
                }
                Tail::Instance { path, handle, .. } => {
                    out.push(increase(b.fixed("InstanceCount"), b.number(1)));
                    let value = if *handle {
                        b.or(e.read(*path), b.fixed("EMPTY_HANDLE"))
                    } else {
                        let cached = b.index(b.fixed("HANDLES"), e.read(*path));
                        b.or(cached, b.invoke("NewHandle", [e.read(*path)]))
                    };
                    out.push(assign(b.index(b.fixed("Instances"), b.fixed("InstanceCount")), value));
                }
                Tail::Union { path, .. } => {
                    let body = |variant| self.within(*path, ".", || self.node_body(variant, true));
                    out.extend(self.by_variant(node, index, locals, body));
                }
            }
        }
        out
    }

    fn array(&self, path: PathId, count: &Count, element: NodeId, chunk: Option<Chunk>, total: ExprId) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let (position, item) = (e.temp('I'), e.temp('X'));
        let element_node = e.layout.node(element);
        e.bind(element_node.base, Base::Local(item));
        let mut body = vec![local(item, b.index(e.read(path), b.name(position)))];
        if let Some(chunk) = chunk {
            let whole = count_max(count).is_none_or(|max| max >= chunk.size);
            return self.chunked_array(element, chunk, whole, total, position, body);
        }
        let mut out = Vec::new();
        let flat = e.layout.tails(element).is_empty();
        if flat && element_node.bits > 0 {
            out.push(self.room(Some(b.mul(total, b.number(element_node.bits)))));
        }
        if e.layout.is_bare_bool(element) && !matches!(count, Count::Fixed(fixed) if *fixed < BOOLS_PER_ADD) {
            out.extend(self.bool_array(e.read(path), total));
            return out;
        }
        if let Some(write) = self.whole_write(element) {
            out.extend(self.whole_array(element, write, total, position, body));
            return out;
        }
        if flat && element_node.bits > 0 && element_node.bits <= PACKED_BITS && !self.aligned.get() {
            out.extend(self.packed_array(element, total, position, body));
            return out;
        }
        body.extend(self.node_body(element, !flat));
        out.push(Stmt::For(position, b.number(1), total, body));
        out
    }

    fn whole_write(&self, element: NodeId) -> Option<&'static str> {
        let layout = self.e.layout;
        let leaves = layout.leaves(element);
        if !self.aligned.get() || !layout.tails(element).is_empty() || !layout.groups(element).is_empty() {
            return None;
        }
        match leaves {
            [leaf]
                if leaf.is_placed()
                    && !leaf.is_flag()
                    && leaf.position == 0
                    && leaf.bits == layout.node(element).bits =>
            {
                match leaf.bits {
                    8 => Some(WRITEU8),
                    16 => Some(WRITEU16),
                    32 => Some(WRITEU32),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn whole_array(
        &self,
        element: NodeId,
        write: &'static str,
        total: ExprId,
        position: Name<'a>,
        prefix: Block<'a>,
    ) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let bytes = e.layout.node(element).bits / 8;
        let byte = e.temp('B');
        let mut body = prefix;
        body.extend(e.scope(e.layout.node(element).base, || {
            let locals = Locals::new(e, element);
            let mut out = self.early(element, &locals);
            out.extend(self.stores(element, &locals));
            out.push(Stmt::Call(b.invoke(write, [self.stream(), b.name(byte), b.name(locals.leaves[0])])));
            out.push(increase(b.name(byte), b.number(bytes)));
            out
        }));
        vec![
            Stmt::Do(vec![
                local(byte, b.floor_div(self.cursor(), b.number(8))),
                Stmt::For(position, b.number(1), total, body),
            ]),
            increase(self.cursor(), b.mul(total, b.number(e.layout.node(element).bits))),
        ]
    }

    fn packed_array(&self, element: NodeId, total: ExprId, position: Name<'a>, prefix: Block<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let bits = e.layout.node(element).bits;
        let (byte, fill, scale, acc) = (e.temp('B'), e.temp('F'), e.temp('G'), e.temp('Q'));
        let mut body = prefix;
        body.extend(e.scope(e.layout.node(element).base, || {
            let locals = Locals::new(e, element);
            let mut out = self.early(element, &locals);
            out.extend(self.stores(element, &locals));
            let word = self.words(element, &locals, None).swap_remove(0);
            out.push(increase(b.name(acc), b.mul(word, b.name(scale))));
            out.push(Stmt::Compound(b.name(scale), BinOp::Mul, b.number((1u64 << bits) as f64)));
            out.push(increase(b.name(fill), b.number(bits)));
            out.push(when(
                b.binary(BinOp::Le, b.number(32), b.name(fill)),
                vec![
                    Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.name(acc)])),
                    increase(b.name(byte), b.number(4)),
                    assign(b.name(acc), b.invoke(FLOOR, [b.mul(b.name(acc), b.number(CARRY))])),
                    Stmt::Compound(b.name(scale), BinOp::Mul, b.number(CARRY)),
                    decrease(b.name(fill), b.number(32)),
                ],
            ));
            out
        }));
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), b.name(byte)]), b.sub(b.name(scale), b.number(1))]);
        vec![
            Stmt::Do(vec![
                local(byte, b.floor_div(self.cursor(), b.number(8))),
                local(fill, b.invoke(BAND, [self.cursor(), b.number(7)])),
                local(scale, b.invoke(LSHIFT, [b.number(1), b.name(fill)])),
                local(acc, kept),
                Stmt::For(position, b.number(1), total, body),
                Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.name(acc)])),
            ]),
            increase(self.cursor(), b.mul(total, b.number(bits))),
        ]
    }

    fn bool_array(&self, array: ExprId, total: ExprId) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let names = [e.temp('B'), e.temp('F'), e.temp('G'), e.temp('Q')];
        let [byte, fill, scale, acc] = names;
        let (step, end, value, position) = (e.temp('P'), e.temp('E'), e.temp('V'), e.temp('I'));
        let bit = |index: ExprId, weight: u64| b.if_else(b.index(array, index), b.hex(weight), b.number(0));
        let terms = (0..BOOLS_PER_ADD).map(|at| bit(b.minus(b.name(end), f64::from(BOOLS_PER_ADD - 1 - at)), 1 << at));
        let mut adds = vec![local(end, b.mul(b.name(step), b.number(BOOLS_PER_ADD))), number(value, b.sum(terms))];
        let whole_scale = b.number((1u64 << BOOLS_PER_ADD) as f64);
        adds.extend(self.accumulate(names, b.name(value), whole_scale, b.number(BOOLS_PER_ADD)));
        let single = self.accumulate(names, bit(b.name(position), 1), b.number(2), b.number(1));
        let groups = b.floor_div(total, b.number(BOOLS_PER_ADD));
        let done = b.mul(groups, b.number(BOOLS_PER_ADD));
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), b.name(byte)]), b.sub(b.name(scale), b.number(1))]);
        vec![
            Stmt::Do(vec![
                local(byte, b.floor_div(self.cursor(), b.number(8))),
                local(fill, b.invoke(BAND, [self.cursor(), b.number(7)])),
                local(scale, b.invoke(LSHIFT, [b.number(1), b.name(fill)])),
                local(acc, kept),
                Stmt::For(step, b.number(1), groups, adds),
                Stmt::For(position, b.add(done, b.number(1)), total, single),
                Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.name(acc)])),
            ]),
            increase(self.cursor(), total),
        ]
    }

    fn accumulate(&self, names: [Name<'a>; 4], value: ExprId, scale_by: ExprId, bits: ExprId) -> Block<'a> {
        let b = self.e.b;
        let [byte, fill, scale, acc] = names;
        vec![
            increase(b.name(acc), b.mul(value, b.name(scale))),
            Stmt::Compound(b.name(scale), BinOp::Mul, scale_by),
            increase(b.name(fill), bits),
            when(
                b.binary(BinOp::Le, b.number(32), b.name(fill)),
                vec![
                    Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.name(acc)])),
                    increase(b.name(byte), b.number(4)),
                    assign(b.name(acc), b.invoke(FLOOR, [b.mul(b.name(acc), b.number(CARRY))])),
                    Stmt::Compound(b.name(scale), BinOp::Mul, b.number(CARRY)),
                    decrease(b.name(fill), b.number(32)),
                ],
            ),
        ]
    }

    fn chunked_array(
        &self,
        element: NodeId,
        chunk: Chunk,
        whole: bool,
        total: ExprId,
        position: Name<'a>,
        prefix: Block<'a>,
    ) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let names = [e.temp('B'), e.temp('F'), e.temp('G'), e.temp('Q')];
        let [byte, fill, scale, acc] = names;
        let (packed, place, held) = (e.temp('R'), e.temp('L'), e.temp('N'));
        let table = b.name(b.symbol(chunk_table(chunk)));
        let width = e.temp('W');
        let mut body = prefix;
        body.extend(e.scope(e.layout.node(element).base, || {
            let locals = Locals::new(e, element);
            let mut out = self.early(element, &locals);
            out.extend(self.stores(element, &locals));
            let value = b.name(locals.leaves[0]);
            out.push(increase(b.name(packed), b.mul(value, b.name(place))));
            out.push(Stmt::Compound(b.name(place), BinOp::Mul, b.number(chunk.base as f64)));
            if whole {
                out.push(increase(b.name(held), b.number(1)));
                let mut full =
                    self.accumulate(names, b.name(packed), b.number((1u64 << chunk.bits) as f64), b.number(chunk.bits));
                full.extend([
                    assign(b.name(packed), b.number(0)),
                    assign(b.name(place), b.number(1)),
                    assign(b.name(held), b.number(0)),
                ]);
                out.push(when(b.eq(b.name(held), b.number(chunk.size)), full));
            }
            out
        }));
        if !whole {
            let low = || b.invoke(BAND, [self.cursor(), b.number(7)]);
            let kept = b.invoke(
                BAND,
                [
                    b.invoke(READU8, [self.stream(), b.name(byte)]),
                    b.sub(b.invoke(LSHIFT, [b.number(1), low()]), b.number(1)),
                ],
            );
            let scaled = b.mul(b.name(packed), b.invoke(LSHIFT, [b.number(1), low()]));
            return vec![
                number(width, b.index(table, b.add(total, b.number(1)))),
                self.room(Some(b.name(width))),
                Stmt::Do(vec![
                    local(packed, b.number(0)),
                    local(place, b.number(1)),
                    Stmt::For(position, b.number(1), total, body),
                    local(byte, b.floor_div(self.cursor(), b.number(8))),
                    Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.add(scaled, kept)])),
                ]),
                increase(self.cursor(), b.name(width)),
            ];
        }
        let mut rest =
            vec![Stmt::Local(width, Some("number".into()), Some(b.index(table, b.add(b.name(held), b.number(1)))))];
        rest.extend(self.accumulate(
            names,
            b.name(packed),
            b.invoke(LSHIFT, [b.number(1), b.name(width)]),
            b.name(width),
        ));
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), b.name(byte)]), b.sub(b.name(scale), b.number(1))]);
        let all = e.temp('W');
        vec![
            number(all, chunk_bits(e, chunk, total)),
            self.room(Some(b.name(all))),
            Stmt::Do(vec![
                local(byte, b.floor_div(self.cursor(), b.number(8))),
                local(fill, b.invoke(BAND, [self.cursor(), b.number(7)])),
                local(scale, b.invoke(LSHIFT, [b.number(1), b.name(fill)])),
                local(acc, kept),
                local(packed, b.number(0)),
                local(place, b.number(1)),
                local(held, b.number(0)),
                Stmt::For(position, b.number(1), total, body),
                when(b.gt(b.name(held), b.number(0)), rest),
                Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.name(acc)])),
            ]),
            increase(self.cursor(), b.name(all)),
        ]
    }

    fn node_body(&self, node: NodeId, check: bool) -> Block<'a> {
        self.e.scope(self.e.layout.node(node).base, || {
            let locals = Locals::new(self.e, node);
            let mut out = self.early(node, &locals);
            out.extend(self.counts(node, &locals));
            let bits = self.e.layout.node(node).bits;
            if check && bits > 0 {
                out.push(self.room(Some(self.e.b.number(bits))));
            }
            out.extend(self.stores(node, &locals));
            if self.e.layout.node(node).bits > 0 {
                out.extend(self.merge(node, &locals, None));
            }
            out.extend(self.tails(node, &locals));
            out
        })
    }

    fn message(&self, event: &EventLayout) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let node = event.node;
        let bits = e.layout.node(node).bits;
        let bytes = e.layout.any_tail(node, &|tail| matches!(tail, Tail::Bytes { .. }));
        let instances = e.layout.any_tail(node, &|tail| matches!(tail, Tail::Instance { .. }));
        let locals = Locals::new(e, node);
        let mut body = self.early(node, &locals);
        body.extend(self.counts(node, &locals));
        if event.aligned {
            body.push(local(Name::Fixed("Start"), self.header(0)));
            let past = b.add(b.fixed("Start"), b.number(event.code_bits + 7));
            body.push(local(Name::Fixed("Cursor"), b.invoke(BAND, [past, b.hex(0xFFFF_FFF8)])));
        } else {
            body.push(local(Name::Fixed("Cursor"), self.header(0)));
        }
        if e.layout.tails(node).is_empty() {
            let full = b.invoke("Full", [self.stream(), b.number(bits), b.number(0)]);
            body.push(when(
                b.gt(b.add(self.cursor(), b.number(bits)), self.header(4)),
                vec![Stmt::Call(full), assign(self.cursor(), self.header(0))],
            ));
        } else {
            if bytes {
                body.push(local(Name::Fixed("Top"), self.header(8)));
            }
            body.push(local(Name::Fixed("Limit"), self.header(4)));
            body.push(self.room(Some(b.number(bits))));
            if instances {
                let listed = b.index(b.fixed("STREAM_INSTANCES"), self.stream());
                body.push(local(Name::Fixed("Instances"), b.or(listed, b.invoke("StreamInstances", [self.stream()]))));
                body.push(local(Name::Fixed("InstanceCount"), self.header(12)));
            }
        }
        body.extend(self.stores(node, &locals));
        if event.aligned {
            body.push(self.id_merge(event.code));
            if bits > 0 {
                body.extend(self.merge(node, &locals, None));
            }
        } else {
            body.extend(self.merge(node, &locals, Some(event.code)));
        }
        body.extend(self.tails(node, &locals));
        body.push(self.write_header(0, self.cursor()));
        if event.aligned {
            let mark = b.sub(b.add(b.fixed("Start"), b.number(event.code_bits + 1)), b.fixed("HEADER_BITS"));
            body.push(when(b.eq(self.header(16), b.number(0)), vec![self.write_header(16, mark)]));
        }
        if instances {
            body.push(self.write_header(12, b.fixed("InstanceCount")));
        }
        if bytes {
            body.push(self.write_header(8, b.fixed("Top")));
            body.push(self.write_header(4, b.fixed("Limit")));
        }
        body
    }
}

pub fn count_max(count: &Count) -> Option<u32> {
    match *count {
        Count::Fixed(fixed) => Some(fixed),
        Count::Head { max, .. } => Some(max),
        Count::Coded { max, .. } => max,
    }
}

pub fn chunk_table(chunk: Chunk) -> String {
    format!("CHUNK_BITS_{}", chunk.base)
}

pub fn chunk_bits<'a>(e: &Emitter<'_, 'a>, chunk: Chunk, total: ExprId) -> ExprId {
    let b = e.b;
    let whole = b.floor_div(total, b.number(chunk.size));
    let rest = b.sub(total, b.mul(whole, b.number(chunk.size)));
    let table = b.name(b.symbol(chunk_table(chunk)));
    b.add(b.mul(whole, b.number(chunk.bits)), b.index(table, b.add(rest, b.number(1))))
}

fn number<'a>(name: Name<'a>, value: ExprId) -> Stmt<'a> {
    Stmt::Local(name, Some("number".into()), Some(value))
}

fn is_empty(block: &[Stmt]) -> bool {
    block.iter().all(|statement| matches!(statement, Stmt::Slot(_)))
}

pub fn writer<'a>(e: &Emitter<'_, 'a>, event: &EventLayout, spilled: bool) -> Stmt<'a> {
    e.begin_function();
    let declared = &e.schema.events[event.event];
    let base = e.layout.node(event.node).base;
    e.bind(base, Base::Parameters);
    let mut signature = vec![Parameter::new(Name::Fixed("Stream"), "Stream")];
    signature.extend(parameters(&fields_typed(e.schema, &declared.fields, true)));
    let callee =
        if spilled { e.b.field(e.b.fixed("WRITERS"), &declared.name) } else { e.b.name(writer_name(e, event)) };
    let mut arguments = vec![e.b.fixed("Stream"), callee];
    arguments.extend(signature[1..].iter().map(|parameter| e.b.name(parameter.name)));
    let again = e.b.invoke("Overflow", arguments);
    let writer = Writer { e, again, event: &declared.name, trail: RefCell::new(Vec::new()), aligned: Cell::new(false) };
    let mut body = e.scope(base, || writer.message(event));
    e.hoist(&mut body);
    Stmt::Function {
        name: e.b.symbol(format!("Write{}", declared.name)),
        parameters: signature,
        returns: "()".into(),
        body,
    }
}

pub fn writer_name<'a>(e: &Emitter<'_, 'a>, event: &EventLayout) -> Name<'a> {
    e.b.symbol(format!("Write{}", e.schema.events[event.event].name))
}
