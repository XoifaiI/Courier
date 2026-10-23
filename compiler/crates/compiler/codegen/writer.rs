//! Writers. Packs each message into the stream with constant shifts, bits from the front and
//! bytes from the back.

use super::emitter::{Base, Emitter};
use super::types::{enum_table, fields_typed, parameters, screaming};
use crate::layout::{Chunk, Codec, Count, EventLayout, Leaf, LeafKind, NodeId, PathId, Tail, bits_for, mask};
use crate::luau::builtins::*;
use std::cell::{Cell, RefCell};
use std::ops::Range;

use crate::luau::{BinOp, Block, ExprId, Name, Parameter, Stmt, assign, decrease, increase, local, when};
use crate::schema::UNION_TAG;

const PACKED_BITS: u32 = 21;

const BOOLS_PER_ADD: u32 = 8;

pub const MAX_CHAIN: usize = 8;

pub const F32_LIMIT: f64 = 340_282_356_779_733_661_637_539_395_458_142_568_448.0;

const CARRY: f64 = 1.0 / 4_294_967_296.0;

const SCALED: u32 = 3;

pub const WIDE_LOCALS: usize = 120;

pub const CHUNK_WORDS: u32 = 16;

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
    raw_floats: Cell<bool>,
    shared: RefCell<Vec<bool>>,
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

    fn finite_value(&self, value: ExprId, path: PathId) -> Stmt<'a> {
        let b = self.e.b;
        let message = format!("Courier: {} must be a finite number, not NaN or infinity", self.place(path));
        when(b.not(b.lt(b.invoke(ABS, [value]), b.number(F32_LIMIT))), vec![self.fail(message)])
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
            let total = match tail {
                Tail::Bytes { buffer: false, .. } => b.invoke(STRING_LEN, [e.read(path)]),
                _ => self.length(e.read(path), buffer),
            };
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
        self.stores_where(node, locals, &|_| true)
    }

    fn stores_where(&self, node: NodeId, locals: &Locals<'a>, include: &dyn Fn(usize) -> bool) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let leaves = e.layout.leaves(node);
        let axes = self.cframe_axes(node);
        let mut positions: Vec<Option<[Name<'a>; 3]>> = vec![None; leaves.len()];
        let pack = |rotation: usize, position: [Name<'a>; 3]| {
            let LeafKind::Rotation { steps, .. } = leaves[rotation].kind else {
                panic!("a rotation starts at its first part");
            };
            let packed = b.invoke("PackCFrame", [e.read(leaves[rotation].path), b.number(steps)]);
            let mut targets: Vec<(Name<'a>, Option<usize>)> = position.iter().map(|name| (*name, None)).collect();
            targets.extend((rotation..rotation + 4).map(|slot| (locals.leaves[slot], Some(slot))));
            self.unpack(&targets, packed, locals)
        };
        let mut out = Vec::new();
        for (slot, leaf) in leaves.iter().enumerate() {
            if matches!(leaf.kind, LeafKind::Group { .. }) || !include(slot) {
                continue;
            }
            let value = match axes[slot] {
                Some((rotation, axis)) => {
                    if axis == 0 {
                        let position = [e.temp('K'), e.temp('K'), e.temp('K')];
                        out.extend(pack(rotation, position));
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
                    Codec::F32 if self.raw_floats.get() => {
                        out.push(self.bind(slot, locals, value));
                        out.push(self.finite_value(b.name(locals.leaves[slot]), leaf.path));
                        continue;
                    }
                    Codec::F32 => {
                        out.push(self.bind(slot, locals, b.invoke("F32Bits", [value])));
                        out.push(self.finite(b.name(locals.leaves[slot]), 0x7F80_0000, leaf.path));
                        continue;
                    }
                    Codec::F64 { high: false } => {
                        let targets = [(locals.leaves[slot], Some(slot)), (locals.leaves[slot + 1], Some(slot + 1))];
                        out.extend(self.unpack(&targets, b.invoke("F64Bits", [value]), locals));
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
                    out.extend(pack(slot, [Name::Fixed("_"); 3]));
                    continue;
                }
                LeafKind::Presence | LeafKind::Tag { .. } | LeafKind::Rotation { .. } | LeafKind::Group { .. } => {
                    continue;
                }
            };
            out.push(self.bind(slot, locals, stored));
        }
        out.extend(self.groups(node, locals, include));
        out
    }

    fn groups(&self, node: NodeId, locals: &Locals<'a>, include: &dyn Fn(usize) -> bool) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        for group in e.layout.groups(node).iter().filter(|group| include(group.low.0 as usize)) {
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
                None => out.push(self.bind(low.0 as usize, locals, packed)),
                Some(high) => {
                    let held = e.temp('R');
                    out.push(number(held, packed));
                    let high_value = b.invoke(FLOOR, [b.mul(b.name(held), b.number(CARRY))]);
                    out.push(self.bind(high.0 as usize, locals, high_value));
                    let low_value =
                        b.sub(b.name(held), b.mul(b.name(locals.leaves[high.0 as usize]), b.number(4_294_967_296.0)));
                    out.push(self.bind(low.0 as usize, locals, low_value));
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

    fn wide(&self, node: NodeId) -> bool {
        let layout = &self.e.layout;
        let groups: usize = layout.groups(node).iter().map(|group| group.members.len() + 1).sum();
        2 * layout.node(node).words() as usize + layout.leaves(node).len() + groups > WIDE_LOCALS
    }

    fn atoms(&self, node: NodeId) -> Vec<usize> {
        let leaves = self.e.layout.leaves(node);
        let mut atoms: Vec<usize> = (0..leaves.len()).collect();
        for (slot, axis) in self.cframe_axes(node).into_iter().enumerate() {
            if let Some((rotation, _)) = axis {
                atoms[slot] = rotation;
            }
        }
        for (slot, leaf) in leaves.iter().enumerate() {
            match leaf.kind {
                LeafKind::Rotation { part, .. } if part > 0 => atoms[slot] = slot - part as usize,
                LeafKind::Value(Codec::F64 { high: true }) => atoms[slot] = slot - 1,
                _ => {}
            }
        }
        atoms
    }

    fn chunks(&self, node: NodeId) -> (Vec<Range<u32>>, Vec<usize>, Vec<bool>) {
        let leaves = self.e.layout.leaves(node);
        let words = self.e.layout.node(node).words();
        if !self.wide(node) {
            return (std::iter::once(0..words).collect(), vec![0; leaves.len()], vec![false; leaves.len()]);
        }
        let atoms = self.atoms(node);
        let mut span: Vec<Option<(u32, u32)>> = vec![None; leaves.len()];
        for (slot, leaf) in leaves.iter().enumerate().filter(|(_, leaf)| leaf.is_placed()) {
            let (first, last) = (leaf.position / 32, (leaf.position + leaf.bits.max(1) - 1) / 32);
            let known = span[atoms[slot]].get_or_insert((first, last));
            *known = (known.0.min(first), known.1.max(last));
        }
        let mut size = vec![0; leaves.len()];
        for slot in 0..leaves.len() {
            size[atoms[slot]] += 1;
        }
        let blocked = |at: u32| {
            span.iter()
                .enumerate()
                .any(|(atom, known)| size[atom] > 1 && known.is_some_and(|(first, last)| first < at && last >= at))
        };
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < words {
            let mut end = (start + CHUNK_WORDS).min(words);
            while end < words && blocked(end) {
                end += 1;
            }
            chunks.push(start..end);
            start = end;
        }
        let chunk_of = |position: u32| chunks.iter().position(|chunk| chunk.contains(&(position / 32))).unwrap_or(0);
        let last_of = |leaf: &crate::layout::Leaf| chunk_of(leaf.position + leaf.bits.max(1) - 1);
        let mut used = vec![0; leaves.len()];
        for (slot, leaf) in leaves.iter().enumerate().filter(|(_, leaf)| leaf.is_placed()) {
            used[slot] = chunk_of(leaf.position);
        }
        for group in self.e.layout.groups(node) {
            let summed = chunk_of(leaves[group.low.0 as usize].position);
            for member in &group.members {
                used[member.0 as usize] = summed;
            }
            for part in std::iter::once(group.low).chain(group.high) {
                used[part.0 as usize] = summed.min(used[part.0 as usize]);
            }
        }
        let mut first = vec![usize::MAX; leaves.len()];
        for slot in 0..leaves.len() {
            first[atoms[slot]] = first[atoms[slot]].min(used[slot]);
        }
        let owner: Vec<usize> = (0..leaves.len()).map(|slot| first[atoms[slot]]).collect();
        let mut shared: Vec<bool> = (0..leaves.len())
            .map(|slot| owner[slot] != chunk_of(leaves[slot].position) || owner[slot] != last_of(&leaves[slot]))
            .collect();
        for (slot, leaf) in leaves.iter().enumerate() {
            if !leaf.is_placed() {
                shared[slot] = owner[slot] != used[slot];
            }
        }
        for group in self.e.layout.groups(node) {
            let summed = chunk_of(leaves[group.low.0 as usize].position);
            for part in std::iter::once(group.low).chain(group.high) {
                let leaf = &leaves[part.0 as usize];
                shared[part.0 as usize] = chunk_of(leaf.position) != summed || last_of(leaf) != summed;
            }
        }
        for (slot, leaf) in leaves.iter().enumerate() {
            let bound = match leaf.kind {
                LeafKind::Value(Codec::Bool | Codec::Enum { optional: true, .. }) => false,
                LeafKind::Value(Codec::Enum { id, .. }) => !self.chained(id),
                LeafKind::Value(_) | LeafKind::Count { .. } | LeafKind::Group { .. } => true,
                LeafKind::Rotation { .. } => true,
                LeafKind::Presence | LeafKind::Tag { .. } => false,
            };
            shared[slot] &= bound;
        }
        (chunks, owner, shared)
    }

    fn unpack(&self, targets: &[(Name<'a>, Option<usize>)], value: ExprId, locals: &Locals<'a>) -> Block<'a> {
        let shared =
            |slot: Option<usize>| slot.is_some_and(|slot| self.shared.borrow().get(slot).copied().unwrap_or(false));
        let held: Vec<Name<'a>> =
            targets.iter().map(|(name, slot)| if shared(*slot) { self.e.temp('T') } else { *name }).collect();
        let mut out = vec![Stmt::Locals(held.clone(), Some("number".into()), value)];
        for ((_, slot), name) in targets.iter().zip(held) {
            if let Some(slot) = slot.filter(|slot| shared(Some(*slot))) {
                out.push(self.bind(slot, locals, self.e.b.name(name)));
            }
        }
        out
    }

    fn bind(&self, slot: usize, locals: &Locals<'a>, value: ExprId) -> Stmt<'a> {
        if self.shared.borrow().get(slot).copied().unwrap_or(false) {
            return assign(self.e.b.name(locals.leaves[slot]), value);
        }
        number(locals.leaves[slot], value)
    }

    fn head(&self, node: NodeId, locals: &Locals<'a>, id: Option<u32>, before: Option<Stmt<'a>>) -> Block<'a> {
        let bits = self.e.layout.node(node).bits;
        let (chunks, owner, shared) = self.chunks(node);
        if chunks.len() == 1 {
            let mut out = self.stores(node, locals);
            out.extend(before);
            if bits > 0 {
                out.extend(self.merge(node, locals, id));
            }
            return out;
        }
        let declared: Vec<(Name<'a>, std::borrow::Cow<'a, str>)> =
            (0..shared.len()).filter(|slot| shared[*slot]).map(|slot| (locals.leaves[slot], "number".into())).collect();
        let mut out: Block<'a> = before.into_iter().collect();
        if !declared.is_empty() {
            out.push(Stmt::Declare(declared));
        }
        *self.shared.borrow_mut() = shared;
        for (index, range) in chunks.into_iter().enumerate() {
            let mut body = self.stores_where(node, locals, &|slot| owner[slot] == index);
            body.extend(self.merge_range(node, locals, id, range));
            out.push(Stmt::Do(body));
        }
        self.shared.borrow_mut().clear();
        out.push(increase(self.cursor(), self.e.b.number(bits)));
        out
    }

    fn merge(&self, node: NodeId, locals: &Locals<'a>, id: Option<u32>) -> Block<'a> {
        let words = self.e.layout.node(node).words();
        let body = self.merge_range(node, locals, id, 0..words);
        vec![Stmt::Do(body), increase(self.cursor(), self.e.b.number(self.e.layout.node(node).bits))]
    }

    fn merge_range(&self, node: NodeId, locals: &Locals<'a>, id: Option<u32>, range: Range<u32>) -> Block<'a> {
        let b = self.e.b;
        let bits = self.e.layout.node(node).bits;
        let words = self.e.layout.node(node).words();
        let byte = || b.fixed("Byte");
        let at_byte = |offset: u32| if offset == 0 { byte() } else { b.add(byte(), b.number(offset)) };
        let scaled = |at: u32| Name::Temp('M', at % SCALED);
        let carry = |at: u32| b.invoke(FLOOR, [b.mul(b.name(scaled(at)), b.number(CARRY))]);
        let values = self.words(node, locals, id);

        let mut body: Block<'a> = vec![local(Name::Fixed("Byte"), b.floor_div(self.cursor(), b.number(8)))];
        if self.aligned.get() {
            for (at, value) in (0..words).zip(values).filter(|(at, _)| range.contains(at)) {
                let write = match bits - at * 32 {
                    8 => WRITEU8,
                    16 => WRITEU16,
                    _ => WRITEU32,
                };
                body.push(Stmt::Call(b.invoke(write, [self.stream(), at_byte(at * 4), value])));
            }
            return body;
        }
        let shift = b.invoke(BAND, [self.cursor(), b.number(7)]);
        body.insert(0, local(Name::Fixed("P"), b.invoke(LSHIFT, [b.number(1), shift])));
        let declared = Cell::new([false; SCALED as usize]);
        let hold = |at: u32| {
            let mut seen = declared.get();
            let first = !seen[(at % SCALED) as usize];
            seen[(at % SCALED) as usize] = true;
            declared.set(seen);
            let value = b.mul(values[at as usize], b.fixed("P"));
            if first { local(scaled(at), value) } else { assign(b.name(scaled(at)), value) }
        };
        let (start, end) = (range.start, range.end);
        let top = end - 1;
        body.push(hold(top));
        if top > start {
            body.push(hold(top - 1));
        }
        if end < words || bits + 7 > words * 32 {
            body.push(Stmt::Call(b.invoke(WRITEU8, [self.stream(), at_byte(end * 4), carry(top)])));
        }
        for at in (start + 1..end).rev() {
            if at < top {
                body.push(hold(at - 1));
            }
            let value = b.add(b.name(scaled(at)), carry(at - 1));
            body.push(Stmt::Call(b.invoke(WRITEU32, [self.stream(), at_byte(at * 4), value])));
        }
        let bottom = at_byte(start * 4);
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), bottom]), b.sub(b.fixed("P"), b.number(1))]);
        body.push(Stmt::Call(
            b.invoke(WRITEU32, [self.stream(), at_byte(start * 4), b.add(b.name(scaled(start)), kept)]),
        ));
        body
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
                    let iterable = vec![b.fixed("next"), b.cast(e.read(*path), "any")];
                    if self.packable(&[*key, *value]) {
                        let total = locals.count(e, index, count);
                        let entries = [(*key, "[key]."), (*value, "[value].")];
                        out.extend(self.packed_bytes(*path, &entries, total, |body| {
                            Stmt::ForIn(vec![held_key, held_value], iterable.clone(), body)
                        }));
                        continue;
                    }
                    let mut body = self.within(*path, "[key].", || self.node_body(*key, true));
                    body.extend(self.within(*path, "[value].", || self.node_body(*value, true)));
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
            out.extend(if self.aligned.get() {
                self.bool_bytes(e.read(path), total)
            } else {
                self.bool_array(e.read(path), total)
            });
            return out;
        }
        if let Some(writes) = self.whole_writes(element) {
            if let [write @ (WRITEU8 | WRITEU16)] = writes[..] {
                let array = e.read(path);
                out.extend(self.grouped_array(element, write, array, total, item, position, body));
                return out;
            }
            out.extend(self.whole_array(element, &writes, total, position, body));
            return out;
        }
        if flat && element_node.bits > 0 && element_node.bits <= PACKED_BITS && !self.aligned.get() {
            out.extend(self.packed_array(element, total, position, body));
            return out;
        }
        if !flat && self.packable(&[element]) {
            out.extend(self.packed_bytes(path, &[(element, "")], total, |items| {
                let mut each = body;
                each.extend(items);
                Stmt::For(position, b.number(1), total, each)
            }));
            return out;
        }
        body.extend(self.node_body(element, !flat));
        out.push(Stmt::For(position, b.number(1), total, body));
        out
    }

    fn packable(&self, nodes: &[NodeId]) -> bool {
        let layout = self.e.layout;
        let bits: u32 = nodes.iter().map(|node| layout.node(*node).bits).sum();
        bits <= PACKED_BITS
            && nodes.iter().all(|node| {
                layout.groups(*node).is_empty()
                    && layout.tails(*node).iter().all(|tail| match tail {
                        Tail::Bytes { count, .. } => !matches!(count, Count::Coded { .. }),
                        _ => false,
                    })
            })
    }

    fn packed_bytes(
        &self,
        path: PathId,
        nodes: &[(NodeId, &str)],
        total: ExprId,
        iterate: impl FnOnce(Block<'a>) -> Stmt<'a>,
    ) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let bits: u32 = nodes.iter().map(|(node, _)| e.layout.node(*node).bits).sum();
        let end = e.temp('H');
        let names = [e.temp('B'), e.temp('F'), e.temp('G'), e.temp('Q')];
        let [byte, fill, scale, acc] = names;
        let again = || vec![Stmt::Return(vec![self.again])];
        let mut item = Vec::new();
        let mut terms = Vec::new();
        let mut shift = 0;
        for &(node, step) in nodes {
            let node_bits = e.layout.node(node).bits;
            item.extend(self.within(path, step, || {
                e.scope(e.layout.node(node).base, || {
                    let locals = Locals::new(e, node);
                    let mut out = self.early(node, &locals);
                    out.extend(self.counts(node, &locals));
                    out.extend(self.stores(node, &locals));
                    if node_bits > 0 {
                        let held = e.temp('W');
                        out.push(number(held, self.words(node, &locals, None).swap_remove(0)));
                        let weight = (1u64 << shift) as f64;
                        terms.push(if shift == 0 { b.name(held) } else { b.mul(b.name(held), b.number(weight)) });
                    }
                    for (index, tail) in e.layout.tails(node).iter().enumerate() {
                        let Tail::Bytes { path, count, buffer } = tail else { continue };
                        if matches!(count, Count::Fixed(0)) {
                            continue;
                        }
                        let length = locals.count(e, index, count);
                        let lower = vec![
                            decrease(b.fixed("Top"), length),
                            assign(b.fixed("Limit"), b.sub(b.mul(b.fixed("Top"), b.number(8)), b.fixed("GAP"))),
                        ];
                        match count {
                            Count::Fixed(_) => out.extend(lower),
                            _ => out.push(when(b.gt(length, b.number(0)), lower)),
                        }
                        out.push(when(b.gt(b.name(end), b.fixed("Limit")), again()));
                        let copy = if *buffer {
                            b.invoke(COPY, [self.stream(), b.fixed("Top"), e.read(*path), b.number(0), length])
                        } else {
                            b.invoke(WRITESTRING, [self.stream(), b.fixed("Top"), e.read(*path), length])
                        };
                        out.push(Stmt::Call(copy));
                    }
                    out
                })
            }));
            shift += node_bits;
        }
        let mut out = vec![
            local(end, b.add(self.cursor(), b.mul(total, b.number(bits)))),
            when(b.gt(b.name(end), b.fixed("Limit")), again()),
        ];
        if bits == 0 {
            out.push(iterate(item));
            return out;
        }
        item.extend(self.accumulate(names, b.sum(terms), b.number((1u64 << bits) as f64), b.number(bits)));
        let kept = b.invoke(BAND, [b.invoke(READU8, [self.stream(), b.name(byte)]), b.sub(b.name(scale), b.number(1))]);
        out.push(Stmt::Do(vec![
            local(byte, b.floor_div(self.cursor(), b.number(8))),
            local(fill, b.invoke(BAND, [self.cursor(), b.number(7)])),
            local(scale, b.invoke(LSHIFT, [b.number(1), b.name(fill)])),
            local(acc, kept),
            iterate(item),
            Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.name(acc)])),
        ]));
        out.push(assign(self.cursor(), b.name(end)));
        out
    }

    fn whole_writes(&self, element: NodeId) -> Option<Vec<&'static str>> {
        let layout = self.e.layout;
        if !self.aligned.get() || !layout.tails(element).is_empty() || !layout.groups(element).is_empty() {
            return None;
        }
        let leaves = layout.leaves(element);
        let writes = leaves.iter().map(direct_write).collect::<Option<Vec<_>>>()?;
        let floats = leaves.iter().any(|leaf| matches!(leaf.kind, LeafKind::Value(Codec::F32)));
        (leaves.len() == 1 || floats).then_some(writes)
    }

    fn whole_array(
        &self,
        element: NodeId,
        writes: &[&'static str],
        total: ExprId,
        position: Name<'a>,
        prefix: Block<'a>,
    ) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let byte = e.temp('B');
        let mut body = prefix;
        body.extend(self.whole_item(element, writes, byte));
        vec![
            Stmt::Do(vec![
                local(byte, b.floor_div(self.cursor(), b.number(8))),
                Stmt::For(position, b.number(1), total, body),
            ]),
            increase(self.cursor(), b.mul(total, b.number(e.layout.node(element).bits))),
        ]
    }

    fn whole_item(&self, element: NodeId, writes: &[&'static str], byte: Name<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let bytes = e.layout.node(element).bits / 8;
        e.scope(e.layout.node(element).base, || {
            let locals = Locals::new(e, element);
            let mut out = self.early(element, &locals);
            self.raw_floats.set(true);
            out.extend(self.stores(element, &locals));
            self.raw_floats.set(false);
            let leaves = e.layout.leaves(element);
            let mut order: Vec<usize> = (0..leaves.len()).collect();
            order.sort_by_key(|slot| std::cmp::Reverse(leaves[*slot].position));
            for slot in order {
                let offset = leaves[slot].position / 8;
                let at = if offset == 0 { b.name(byte) } else { b.add(b.name(byte), b.number(offset)) };
                out.push(Stmt::Call(b.invoke(writes[slot], [self.stream(), at, b.name(locals.leaves[slot])])));
            }
            out.push(increase(b.name(byte), b.number(bytes)));
            out
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn grouped_array(
        &self,
        element: NodeId,
        write: &'static str,
        array: ExprId,
        total: ExprId,
        item: Name<'a>,
        position: Name<'a>,
        prefix: Block<'a>,
    ) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let base = e.layout.node(element).base;
        let bits = e.layout.node(element).bits;
        let factor = 32 / bits;
        let (byte, step, end) = (e.temp('B'), e.temp('P'), e.temp('E'));
        let mut group = vec![local(end, b.mul(b.name(step), b.number(factor)))];
        let mut terms = Vec::new();
        for at in 0..factor {
            let held = e.temp('X');
            e.bind(base, Base::Local(held));
            let back = f64::from(factor - 1 - at);
            let index = if back == 0.0 { b.name(end) } else { b.minus(b.name(end), back) };
            group.push(local(held, b.index(array, index)));
            let mut value = None;
            group.extend(e.scope(base, || {
                let locals = Locals::new(e, element);
                let mut out = self.early(element, &locals);
                out.extend(self.stores(element, &locals));
                value = Some(locals.leaves[0]);
                out
            }));
            let value = b.name(value.expect("a single leaf"));
            let weight = 1u64 << (at * bits);
            terms.push(if weight == 1 { value } else { b.mul(value, b.hex(weight)) });
        }
        group.push(Stmt::Call(b.invoke(WRITEU32, [self.stream(), b.name(byte), b.sum(terms)])));
        group.push(increase(b.name(byte), b.number(4)));
        e.bind(base, Base::Local(item));
        let mut single = prefix;
        single.extend(self.whole_item(element, &[write], byte));
        let groups = b.floor_div(total, b.number(factor));
        let done = b.mul(groups, b.number(factor));
        vec![
            Stmt::Do(vec![
                local(byte, b.floor_div(self.cursor(), b.number(8))),
                Stmt::For(step, b.number(1), groups, group),
                Stmt::For(position, b.add(done, b.number(1)), total, single),
            ]),
            increase(self.cursor(), b.mul(total, b.number(bits))),
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
        let (step, value, position) = (e.temp('P'), e.temp('V'), e.temp('I'));
        let bit = |index: ExprId, weight: u64| b.if_else(b.index(array, index), b.hex(weight), b.number(0));
        let mut adds = self.bool_group(array, step, value);
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

    fn bool_bytes(&self, array: ExprId, total: ExprId) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let (byte, step, value) = (e.temp('B'), e.temp('P'), e.temp('V'));
        let (position, rest, scale) = (e.temp('I'), e.temp('Q'), e.temp('G'));
        let groups = b.floor_div(total, b.number(BOOLS_PER_ADD));
        let done = b.mul(groups, b.number(BOOLS_PER_ADD));
        let mut each = self.bool_group(array, step, value);
        each.push(Stmt::Call(b.invoke(WRITEU8, [self.stream(), b.add(b.name(byte), b.name(step)), b.name(value)])));
        let single = vec![
            increase(b.name(rest), b.if_else(b.index(array, b.name(position)), b.name(scale), b.number(0))),
            increase(b.name(scale), b.name(scale)),
        ];
        vec![
            Stmt::Do(vec![
                local(byte, b.minus(b.floor_div(self.cursor(), b.number(8)), 1.0)),
                Stmt::For(step, b.number(1), groups, each),
                local(rest, b.number(0)),
                local(scale, b.number(1)),
                Stmt::For(position, b.add(done, b.number(1)), total, single),
                Stmt::Call(
                    b.invoke(WRITEU8, [self.stream(), b.add(b.name(byte), b.add(groups, b.number(1))), b.name(rest)]),
                ),
            ]),
            increase(self.cursor(), total),
        ]
    }

    fn bool_group(&self, array: ExprId, step: Name<'a>, value: Name<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let end = e.temp('E');
        let bools: Vec<Name<'a>> = (0..BOOLS_PER_ADD).map(|_| e.temp('A')).collect();
        let mut out = vec![local(end, b.mul(b.name(step), b.number(BOOLS_PER_ADD)))];
        for (at, held) in bools.iter().enumerate() {
            let back = f64::from(BOOLS_PER_ADD - 1) - at as f64;
            let index = if back == 0.0 { b.name(end) } else { b.minus(b.name(end), back) };
            out.push(local(*held, b.index(array, index)));
        }
        out.push(number(value, b.number(0)));
        for (at, held) in bools.iter().enumerate() {
            out.push(when(b.name(*held), vec![increase(b.name(value), b.hex(1 << at))]));
        }
        out
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
            out.extend(self.head(node, &locals, None, None));
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
        if event.aligned {
            body.extend(self.head(node, &locals, None, Some(self.id_merge(event.code))));
        } else {
            body.extend(self.head(node, &locals, Some(event.code), None));
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

fn direct_write(leaf: &Leaf) -> Option<&'static str> {
    if !leaf.is_placed() || leaf.is_flag() || !leaf.position.is_multiple_of(8) {
        return None;
    }
    match (leaf.kind, leaf.bits) {
        (LeafKind::Value(Codec::F32), _) => Some(WRITEF32),
        (LeafKind::Value(Codec::F64 { .. }), _) => None,
        (_, 8) => Some(WRITEU8),
        (_, 16) => Some(WRITEU16),
        (_, 32) => Some(WRITEU32),
        _ => None,
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
    let writer = Writer {
        e,
        again,
        event: &declared.name,
        trail: RefCell::new(Vec::new()),
        aligned: Cell::new(false),
        raw_floats: Cell::new(false),
        shared: RefCell::new(Vec::new()),
    };
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
