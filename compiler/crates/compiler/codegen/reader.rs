//! Readers. Turns frames back into handler calls and checks every value on the way, so a bad
//! frame never reaches a handler.

use std::borrow::Cow;
use std::cell::Cell;

use super::emitter::Emitter;
use super::module::Mode;
use super::types::enum_table;
use super::writer::{chunk_bits, chunk_table, count_max};
use crate::layout::{
    Chunk, Codec, Count, Direction, EventLayout, Layout, Leaf, LeafKind, LeafSlot, NodeId, Shape, ShapeId, Tail, mask,
};
use crate::luau::builtins::*;
use crate::luau::{BinOp, Block, Entry, ExprId, Name, Parameter, Stmt, assign, decrease, increase, local, when};
use crate::schema::{EnumId, Role, Side, UNION_TAG};

const MAX_INSTANCES: u32 = 1024;

const MAX_UNROLLED: u32 = 8;

const MAX_UNROLLED_PER_READER: u32 = 32;

struct Locals<'a> {
    leaves: Vec<Name<'a>>,
    tails: Vec<Name<'a>>,
}

pub struct Reader<'f, 'e, 'a> {
    e: &'f Emitter<'e, 'a>,
    from: Side,
    mode: Mode,
    spill: bool,
    unrolled: Cell<u32>,
    aligned: Cell<bool>,
    head_end: Cell<Option<Name<'a>>>,
    item_byte: Cell<Option<ExprId>>,
}

impl<'f, 'e, 'a> Reader<'f, 'e, 'a> {
    pub fn new(e: &'f Emitter<'e, 'a>, from: Side, mode: Mode, spill: bool) -> Reader<'f, 'e, 'a> {
        Reader {
            e,
            from,
            mode,
            spill,
            unrolled: Cell::new(0),
            aligned: Cell::new(false),
            head_end: Cell::new(None),
            item_byte: Cell::new(None),
        }
    }

    fn cursor(&self) -> ExprId {
        self.e.fixed("Cursor")
    }

    fn source(&self) -> ExprId {
        self.e.fixed("Source")
    }

    fn bit_limit(&self) -> ExprId {
        let b = self.e.b;
        b.mul(b.fixed("ByteCursor"), b.number(8))
    }

    fn refuse(&self) -> Stmt<'a> {
        let b = self.e.b;
        Stmt::Return(vec![b.number(-1), b.number(0), b.number(0)])
    }

    fn non_finite(&self, leaf: &Leaf, bits: ExprId) -> Option<ExprId> {
        let exponent = match leaf.kind {
            LeafKind::Value(Codec::F32) => 0x7F80_0000,
            LeafKind::Value(Codec::F64 { high: true }) => 0x7FF0_0000,
            _ => return None,
        };
        let b = self.e.b;
        Some(b.eq(b.invoke(BAND, [bits, b.hex(exponent)]), b.hex(exponent)))
    }

    fn refuse_in_loop(&self) -> Block<'a> {
        vec![assign(self.e.fixed("Valid"), self.e.b.boolean(false)), Stmt::Break]
    }

    fn word(&self, at: u32) -> ExprId {
        self.e.b.name(Name::Temp('W', at))
    }

    fn enum_table(&self, id: EnumId) -> ExprId {
        self.e.b.name(self.e.b.symbol(enum_table(self.e.schema.enumeration(id))))
    }

    fn direct(&self, leaf: &Leaf) -> Option<&'static str> {
        if !self.aligned.get() || !leaf.is_placed() || leaf.is_flag() || !leaf.position.is_multiple_of(8) {
            return None;
        }
        match leaf.bits {
            8 => Some(READU8),
            16 => Some(READU16),
            32 => Some(READU32),
            _ => None,
        }
    }

    fn fixed_head(&self, node: NodeId) -> bool {
        self.e.layout.tails(node).iter().all(|tail| match tail {
            Tail::Bytes { count, .. } => !matches!(count, Count::Coded { .. }),
            _ => false,
        })
    }

    fn hoist(&self, nodes: &[NodeId], total: ExprId, out: &mut Block<'a>) -> bool {
        let (e, b) = (self.e, self.e.b);
        if !nodes.iter().all(|node| self.fixed_head(*node)) {
            return false;
        }
        let bits: u32 = nodes.iter().map(|node| e.layout.node(*node).bits).sum();
        let end = e.temp('H');
        let past = b.add(self.cursor(), b.add(b.mul(total, b.number(bits)), b.number(7)));
        out.push(local(end, b.invoke(RSHIFT, [past, b.number(3)])));
        self.head_end.set(Some(end));
        true
    }

    fn used_words(&self, node: NodeId) -> Vec<bool> {
        let mut used = vec![false; self.e.layout.node(node).words() as usize];
        let words = self.e.layout.leaves(node).iter().filter(|leaf| leaf.is_placed() && self.direct(leaf).is_none());
        for leaf in words {
            let word = (leaf.position / 32) as usize;
            used[word] = true;
            if leaf.position % 32 + leaf.bits > 32 {
                used[word + 1] = true;
            }
        }
        used
    }

    fn high_parts(&self, node: NodeId) -> Vec<bool> {
        let mut high = vec![false; self.e.layout.node(node).words() as usize];
        if self.aligned.get() {
            return high;
        }
        for leaf in self.e.layout.leaves(node).iter().filter(|leaf| leaf.is_placed()) {
            let end = leaf.position + leaf.bits;
            for word in leaf.position / 32..end.div_ceil(32) {
                if end.min(32 * (word + 1)) - 32 * word > 25 {
                    high[word as usize] = true;
                }
            }
        }
        high
    }

    fn rebuild(&self, node: NodeId, first_word_read: bool) -> Block<'a> {
        let b = self.e.b;
        let used = self.used_words(node);
        if self.aligned.get() {
            let direct = self.e.layout.leaves(node).iter().any(|leaf| self.direct(leaf).is_some());
            if !direct && !used.iter().any(|&is_used| is_used) {
                return Vec::new();
            }
            let byte = self.item_byte.get().unwrap_or_else(|| b.floor_div(self.cursor(), b.number(8)));
            let mut out = vec![local(Name::Fixed("Byte"), byte)];
            for (at, &is_used) in used.iter().enumerate() {
                if is_used {
                    let offset =
                        if at == 0 { b.fixed("Byte") } else { b.add(b.fixed("Byte"), b.number(at as u32 * 4)) };
                    out.push(local(Name::Temp('W', at as u32), b.invoke(READU32, [self.source(), offset])));
                }
            }
            return out;
        }
        if !used.iter().any(|&is_used| is_used) {
            return Vec::new();
        }
        let high = self.high_parts(node);
        let words = self.e.layout.node(node).words();
        let raw = |at: u32| b.name(Name::Temp('A', at));
        let mut out = Vec::new();
        if !first_word_read {
            out.push(local(Name::Fixed("Byte"), b.floor_div(self.cursor(), b.number(8))));
            out.push(local(Name::Fixed("Shift"), b.invoke(BAND, [self.cursor(), b.number(7)])));
            out.push(local(Name::Temp('A', 0), b.invoke(READU32, [self.source(), b.fixed("Byte")])));
        }
        if high.iter().any(|&needed| needed) {
            out.push(local(Name::Fixed("Back"), b.sub(b.number(31), b.fixed("Shift"))));
        }
        for at in 1..=words {
            if high[at as usize - 1] || (at < words && used[at as usize]) {
                let read = if at == words { READU8 } else { READU32 };
                let offset = b.add(b.fixed("Byte"), b.number(at * 4));
                out.push(local(Name::Temp('A', at), b.invoke(read, [self.source(), offset])));
            }
        }
        for at in 0..words {
            if !used[at as usize] {
                continue;
            }
            let low = b.invoke(RSHIFT, [raw(at), b.fixed("Shift")]);
            let value = if high[at as usize] {
                let shifted = b.invoke(LSHIFT, [raw(at + 1), b.fixed("Back")]);
                b.invoke(BOR, [low, b.invoke(LSHIFT, [shifted, b.number(1)])])
            } else {
                low
            };
            out.push(local(Name::Temp('W', at), value));
        }
        out
    }

    fn extract(&self, leaf: &Leaf) -> ExprId {
        let b = self.e.b;
        if let Some(read) = self.direct(leaf) {
            let byte = leaf.position / 8;
            let offset = if byte == 0 { b.fixed("Byte") } else { b.add(b.fixed("Byte"), b.number(byte)) };
            return b.invoke(read, [self.source(), offset]);
        }
        let at = leaf.position / 32;
        let offset = leaf.position % 32;
        if offset + leaf.bits <= 32 {
            if offset == 0 && leaf.bits == 32 {
                self.word(at)
            } else if offset == 0 {
                b.invoke(BAND, [self.word(at), b.hex(mask(leaf.bits))])
            } else if offset + leaf.bits == 32 {
                b.invoke(RSHIFT, [self.word(at), b.number(offset)])
            } else {
                b.invoke(EXTRACT, [self.word(at), b.number(offset), b.number(leaf.bits)])
            }
        } else {
            let low_bits = 32 - offset;
            let high = b.invoke(BAND, [self.word(at + 1), b.hex(mask(leaf.bits - low_bits))]);
            let low = b.invoke(RSHIFT, [self.word(at), b.number(offset)]);
            b.invoke(BOR, [low, b.invoke(LSHIFT, [high, b.number(low_bits)])])
        }
    }

    fn flag_value(&self, leaf: &Leaf) -> ExprId {
        let b = self.e.b;
        b.invoke(BTEST, [self.word(leaf.position / 32), b.hex(1 << (leaf.position % 32))])
    }

    fn leaf_type(&self, leaf: &Leaf) -> Cow<'a, str> {
        match leaf.kind {
            LeafKind::Presence | LeafKind::Value(Codec::Bool) => "boolean".into(),
            LeafKind::Value(Codec::Enum { id, optional }) => {
                let name: &'a str = &self.e.schema.enumeration(id).name;
                if optional { format!("{name}?").into() } else { name.into() }
            }
            _ => "number".into(),
        }
    }

    fn leaf_value(&self, leaf: &Leaf, stored: ExprId) -> ExprId {
        let b = self.e.b;
        match leaf.kind {
            LeafKind::Count { min } => b.minus(stored, -f64::from(min)),
            LeafKind::Value(Codec::Int { min, .. }) => b.minus(stored, -(min as f64)),
            LeafKind::Value(Codec::Float { min, step, .. }) => b.minus(b.mul(stored, b.number(step)), -min),
            LeafKind::Value(Codec::Enum { id, optional: false }) => {
                b.index(self.enum_table(id), b.add(stored, b.number(1)))
            }
            LeafKind::Value(Codec::Enum { id, optional: true }) => {
                let value = b.if_else(b.eq(stored, b.number(0)), b.nil(), b.index(self.enum_table(id), stored));
                b.cast(value, format!("{}?", self.e.schema.enumeration(id).name))
            }
            LeafKind::Value(Codec::F32) => b.invoke("BitsF32", [stored]),
            LeafKind::Tag { .. }
            | LeafKind::Rotation { .. }
            | LeafKind::Group { .. }
            | LeafKind::Value(Codec::F64 { high: false }) => stored,
            LeafKind::Value(Codec::F64 { high: true }) => panic!("a double's high word is read with its low word"),
            LeafKind::Presence | LeafKind::Value(Codec::Bool) => panic!("flags are read with flag_value"),
        }
    }

    fn group_reads(
        &self,
        node: NodeId,
        part: impl Fn(LeafSlot) -> ExprId,
        refuse: impl Fn() -> Block<'a>,
    ) -> (Block<'a>, Vec<Option<ExprId>>) {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        let mut digits = vec![None; e.layout.leaves(node).len()];
        for group in e.layout.groups(node) {
            let packed = match group.high {
                None => part(group.low),
                Some(high) => b.add(part(group.low), b.mul(part(high), b.number(4_294_967_296.0))),
            };
            let mut quotient = e.temp('R');
            out.push(Stmt::Local(quotient, Some("number".into()), Some(packed)));
            out.push(when(b.gt(b.name(quotient), b.number((group.product - 1) as f64)), refuse()));
            for (index, member) in group.members.iter().enumerate() {
                let digit = e.layout.leaf(node, *member).digit.expect("a group's member is a digit");
                if index + 1 == group.members.len() {
                    digits[member.0 as usize] = Some(b.name(quotient));
                    break;
                }
                let next = e.temp('Q');
                out.push(local(next, b.floor_div(b.name(quotient), b.number(digit.base as f64))));
                let rest = b.sub(b.name(quotient), b.mul(b.name(next), b.number(digit.base as f64)));
                digits[member.0 as usize] = Some(rest);
                quotient = next;
            }
        }
        (out, digits)
    }

    fn shape_value(&self, shape: ShapeId, leaves: &[ExprId], tails: &[ExprId], tag: Option<&'a str>) -> ExprId {
        let (e, b) = (self.e, self.e.b);
        match e.layout.shape(shape) {
            Shape::Leaf(slot) => leaves[slot.0 as usize],
            Shape::Vector([x, y, z]) => {
                b.invoke(VECTOR3_NEW, [leaves[x.0 as usize], leaves[y.0 as usize], leaves[z.0 as usize]])
            }
            Shape::Rotation { parts, position, steps } => {
                let mut arguments: Vec<ExprId> = match position {
                    Some(axes) => axes.iter().map(|axis| leaves[axis.0 as usize]).collect(),
                    None => vec![b.number(0), b.number(0), b.number(0)],
                };
                let mut unpack: Vec<ExprId> = parts.iter().map(|part| leaves[part.0 as usize]).collect();
                unpack.push(b.number(*steps));
                arguments.push(b.invoke("UnpackRotation", unpack));
                b.invoke(CFRAME_NEW, arguments)
            }
            Shape::Tail(slot) => tails[slot.0 as usize],
            Shape::Struct(fields) => {
                let mut entries = Vec::new();
                if let Some(variant) = tag {
                    entries.push(Entry { key: Some(UNION_TAG.into()), value: b.string(variant) });
                }
                for field in e.layout.fields(fields) {
                    let value = self.shape_value(field.shape, leaves, tails, None);
                    entries.push(Entry { key: Some(field.name.into()), value });
                }
                b.table(entries, false)
            }
        }
    }

    fn field_values(&self, node: NodeId, leaves: &[ExprId], tails: &[ExprId]) -> Vec<ExprId> {
        let layout = self.e.layout;
        let Shape::Struct(fields) = layout.shape(layout.node(node).shape) else {
            panic!("an event's shape is its fields");
        };
        layout.fields(fields).iter().map(|field| self.shape_value(field.shape, leaves, tails, None)).collect()
    }

    fn locals(&self, node: NodeId) -> Locals<'a> {
        let e = self.e;
        Locals {
            leaves: e.layout.leaves(node).iter().map(|_| e.temp('V')).collect(),
            tails: e.layout.tails(node).iter().map(|_| e.temp('T')).collect(),
        }
    }

    fn names(&self, names: &[Name<'a>]) -> Vec<ExprId> {
        names.iter().map(|name| self.e.b.name(*name)).collect()
    }

    fn node_read(&self, node: NodeId, check_head: bool, tag: Option<&'a str>, out: &mut Block<'a>) -> ExprId {
        let locals = self.locals(node);
        self.read_into(node, &locals, check_head, out);
        let shape = self.e.layout.node(node).shape;
        self.shape_value(shape, &self.names(&locals.leaves), &self.names(&locals.tails), tag)
    }

    fn read_into(&self, node: NodeId, locals: &Locals<'a>, check_head: bool, out: &mut Block<'a>) {
        let (e, b) = (self.e, self.e.b);
        let bits = e.layout.node(node).bits;
        let leaves = e.layout.leaves(node);
        if check_head && bits > 0 {
            out.push(when(b.gt(b.add(self.cursor(), b.number(bits)), self.bit_limit()), vec![self.refuse()]));
        }
        if !leaves.is_empty() {
            let declared = leaves.iter().zip(&locals.leaves).map(|(leaf, name)| (*name, self.leaf_type(leaf)));
            out.push(Stmt::Declare(declared.collect()));
            let mut body = self.rebuild(node, false);
            for (slot, (leaf, name)) in leaves.iter().zip(&locals.leaves).enumerate() {
                if matches!(leaf.kind, LeafKind::Value(Codec::F64 { high: true })) {
                    let low = b.name(locals.leaves[slot - 1]);
                    let high = e.temp('S');
                    body.push(local(high, self.extract(leaf)));
                    body.push(when(self.non_finite(leaf, b.name(high)).expect("a high word"), vec![self.refuse()]));
                    body.push(assign(b.name(*name), b.invoke("BitsF64", [low, b.name(high)])));
                } else if leaf.is_placed() {
                    body.extend(self.leaf_assign(leaf, *name));
                }
            }
            let (reads, digits) =
                self.group_reads(node, |slot| b.name(locals.leaves[slot.0 as usize]), || vec![self.refuse()]);
            body.extend(reads);
            for ((leaf, name), digit) in leaves.iter().zip(&locals.leaves).zip(digits) {
                if let Some(digit) = digit {
                    body.push(assign(b.name(*name), self.leaf_value(leaf, digit)));
                }
            }
            out.push(Stmt::Do(body));
        }
        if bits > 0 && self.item_byte.get().is_none() {
            out.push(increase(self.cursor(), b.number(bits)));
        }
        self.tails_read(node, locals, out);
    }

    fn tails_read(&self, node: NodeId, locals: &Locals<'a>, out: &mut Block<'a>) {
        let (e, b) = (self.e, self.e.b);
        for (tail, held) in e.layout.tails(node).iter().zip(&locals.tails) {
            let held = *held;
            match tail {
                Tail::Optional { present, node: inner, .. } => {
                    out.push(local(held, b.nil()));
                    let mut body = Vec::new();
                    let value = self.node_read(*inner, true, None, &mut body);
                    body.push(assign(b.name(held), value));
                    out.push(when(b.name(locals.leaves[present.0 as usize]), body));
                }
                Tail::Map { count, key, value, .. } => {
                    let total = self.count_read(count, locals, out);
                    let entry_bits = e.layout.node(*key).min_bits + e.layout.node(*value).min_bits;
                    let room = b.sub(self.bit_limit(), self.cursor());
                    out.push(when(b.gt(b.mul(total, b.number(entry_bits)), room), vec![self.refuse()]));
                    out.push(local(held, b.cast(b.table([], false), "any")));
                    let outer = self.head_end.get();
                    let hoisted = self.hoist(&[*key, *value], total, out);
                    let mut body = Vec::new();
                    let read_key = self.node_read(*key, !hoisted, None, &mut body);
                    let key_held = e.temp('K');
                    body.push(local(key_held, read_key));
                    let read_value = self.node_read(*value, !hoisted, None, &mut body);
                    self.head_end.set(outer);
                    let target = b.index(b.name(held), b.name(key_held));
                    if self.bare_instance(*value) {
                        body.push(when(read_value, vec![assign(target, read_value)]));
                    } else {
                        body.push(assign(target, read_value));
                    }
                    out.push(Stmt::For(e.temp('I'), b.number(1), total, body));
                }
                Tail::Array { count, element, chunk: Some(chunk), .. } => {
                    let total = self.count_read(count, locals, out);
                    let whole = count_max(count).is_none_or(|max| max >= chunk.size);
                    self.chunked_read(*element, *chunk, whole, total, held, out);
                }
                Tail::Array { count, element, chunk: None, padded, .. } => {
                    let total = self.count_read(count, locals, out);
                    if *padded {
                        let past = b.add(self.cursor(), b.number(7));
                        out.push(assign(self.cursor(), b.invoke(BAND, [past, b.hex(0xFFFF_FFF8)])));
                    }
                    let aligned = self.aligned.replace(*padded);
                    let element_node = e.layout.node(*element);
                    let refused = if element_node.min_bits == 0 {
                        b.gt(total, b.number(MAX_INSTANCES))
                    } else {
                        let room = b.sub(self.bit_limit(), self.cursor());
                        b.gt(b.mul(total, b.number(element_node.min_bits)), room)
                    };
                    out.push(when(refused, vec![self.refuse()]));
                    if reads_bool_bytes(e.layout, count, *element) {
                        self.bool_bytes_read(*element, total, held, out);
                        self.aligned.set(aligned);
                        continue;
                    }
                    let leaves = e.layout.leaves(*element).len().max(1) as u32;
                    if let Count::Fixed(fixed) = *count
                        && fixed > 0
                        && fixed * leaves <= MAX_UNROLLED
                        && self.unrolled.get() + fixed * leaves <= MAX_UNROLLED_PER_READER
                        && e.layout.tails(*element).is_empty()
                        && !self.bare_instance(*element)
                    {
                        self.unrolled.set(self.unrolled.get() + fixed * leaves);
                        let values: Vec<ExprId> =
                            (0..fixed).map(|_| self.node_read(*element, false, None, out)).collect();
                        let entries = values.into_iter().map(|value| Entry { key: None, value });
                        out.push(local(held, b.cast(b.table(entries, false), "any")));
                        self.aligned.set(aligned);
                        continue;
                    }
                    out.push(local(held, b.cast(b.invoke(TABLE_CREATE, [total]), "any")));
                    let position = e.temp('I');
                    let mut body = Vec::new();
                    let has_tails = !e.layout.tails(*element).is_empty();
                    let indexed = *padded && !has_tails && !self.bare_instance(*element);
                    if indexed {
                        let bytes = element_node.bits / 8;
                        let base = e.temp('B');
                        out.push(local(base, b.sub(b.floor_div(self.cursor(), b.number(8)), b.number(bytes))));
                        let step = if bytes == 1 { b.name(position) } else { b.mul(b.name(position), b.number(bytes)) };
                        self.item_byte.set(Some(b.add(b.name(base), step)));
                    }
                    let outer = self.head_end.get();
                    let hoisted = has_tails && self.hoist(&[*element], total, out);
                    let value = self.node_read(*element, has_tails && !hoisted, None, &mut body);
                    self.head_end.set(outer);
                    if self.bare_instance(*element) {
                        let kept = e.temp('K');
                        out.push(local(kept, b.number(0)));
                        body.push(when(
                            value,
                            vec![
                                increase(b.name(kept), b.number(1)),
                                assign(b.index(b.name(held), b.name(kept)), value),
                            ],
                        ));
                    } else {
                        body.push(assign(b.index(b.name(held), b.name(position)), value));
                    }
                    out.push(Stmt::For(position, b.number(1), total, body));
                    if indexed {
                        self.item_byte.set(None);
                        out.push(increase(self.cursor(), b.mul(total, b.number(element_node.bits))));
                    }
                    self.aligned.set(aligned);
                }
                Tail::Bytes { count, buffer, .. } => {
                    let total = self.count_read(count, locals, out);
                    let used = match self.head_end.get() {
                        Some(end) => b.name(end),
                        None => b.invoke(RSHIFT, [b.add(self.cursor(), b.number(7)), b.number(3)]),
                    };
                    let room = b.sub(b.fixed("ByteCursor"), used);
                    out.push(when(b.gt(total, room), vec![self.refuse()]));
                    out.push(decrease(b.fixed("ByteCursor"), total));
                    if *buffer {
                        out.push(local(held, b.invoke(CREATE, [total])));
                        let copy =
                            b.invoke(COPY, [b.name(held), b.number(0), self.source(), b.fixed("ByteCursor"), total]);
                        out.push(Stmt::Call(copy));
                    } else {
                        out.push(local(held, b.invoke(READSTRING, [self.source(), b.fixed("ByteCursor"), total])));
                    }
                }
                Tail::Instance { class, handle, .. } => self.instance_read(*class, *handle, held, out),
                Tail::Union { union, tag, variants, optional, .. } => {
                    out.push(local(held, b.cast(b.nil(), "any")));
                    let tag = locals.leaves[tag.0 as usize];
                    let first = u32::from(*optional);
                    let declared = e.schema.union(*union);
                    let branches = e
                        .layout
                        .variant_nodes(variants)
                        .iter()
                        .zip(&declared.variants)
                        .enumerate()
                        .map(|(position, (variant, declared))| {
                            let mut body = Vec::new();
                            let value = self.node_read(*variant, true, Some(&declared.name), &mut body);
                            body.push(assign(b.name(held), value));
                            (b.eq(b.name(tag), b.number(first + position as u32)), body)
                        })
                        .collect();
                    out.push(Stmt::If(branches, None));
                }
            }
        }
    }

    fn chunked_read(
        &self,
        element: NodeId,
        chunk: Chunk,
        whole: bool,
        total: ExprId,
        held: Name<'a>,
        out: &mut Block<'a>,
    ) {
        let (e, b) = (self.e, self.e.b);
        let room = b.sub(self.bit_limit(), self.cursor());
        let table = b.name(b.symbol(chunk_table(chunk)));
        let needed = if whole { chunk_bits(e, chunk, total) } else { b.index(table, b.add(total, b.number(1))) };
        let width = e.temp('W');
        out.push(Stmt::Local(width, Some("number".into()), Some(needed)));
        out.push(when(b.gt(b.name(width), room), vec![self.refuse()]));
        out.push(local(held, b.cast(b.invoke(TABLE_CREATE, [total]), "any")));
        let leaf = &e.layout.leaves(element)[0];
        let shape = e.layout.node(element).shape;
        let (filled, step, packed, part) = (e.temp('I'), e.temp('J'), e.temp('R'), e.temp('Q'));
        let window = || {
            let word = b.invoke(READU32, [self.source(), b.floor_div(self.cursor(), b.number(8))]);
            b.invoke(RSHIFT, [word, b.invoke(BAND, [self.cursor(), b.number(7)])])
        };
        let split = |count: ExprId| {
            let digit = b.sub(b.name(packed), b.mul(b.name(part), b.number(chunk.base as f64)));
            let value = self.shape_value(shape, &[self.leaf_value(leaf, digit)], &[], None);
            let target = b.index(b.name(held), b.add(b.name(filled), b.name(step)));
            vec![
                Stmt::For(
                    step,
                    b.number(1),
                    count,
                    vec![
                        local(part, b.floor_div(b.name(packed), b.number(chunk.base as f64))),
                        assign(target, value),
                        assign(b.name(packed), b.name(part)),
                    ],
                ),
                when(b.ne(b.name(packed), b.number(0)), vec![self.refuse()]),
            ]
        };
        let chunks = b.floor_div(total, b.number(chunk.size));
        let mut each = vec![
            Stmt::Local(packed, Some("number".into()), Some(b.invoke(BAND, [window(), b.hex(mask(chunk.bits))]))),
            increase(self.cursor(), b.number(chunk.bits)),
        ];
        each.extend(split(b.number(chunk.size)));
        each.push(increase(b.name(filled), b.number(chunk.size)));
        out.push(local(filled, b.number(0)));
        if whole {
            out.push(Stmt::For(e.temp('C'), b.number(1), chunks, each));
        }
        let (rest, width) = (e.temp('N'), e.temp('W'));
        let mut last = vec![
            Stmt::Local(width, Some("number".into()), Some(b.index(table, b.add(b.name(rest), b.number(1))))),
            Stmt::Local(
                packed,
                Some("number".into()),
                Some(b.invoke(BAND, [window(), b.sub(b.invoke(LSHIFT, [b.number(1), b.name(width)]), b.number(1))])),
            ),
            increase(self.cursor(), b.name(width)),
        ];
        last.extend(split(b.name(rest)));
        let left = if whole {
            b.sub(total, b.mul(b.floor_div(total, b.number(chunk.size)), b.number(chunk.size)))
        } else {
            total
        };
        out.push(local(rest, left));
        out.push(when(b.gt(b.name(rest), b.number(0)), last));
    }

    fn bool_bytes_read(&self, element: NodeId, total: ExprId, held: Name<'a>, out: &mut Block<'a>) {
        let (e, b) = (self.e, self.e.b);
        out.push(local(held, b.cast(b.invoke(TABLE_CREATE, [total]), "any")));
        let (bytes, base, shift, bits, end, position) =
            (e.temp('N'), e.temp('B'), e.temp('F'), e.temp('R'), e.temp('E'), e.temp('P'));
        out.push(local(bytes, b.floor_div(total, b.number(8))));
        let word = b.invoke(READU32, [self.source(), b.add(b.name(base), b.name(position))]);
        let mut body = vec![
            local(bits, b.invoke(RSHIFT, [word, b.name(shift)])),
            local(end, b.mul(b.name(position), b.number(8))),
        ];
        for bit in 0..8u32 {
            let target = b.index(b.name(held), b.minus(b.name(end), f64::from(7 - bit)));
            body.push(assign(target, b.invoke(BTEST, [b.name(bits), b.hex(1 << bit)])));
        }
        out.push(Stmt::Do(vec![
            local(base, b.sub(b.floor_div(self.cursor(), b.number(8)), b.number(1))),
            local(shift, b.invoke(BAND, [self.cursor(), b.number(7)])),
            Stmt::For(position, b.number(1), b.name(bytes), body),
        ]));
        let whole = b.mul(b.name(bytes), b.number(8));
        out.push(increase(self.cursor(), whole));
        let index = e.temp('I');
        let mut rest = Vec::new();
        let value = self.node_read(element, false, None, &mut rest);
        rest.push(assign(b.index(b.name(held), b.name(index)), value));
        out.push(Stmt::For(index, b.add(whole, b.number(1)), total, rest));
    }

    fn bare_instance(&self, element: NodeId) -> bool {
        let layout = self.e.layout;
        layout.leaves(element).is_empty() && matches!(layout.tails(element), [Tail::Instance { .. }])
    }

    fn instance_read(&self, class: Option<&'a str>, handle: bool, held: Name<'a>, out: &mut Block<'a>) {
        let (e, b) = (self.e, self.e.b);
        let raw = e.temp('S');
        let client = self.from == Side::Client;
        out.push(increase(b.fixed("InstanceCursor"), b.number(1)));
        out.push(local(raw, b.index(b.fixed("Instances"), b.fixed("InstanceCursor"))));
        if client {
            let not_handle = b.ne(b.invoke(TYPEOF, [b.name(raw)]), b.string("InstanceHandle"));
            out.push(when(not_handle, vec![self.refuse()]));
        }
        if handle {
            out.push(Stmt::Local(held, Some("InstanceHandle".into()), Some(b.name(raw))));
            return;
        }
        let resolved = b.method(b.cast(b.name(raw), "any"), "Get", []);
        let value = if client { resolved } else { b.if_else(b.name(raw), resolved, b.nil()) };
        let ty = format!("{}?", class.unwrap_or("Instance"));
        out.push(Stmt::Local(held, Some(ty.into()), Some(value)));
        if let (true, Some(class)) = (client, class) {
            let is_a = b.method(b.name(held), "IsA", [b.string(class)]);
            out.push(when(b.binary(BinOp::And, b.name(held), b.not(is_a)), vec![self.refuse()]));
        }
    }

    fn count_read(&self, count: &Count, locals: &Locals<'a>, out: &mut Block<'a>) -> ExprId {
        let (e, b) = (self.e, self.e.b);
        match *count {
            Count::Fixed(fixed) => b.number(fixed),
            Count::Head { leaf, .. } => b.name(locals.leaves[leaf.0 as usize]),
            Count::Coded { k, max } => {
                let (held, window, zeros, top) = (e.temp('C'), e.temp('H'), e.temp('Z'), e.temp('N'));
                out.push(Stmt::Local(held, Some("number".into()), None));
                let read = b.invoke(READU32, [self.source(), b.floor_div(self.cursor(), b.number(8))]);
                let low = b.invoke(EXTRACT, [b.name(window), b.add(b.name(zeros), b.number(1)), b.name(top)]);
                let value = b.sub(b.add(low, b.invoke(LSHIFT, [b.number(1), b.name(top)])), b.number(1u32 << k));
                let fast = vec![
                    assign(b.name(held), value),
                    increase(self.cursor(), b.add(b.add(b.name(zeros), b.number(1)), b.name(top))),
                ];
                let slow = vec![Stmt::Assign(
                    vec![b.name(held), self.cursor()],
                    vec![b.invoke("ReadCoded", [self.source(), self.cursor(), b.number(k)])],
                )];
                let fits = b.binary(BinOp::Le, b.add(b.name(zeros), b.name(top)), b.number(24));
                out.push(Stmt::Do(vec![
                    local(window, b.invoke(RSHIFT, [read, b.invoke(BAND, [self.cursor(), b.number(7)])])),
                    local(zeros, b.invoke(COUNTRZ, [b.name(window)])),
                    local(top, b.add(b.name(zeros), b.number(k))),
                    Stmt::If(vec![(fits, fast)], Some(slow)),
                ]));
                let negative = b.lt(b.name(held), b.number(0));
                let refused = match max {
                    Some(max) => b.or(negative, b.gt(b.name(held), b.number(max))),
                    None => negative,
                };
                out.push(when(refused, vec![self.refuse()]));
                b.name(held)
            }
        }
    }

    fn deliver(&self, event: &EventLayout, values: Vec<ExprId>, lines: bool) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let name: &'a str = &e.schema.events[event.event].name;
        let handler = || b.field(b.fixed(handlers_table(self.from)), name);
        let record = || b.field(b.fixed("EVENTS"), name);
        let peer = || if self.from == Side::Client { b.fixed("Sender") } else { b.nil() };
        let call_handler = |call: ExprId| {
            let invoke = if lines { Stmt::CallLines(call) } else { Stmt::Call(call) };
            vec![local(Name::Fixed("Handler"), handler()), when(b.fixed("Handler"), vec![invoke])]
        };
        match (self.mode, event.role) {
            (Mode::Codec, _) => {
                let mut arguments = values;
                if self.from == Side::Client {
                    arguments.insert(0, b.fixed("Sender"));
                }
                call_handler(b.call(b.fixed("Handler"), arguments))
            }
            (Mode::Api, Role::Message) => {
                let mut arguments = vec![b.fixed("Handler"), peer()];
                arguments.extend(values);
                call_handler(b.method(record(), "Handle", arguments))
            }
            (Mode::Api, Role::Request(_)) => {
                let mut arguments = vec![handler(), peer()];
                arguments.extend(values);
                vec![Stmt::Call(b.method(record(), "HandleInvoke", arguments))]
            }
            (Mode::Api, Role::Reply(function)) => {
                let [call_id, result] = values[..] else { panic!("a reply is a call id and a union") };
                let mut succeeded = vec![peer(), call_id, b.boolean(true)];
                let results = &e.schema.function(function).results;
                succeeded.extend(results.iter().map(|field| b.field(result, &field.name)));
                let failed = vec![peer(), call_id, b.boolean(false), b.field(result, "Reason")];
                let ok = b.eq(b.field(result, UNION_TAG), b.string("Ok"));
                vec![Stmt::If(
                    vec![(ok, vec![Stmt::Call(b.invoke("COMPLETE", succeeded))])],
                    Some(vec![Stmt::Call(b.invoke("COMPLETE", failed))]),
                )]
            }
        }
    }

    fn sender_type(&self) -> &'static str {
        match self.mode {
            Mode::Codec => "any",
            Mode::Api => "Player",
        }
    }

    fn reader_callee(&self, event: &EventLayout) -> ExprId {
        let b = self.e.b;
        let name: &'a str = &self.e.schema.events[event.event].name;
        if self.spill { b.field(b.fixed("READERS"), name) } else { b.name(reader_name(self.e, event)) }
    }

    fn has_instances(&self, node: NodeId) -> bool {
        self.e.layout.any_tail(node, &|tail| matches!(tail, Tail::Instance { .. }))
    }

    fn event_reader(&self, event: &EventLayout) -> Stmt<'a> {
        let (e, b) = (self.e, self.e.b);
        e.begin_function();
        self.unrolled.set(0);
        let instances = self.has_instances(event.node);
        let mut body = Vec::new();
        let locals = self.locals(event.node);
        self.read_into(event.node, &locals, true, &mut body);
        let values = self.field_values(event.node, &self.names(&locals.leaves), &self.names(&locals.tails));
        body.extend(self.deliver(event, values, false));
        let instance_cursor = if instances { b.fixed("InstanceCursor") } else { b.number(0) };
        body.push(Stmt::Return(vec![self.cursor(), b.fixed("ByteCursor"), instance_cursor]));

        let mut parameters = vec![
            Parameter::new(Name::Fixed("Source"), "buffer"),
            Parameter::new(Name::Fixed("Cursor"), "number"),
            Parameter::new(Name::Fixed("ByteCursor"), "number"),
        ];
        if instances {
            parameters.push(Parameter::new(Name::Fixed("Instances"), "{ any }"));
            parameters.push(Parameter::new(Name::Fixed("InstanceCursor"), "number"));
        }
        if self.from == Side::Client {
            parameters.push(Parameter::new(Name::Fixed("Sender"), self.sender_type()));
        }
        Stmt::Function { name: reader_name(e, event), parameters, returns: "(number, number, number)".into(), body }
    }

    fn leaf_assign(&self, leaf: &Leaf, name: Name<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let target = b.name(name);
        if leaf.is_flag() {
            return vec![assign(target, self.flag_value(leaf))];
        }
        let checked = e.temp('S');
        if let Some(refused) = self.non_finite(leaf, b.name(checked)) {
            return vec![
                local(checked, self.extract(leaf)),
                when(refused, vec![self.refuse()]),
                assign(target, self.leaf_value(leaf, b.name(checked))),
            ];
        }
        match leaf.check_max {
            Some(max) => vec![
                local(checked, self.extract(leaf)),
                when(b.gt(b.name(checked), b.number(max as f64)), vec![self.refuse()]),
                assign(target, self.leaf_value(leaf, b.name(checked))),
            ],
            None => vec![assign(target, self.leaf_value(leaf, self.extract(leaf)))],
        }
    }

    fn fixed_leaf(&self, event: &EventLayout) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let node = event.node;
        let bits = e.layout.node(node).bits;
        let mut out = vec![when(b.gt(b.add(self.cursor(), b.number(bits)), self.bit_limit()), self.refuse_in_loop())];
        out.extend(self.rebuild(node, true));
        let leaves = e.layout.leaves(node);
        let (reads, digits) =
            self.group_reads(node, |slot| self.extract(&leaves[slot.0 as usize]), || self.refuse_in_loop());
        let mut values = Vec::new();
        let mut checks = Vec::new();
        for (leaf, digit) in leaves.iter().zip(digits) {
            if let Some(digit) = digit {
                values.push(self.leaf_value(leaf, digit));
                continue;
            }
            if matches!(leaf.kind, LeafKind::Group { .. }) {
                values.push(b.nil());
                continue;
            }
            if leaf.is_flag() {
                values.push(self.flag_value(leaf));
                continue;
            }
            if matches!(leaf.kind, LeafKind::Value(Codec::F64 { high: true })) {
                let low = *values.last().expect("a double's low word comes first");
                let high = e.temp('V');
                checks.push(local(high, self.extract(leaf)));
                checks.push(when(self.non_finite(leaf, b.name(high)).expect("a high word"), self.refuse_in_loop()));
                values.push(b.invoke("BitsF64", [low, b.name(high)]));
                continue;
            }
            let checked = e.temp('V');
            if let Some(refused) = self.non_finite(leaf, b.name(checked)) {
                checks.push(local(checked, self.extract(leaf)));
                checks.push(when(refused, self.refuse_in_loop()));
                values.push(self.leaf_value(leaf, b.name(checked)));
                continue;
            }
            match leaf.check_max {
                Some(max) => {
                    checks.push(local(checked, self.extract(leaf)));
                    checks.push(when(b.gt(b.name(checked), b.number(max as f64)), self.refuse_in_loop()));
                    values.push(self.leaf_value(leaf, b.name(checked)));
                }
                None => values.push(self.leaf_value(leaf, self.extract(leaf))),
            }
        }
        out.extend(checks);
        out.extend(reads);
        let arguments = self.field_values(node, &values, &[]);
        out.extend(self.deliver(event, arguments, true));
        out.push(increase(self.cursor(), b.number(bits)));
        out
    }

    fn leaf(&self, event: &EventLayout) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        if e.layout.tails(event.node).is_empty() {
            assert!(!event.aligned, "an aligned event holds a list or map, so it has tails");
            return self.fixed_leaf(event);
        }
        let mut out = Vec::new();
        if event.aligned {
            let past = b.add(self.cursor(), b.number(event.code_bits + 7));
            out.push(assign(self.cursor(), b.invoke(BAND, [past, b.hex(0xFFFF_FFF8)])));
            out.push(when(b.gt(self.cursor(), self.bit_limit()), self.refuse_in_loop()));
        }
        let mut arguments = vec![self.source(), self.cursor(), b.fixed("ByteCursor")];
        let mut targets = vec![self.cursor(), b.fixed("ByteCursor")];
        if self.has_instances(event.node) {
            arguments.extend([b.fixed("Instances"), b.fixed("InstanceCursor")]);
            targets.push(b.fixed("InstanceCursor"));
        }
        if self.from == Side::Client {
            arguments.push(b.fixed("Sender"));
        }
        out.push(Stmt::Assign(targets, vec![b.call(self.reader_callee(event), arguments)]));
        out.push(when(b.lt(self.cursor(), b.number(0)), self.refuse_in_loop()));
        out
    }

    fn dispatch(&self, low: usize, high: usize, direction: &Direction) -> Block<'a> {
        let b = self.e.b;
        if high - low < 4 {
            if low == high {
                return self.leaf(&direction.events[low - 1]);
            }
            let branches = (low..high)
                .map(|id| (b.eq(b.fixed("Tag"), b.number(id as f64)), self.leaf(&direction.events[id - 1])))
                .collect();
            return vec![Stmt::If(branches, Some(self.leaf(&direction.events[high - 1])))];
        }
        let middle = (low + high) / 2;
        let lower = b.binary(BinOp::Le, b.fixed("Tag"), b.number(middle as f64));
        vec![Stmt::If(
            vec![(lower, self.dispatch(low, middle, direction))],
            Some(self.dispatch(middle + 1, high, direction)),
        )]
    }

    fn id_table(&self, direction: &Direction) -> Option<(Name<'a>, Stmt<'a>)> {
        let ids = direction.ids;
        if ids.short == 0 {
            return None;
        }
        let b = self.e.b;
        let mut symbols = vec![0u32; 1 << ids.bits];
        for symbol in 0..=direction.events.len() as u32 {
            let (code, bits) = ids.code(symbol);
            let mut window = code;
            while window < 1 << ids.bits {
                symbols[window as usize] = symbol;
                window += 1 << bits;
            }
        }
        let entries: Vec<Entry> =
            symbols.into_iter().map(|symbol| Entry { key: None, value: b.number(symbol) }).collect();
        let name = match direction.from {
            Side::Client => "ON_SERVER_IDS",
            Side::Server => "ON_CLIENT_IDS",
        };
        let table = Stmt::Local(Name::Fixed(name), Some("{ number }".into()), Some(b.table(entries, false)));
        Some((Name::Fixed(name), table))
    }

    fn frame_loop(&self, direction: &Direction) -> Stmt<'a> {
        let b = self.e.b;
        let count = direction.events.len();
        let ids = direction.ids;
        let first = ids.first_bits();
        let window = || b.invoke(RSHIFT, [b.name(Name::Temp('A', 0)), b.fixed("Shift")]);
        let tag = if ids.short > 0 {
            let at = b.add(b.invoke(BAND, [window(), b.hex(mask(ids.bits))]), b.number(1));
            Stmt::Local(Name::Fixed("Tag"), Some("number".into()), Some(b.index(b.fixed("Ids"), at)))
        } else {
            local(Name::Fixed("Tag"), b.invoke(BAND, [window(), b.hex(mask(first))]))
        };
        let mut body = vec![
            local(Name::Fixed("Byte"), b.floor_div(self.cursor(), b.number(8))),
            local(Name::Fixed("Shift"), b.invoke(BAND, [self.cursor(), b.number(7)])),
            local(Name::Temp('A', 0), b.invoke(READU32, [self.source(), b.fixed("Byte")])),
            tag,
        ];
        body.push(when(b.eq(b.fixed("Tag"), b.number(0)), vec![Stmt::Break]));
        if count > 0 {
            body.extend(self.dispatch(1, count, direction));
        }
        let more = b.binary(BinOp::Le, b.add(self.cursor(), b.number(first)), self.bit_limit());
        Stmt::While(more, body)
    }

    pub fn readers(&self, direction: &Direction, into_field: impl Fn(Stmt<'a>, ExprId) -> Stmt<'a>) -> Block<'a> {
        let (e, b) = (self.e, self.e.b);
        let mut out = Vec::new();
        for event in &direction.events {
            if !e.layout.tails(event.node).is_empty() {
                let reader = self.event_reader(event);
                out.push(if self.spill { into_field(reader, self.reader_callee(event)) } else { reader });
                out.push(Stmt::Blank);
            }
        }

        let table = self.id_table(direction);
        if let Some((_, statement)) = &table {
            out.push(statement.clone());
            out.push(Stmt::Blank);
        }
        e.begin_function();
        let padded = b.add(b.fixed("Length"), b.fixed("READ_PAD"));
        let scratch_size = b.invoke(LEN, [self.source()]);
        let grown = b.invoke(CREATE, [b.invoke(MAX, [padded, b.mul(scratch_size, b.number(2))])]);
        let mut body = vec![
            local(Name::Fixed("Length"), b.invoke(LEN, [b.fixed("Frame")])),
            local(Name::Fixed("Source"), b.fixed("READ_SCRATCH")),
            assign(b.fixed("READ_SCRATCH"), b.fixed("EMPTY")),
            when(b.gt(padded, scratch_size), vec![assign(self.source(), grown)]),
            Stmt::Call(b.invoke(COPY, [self.source(), b.number(0), b.fixed("Frame"), b.number(0), b.fixed("Length")])),
            Stmt::Call(b.invoke(FILL, [self.source(), b.fixed("Length"), b.number(0), b.fixed("READ_PAD")])),
            local(Name::Fixed("Cursor"), b.number(0)),
            local(Name::Fixed("ByteCursor"), b.fixed("Length")),
        ];
        if direction.events.iter().any(|event| self.has_instances(event.node)) {
            body.push(local(Name::Fixed("Instances"), b.or(b.fixed("Received"), b.fixed("EMPTY_LIST"))));
            body.push(local(Name::Fixed("InstanceCursor"), b.number(0)));
        }
        if let Some((name, _)) = table {
            body.push(local(Name::Fixed("Ids"), b.name(name)));
        }
        body.extend([
            local(Name::Fixed("Valid"), b.boolean(true)),
            self.frame_loop(direction),
            assign(b.fixed("READ_SCRATCH"), self.source()),
            Stmt::Return(vec![b.fixed("Valid")]),
        ]);

        let mut parameters = vec![Parameter::new(Name::Fixed("Frame"), "buffer")];
        let function = match direction.from {
            Side::Client => {
                parameters.push(Parameter::new(Name::Fixed("Sender"), self.sender_type()));
                "ReadOnServer"
            }
            Side::Server => "ReadOnClient",
        };
        parameters.push(Parameter::new(Name::Fixed("Received"), "{ any }?"));
        out.push(Stmt::Function { name: Name::Fixed(function), parameters, returns: "boolean".into(), body });
        out.push(Stmt::Blank);
        out
    }
}

fn reads_bool_bytes(layout: &Layout, count: &Count, element: NodeId) -> bool {
    layout.is_bare_bool(element) && !matches!(count, Count::Fixed(fixed) if *fixed < 8)
}

pub fn handlers_table(from: Side) -> &'static str {
    match from {
        Side::Client => "SERVER_HANDLERS",
        Side::Server => "CLIENT_HANDLERS",
    }
}

fn reader_name<'a>(e: &Emitter<'_, 'a>, event: &EventLayout) -> Name<'a> {
    e.b.symbol(format!("Read{}", e.schema.events[event.event].name))
}
