//! Works out where every value of every message goes, bit by bit.

use super::numbers::{IdCodes, bits_for, float_steps, mask, rotation_steps};
use super::packing::{Chunk, group_bits, groups};
use super::types::{
    Codec, Count, Digit, Direction, EventLayout, Group, GroupId, Layout, Leaf, LeafKind, LeafSlot, Node, NodeId, Path,
    PathId, Shape, ShapeField, ShapeId, Tail, TailSlot,
};
use crate::error::{Error, LayoutError};
use crate::schema::{Bounds, Field, Schema, Side, Type, TypeId, UnionId};

const BYTES_K: u32 = 3;
const ARRAY_K: u32 = 2;
const SPREAD: u32 = 64;

pub fn layout<'a>(schema: &'a Schema) -> Result<Layout<'a>, Error> {
    let mut builder = Builder {
        schema,
        layout: Layout::default(),
        open: Vec::new(),
        spare: Vec::new(),
        trail: Vec::new(),
        aligned: false,
    };
    for from in Side::BOTH {
        let events: Vec<usize> = (0..schema.events.len()).filter(|index| schema.events[*index].from == from).collect();
        let ids = IdCodes::new(events.len() as u64);
        let mut laid = Vec::with_capacity(events.len());
        for (position, &index) in events.iter().enumerate() {
            let event = &schema.events[index];
            let (code, code_bits) = ids.code(position as u32 + 1);
            let aligned = event.fields.iter().any(|field| spreads(schema, field.ty));
            builder.trail.push(&event.name);
            builder.aligned = aligned;
            let base = builder.path(None, None);
            let offset = if aligned { 0 } else { code_bits };
            let node = builder.node(offset, base, |builder, base| builder.fields(&event.fields, base))?;
            builder.trail.pop();
            laid.push(EventLayout {
                event: index,
                id: position as u32 + 1,
                code,
                code_bits,
                aligned,
                reliable: event.reliable,
                role: event.role,
                node,
            });
        }
        builder.layout.directions.push(Direction { from, ids, events: laid });
    }
    Ok(builder.layout)
}

fn spreads(schema: &Schema, ty: TypeId) -> bool {
    let any = |fields: &[Field]| fields.iter().any(|field| spreads(schema, field.ty));
    match schema.ty(ty) {
        Type::Array { bounds, .. } | Type::Map { bounds, .. } if bounds.max.is_none_or(|max| max > SPREAD) => true,
        Type::Array { element: inner, .. } | Type::Optional(inner) => spreads(schema, *inner),
        Type::Map { key, value, .. } => spreads(schema, *key) || spreads(schema, *value),
        Type::Struct(id) => any(&schema.structure(*id).fields),
        Type::Union(id) => schema.union(*id).variants.iter().any(|variant| any(&variant.fields)),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    pub bits: u32,
    pub min_bits: u32,
    pub variable: bool,
    pub per_item: Option<u32>,
    pub item_varies: bool,
}

pub fn measure(schema: &Schema, ty: TypeId) -> Result<Measure, Error> {
    let mut builder = Builder {
        schema,
        layout: Layout::default(),
        open: Vec::new(),
        spare: Vec::new(),
        trail: Vec::new(),
        aligned: false,
    };
    let base = builder.path(None, None);
    let node = builder.node(0, base, |builder, base| builder.add(ty, base))?;
    let laid = builder.layout.node(node);
    let laid_out = &builder.layout;
    let (per_item, item_varies) = match laid_out.tails(node).first() {
        Some(Tail::Array { element, chunk: None, .. }) => {
            (Some(laid_out.node(*element).min_bits), varies(laid_out, *element))
        }
        Some(Tail::Map { key, value, .. }) => (
            Some(laid_out.node(*key).min_bits + laid_out.node(*value).min_bits),
            varies(laid_out, *key) || varies(laid_out, *value),
        ),
        _ => (None, false),
    };
    Ok(Measure {
        bits: laid.bits,
        min_bits: laid.min_bits,
        variable: varies(&builder.layout, node),
        per_item,
        item_varies,
    })
}

pub fn varies(layout: &Layout, node: NodeId) -> bool {
    layout.tails(node).iter().any(|tail| match tail {
        Tail::Instance { .. } => false,
        Tail::Array { count: Count::Fixed(_), element, padded: false, .. } => varies(layout, *element),
        _ => true,
    })
}

#[derive(Default)]
struct Open<'a> {
    leaves: Vec<Leaf>,
    tails: Vec<Tail<'a>>,
    bits: u32,
    min_bits: u32,
}

struct Builder<'a> {
    schema: &'a Schema<'a>,
    layout: Layout<'a>,
    open: Vec<Open<'a>>,
    spare: Vec<Open<'a>>,
    trail: Vec<&'a str>,
    aligned: bool,
}

impl<'a> Builder<'a> {
    fn path(&mut self, parent: Option<PathId>, field: Option<&'a str>) -> PathId {
        self.layout.paths.push(Path { parent, field });
        PathId(self.layout.paths.len() as u32 - 1)
    }

    fn shape(&mut self, shape: Shape) -> ShapeId {
        self.layout.shapes.push(shape);
        ShapeId(self.layout.shapes.len() as u32 - 1)
    }

    fn fail(&self, problem: LayoutError) -> Error {
        Error::Layout { path: self.trail.join("."), problem }
    }

    fn current(&mut self) -> &mut Open<'a> {
        self.open.last_mut().expect("a node is open while its fields are laid out")
    }

    fn node(
        &mut self,
        offset: u32,
        base: PathId,
        fill: impl FnOnce(&mut Self, PathId) -> Result<ShapeId, Error>,
    ) -> Result<NodeId, Error> {
        let mut open = self.spare.pop().unwrap_or_default();
        open.bits = offset;
        open.min_bits = 0;
        self.open.push(open);
        let shape = fill(self, base)?;
        let mut open = self.open.pop().expect("the node opened above");
        let groups = self.pack(&mut open, offset);
        let leaves = self.layout.leaves.len() as u32..(self.layout.leaves.len() + open.leaves.len()) as u32;
        let tails = self.layout.tails.len() as u32..(self.layout.tails.len() + open.tails.len()) as u32;
        self.layout.leaves.append(&mut open.leaves);
        self.layout.tails.append(&mut open.tails);
        let (bits, min_bits) = (open.bits, open.min_bits + open.bits);
        self.spare.push(open);
        self.layout.nodes.push(Node { leaves, tails, groups, bits, min_bits, shape, base });
        Ok(NodeId(self.layout.nodes.len() as u32 - 1))
    }

    fn leaf(&mut self, kind: LeafKind, max: u64, path: PathId) -> Result<LeafSlot, Error> {
        let bits = bits_for(max);
        if bits > 32 {
            return Err(self.fail(LayoutError::LeafTooWide(bits)));
        }
        let open = self.current();
        let check_max = (mask(bits) > max).then_some(max);
        open.leaves.push(Leaf { kind, bits, position: open.bits, check_max, path, digit: None });
        open.bits += bits;
        Ok(LeafSlot(open.leaves.len() as u32 - 1))
    }

    fn pack(&mut self, open: &mut Open<'a>, offset: u32) -> std::ops::Range<u32> {
        let start = self.layout.groups.len() as u32;
        let maxes: Vec<u64> =
            open.leaves.iter().map(|leaf| leaf.check_max.unwrap_or_else(|| mask(leaf.bits))).collect();
        let found = groups(&maxes);
        if found.is_empty() {
            return start..start;
        }
        let mut gathered = Vec::with_capacity(found.len());
        for (index, members) in found.iter().enumerate() {
            let group = GroupId(start + index as u32);
            let mut place = 1u64;
            for &index in members {
                let base = maxes[index] + 1;
                let leaf = &mut open.leaves[index];
                leaf.digit = Some(Digit { group, place, base });
                leaf.bits = 0;
                leaf.position = 0;
                leaf.check_max = None;
                place *= base;
            }
            gathered.push((place, members.iter().map(|&index| LeafSlot(index as u32)).collect::<Vec<_>>()));
        }
        let mut position = offset;
        for leaf in open.leaves.iter_mut().filter(|leaf| leaf.is_placed()) {
            leaf.position = position;
            position += leaf.bits;
        }
        let path = open.leaves[0].path;
        for (index, (product, members)) in gathered.into_iter().enumerate() {
            let group = GroupId(start + index as u32);
            let bits = group_bits(product);
            let mut part = |part: u8, width: u32| {
                let kind = LeafKind::Group { group, part };
                open.leaves.push(Leaf { kind, bits: width, position, check_max: None, path, digit: None });
                position += width;
                LeafSlot(open.leaves.len() as u32 - 1)
            };
            let low = part(0, bits.min(32));
            let high = (bits > 32).then(|| part(1, bits - 32));
            self.layout.groups.push(Group { product, bits, low, high, members });
        }
        open.bits = position;
        start..self.layout.groups.len() as u32
    }

    fn tail(&mut self, tail: Tail<'a>) -> ShapeId {
        let open = self.current();
        open.tails.push(tail);
        let slot = TailSlot(open.tails.len() as u32 - 1);
        self.shape(Shape::Tail(slot))
    }

    fn count(&mut self, bounds: Bounds, k: u32, path: PathId) -> Result<Count, Error> {
        match bounds.max {
            Some(max) if max == bounds.min => Ok(Count::Fixed(max)),
            Some(max) => {
                let kind = LeafKind::Count { min: bounds.min };
                let leaf = self.leaf(kind, u64::from(max - bounds.min), path)?;
                Ok(Count::Head { leaf, min: bounds.min, max })
            }
            None => {
                self.current().min_bits += k + 1;
                Ok(Count::Coded { k, max: None })
            }
        }
    }

    fn fields(&mut self, fields: &'a [Field], base: PathId) -> Result<ShapeId, Error> {
        let mut shapes = Vec::with_capacity(fields.len());
        for field in fields {
            self.trail.push(&field.name);
            let path = self.path(Some(base), Some(&field.name));
            let shape = self.add(field.ty, path)?;
            self.trail.pop();
            shapes.push(ShapeField { name: &field.name, shape });
        }
        let start = self.layout.shape_fields.len() as u32;
        self.layout.shape_fields.extend(shapes);
        Ok(self.shape(Shape::Struct(start..self.layout.shape_fields.len() as u32)))
    }

    fn axes(&mut self, (min, max, step): (f64, f64, f64), path: PathId) -> Result<[LeafSlot; 3], Error> {
        let steps = float_steps(min, max, step);
        let mut axes = [LeafSlot(0); 3];
        for (slot, axis) in axes.iter_mut().zip(["X", "Y", "Z"]) {
            let axis_path = self.path(Some(path), Some(axis));
            *slot = self.leaf(LeafKind::Value(Codec::Float { min, step, steps }), steps, axis_path)?;
        }
        Ok(axes)
    }

    fn real_axes(&mut self, path: PathId) -> Result<[LeafSlot; 3], Error> {
        let mut axes = [LeafSlot(0); 3];
        for (slot, axis) in axes.iter_mut().zip(["X", "Y", "Z"]) {
            let axis_path = self.path(Some(path), Some(axis));
            *slot = self.leaf(LeafKind::Value(Codec::F32), u64::from(u32::MAX), axis_path)?;
        }
        Ok(axes)
    }

    fn entry(&mut self, ty: TypeId, step: &'a str) -> Result<NodeId, Error> {
        self.trail.push(step);
        let base = self.path(None, None);
        let node = self.node(0, base, |builder, base| builder.add(ty, base));
        self.trail.pop();
        node
    }

    fn rotation(&mut self, degrees: f64, position: Option<[LeafSlot; 3]>, path: PathId) -> Result<ShapeId, Error> {
        let steps = rotation_steps(degrees);
        let mut parts = [LeafSlot(0); 4];
        for (part, slot) in (0u8..).zip(parts.iter_mut()) {
            let largest = if part == 0 { 3 } else { u64::from(steps) };
            *slot = self.leaf(LeafKind::Rotation { part, steps }, largest, path)?;
        }
        Ok(self.shape(Shape::Rotation { parts, position, steps }))
    }

    fn union(&mut self, union: UnionId, optional: bool, path: PathId) -> Result<ShapeId, Error> {
        let variants = &self.schema.union(union).variants;
        let largest = variants.len() as u64 - u64::from(!optional);
        let tag = self.leaf(LeafKind::Tag { union, optional }, largest, path)?;
        let mut nodes = Vec::with_capacity(variants.len());
        for variant in variants {
            self.trail.push(&variant.name);
            let base = self.path(None, None);
            nodes.push(self.node(0, base, |builder, base| builder.fields(&variant.fields, base))?);
            self.trail.pop();
        }
        let start = self.layout.variants.len() as u32;
        self.layout.variants.extend(nodes);
        let range = start..self.layout.variants.len() as u32;
        Ok(self.tail(Tail::Union { path, union, tag, variants: range, optional }))
    }

    fn add(&mut self, ty: TypeId, path: PathId) -> Result<ShapeId, Error> {
        let schema = self.schema;
        let leaf = |builder: &mut Self, codec: Codec, max: u64| -> Result<ShapeId, Error> {
            let slot = builder.leaf(LeafKind::Value(codec), max, path)?;
            Ok(builder.shape(Shape::Leaf(slot)))
        };
        match schema.ty(ty) {
            Type::Bool => leaf(self, Codec::Bool, 1),
            &Type::Int { min, max, wraps } => {
                let span = (max - min) as u64;
                leaf(self, Codec::Int { min, span, wraps }, span)
            }
            &Type::Float { min, max, step } => {
                let steps = float_steps(min, max, step);
                leaf(self, Codec::Float { min, step, steps }, steps)
            }
            &Type::Vector { min, max, step } => {
                let axes = self.axes((min, max, step), path)?;
                Ok(self.shape(Shape::Vector(axes)))
            }
            Type::Real { wide: false } => leaf(self, Codec::F32, u64::from(u32::MAX)),
            Type::Real { wide: true } => {
                self.leaf(LeafKind::Value(Codec::F64 { high: false }), u64::from(u32::MAX), path)?;
                let high = self.leaf(LeafKind::Value(Codec::F64 { high: true }), u64::from(u32::MAX), path)?;
                Ok(self.shape(Shape::Leaf(high)))
            }
            Type::RealVector => {
                let axes = self.real_axes(path)?;
                Ok(self.shape(Shape::Vector(axes)))
            }
            &Type::Map { key, value, bounds } => {
                let count = self.count(bounds, ARRAY_K, path)?;
                let aligned = std::mem::replace(&mut self.aligned, false);
                let key = self.entry(key, "[key]");
                let value = self.entry(value, "[value]");
                self.aligned = aligned;
                let (key, value) = (key?, value?);
                let entry_bits = self.layout.node(key).min_bits + self.layout.node(value).min_bits;
                if entry_bits == 0 {
                    return Err(self.fail(LayoutError::EmptyElement));
                }
                if let Count::Fixed(fixed) = count {
                    self.current().min_bits += fixed * entry_bits;
                }
                Ok(self.tail(Tail::Map { path, count, key, value }))
            }
            &Type::Rotation { degrees } => self.rotation(degrees, None, path),
            &Type::CFrame { min, max, step, degrees } => {
                let position = self.axes((min, max, step), path)?;
                self.rotation(degrees, Some(position), path)
            }
            &Type::Enum(id) => {
                let count = schema.enumeration(id).values.len() as u64;
                leaf(self, Codec::Enum { id, optional: false }, count - 1)
            }
            &Type::Union(id) => self.union(id, false, path),
            &Type::Struct(id) => {
                let mut shapes = Vec::new();
                for field in &schema.structure(id).fields {
                    self.trail.push(&field.name);
                    let field_path = self.path(Some(path), Some(&field.name));
                    let shape = self.add(field.ty, field_path)?;
                    self.trail.pop();
                    shapes.push(ShapeField { name: &field.name, shape });
                }
                let start = self.layout.shape_fields.len() as u32;
                self.layout.shape_fields.extend(shapes);
                Ok(self.shape(Shape::Struct(start..self.layout.shape_fields.len() as u32)))
            }
            &Type::Optional(inner) => match *schema.ty(inner) {
                Type::Enum(id) => {
                    let count = schema.enumeration(id).values.len() as u64;
                    leaf(self, Codec::Enum { id, optional: true }, count)
                }
                Type::Union(id) => self.union(id, true, path),
                _ => {
                    let present = self.leaf(LeafKind::Presence, 1, path)?;
                    let node = self.node(0, path, |builder, base| builder.add(inner, base))?;
                    Ok(self.tail(Tail::Optional { path, present, node }))
                }
            },
            &Type::Array { element, bounds } => {
                let count = self.count(bounds, ARRAY_K, path)?;
                self.trail.push("[]");
                let base = self.path(None, None);
                let node = self.node(0, base, |builder, base| builder.add(element, base))?;
                let element_node = self.layout.node(node);
                let holds_instance = self.layout.tails(node).iter().any(|tail| matches!(tail, Tail::Instance { .. }));
                if element_node.min_bits == 0 && !holds_instance {
                    return Err(self.fail(LayoutError::EmptyElement));
                }
                let element_bits = element_node.min_bits;
                let chunk = match (self.layout.leaves(node), self.layout.tails(node).is_empty()) {
                    ([leaf], true) if leaf.is_placed() => Chunk::new(leaf.check_max.unwrap_or_else(|| mask(leaf.bits))),
                    _ => None,
                };
                self.trail.pop();
                if let Count::Fixed(fixed) = count {
                    let bits = match chunk {
                        Some(chunk) => chunk.bits_for_count(u64::from(fixed)) as u32,
                        None => fixed * element_bits,
                    };
                    self.current().min_bits += bits;
                }
                let padded = self.aligned
                    && chunk.is_none()
                    && self.layout.tails(node).is_empty()
                    && element_bits > 0
                    && element_bits.is_multiple_of(8);
                Ok(self.tail(Tail::Array { path, count, element: node, chunk, padded }))
            }
            Type::Instance { class } => Ok(self.tail(Tail::Instance { path, class: class.as_deref(), handle: false })),
            Type::Handle => Ok(self.tail(Tail::Instance { path, class: None, handle: true })),
            &Type::Text(bounds) | &Type::Buffer(bounds) => {
                let count = self.count(bounds, BYTES_K, path)?;
                let buffer = matches!(schema.ty(ty), Type::Buffer(_));
                Ok(self.tail(Tail::Bytes { path, count, buffer }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse;

    #[test]
    fn heads_pack_after_the_event_id() {
        let schema = parse("event A from Server { X: u18, F: bool, Y: u14 } event B from Server {}").expect("valid");
        let layout = layout(&schema).expect("lays out");
        assert_eq!(layout.directions[0].ids, IdCodes { bits: 2, short: 1 });
        let node = layout.directions[0].events[0].node;
        let positions: Vec<(u32, u32)> = layout.leaves(node).iter().map(|leaf| (leaf.position, leaf.bits)).collect();
        assert_eq!(positions, [(2, 18), (20, 1), (21, 14)]);
        assert_eq!(layout.node(node).bits, 35);
    }

    #[test]
    fn nil_folds_into_enum_and_union_tags() {
        let schema = parse("enum E { A, B, C } union U { P, Q } event A from Server { E: E?, U: U? }").expect("valid");
        let layout = layout(&schema).expect("lays out");
        let node = layout.directions[0].events[0].node;
        let bits: Vec<u32> = layout.leaves(node).iter().map(|leaf| leaf.bits).collect();
        assert_eq!(bits, [2, 2]);
        assert!(matches!(layout.leaves(node)[0].kind, LeafKind::Value(Codec::Enum { optional: true, .. })));
    }

    #[test]
    fn counts_are_fixed_in_the_head_or_coded() {
        let schema = parse("event A from Server { F: u8[3], H: u8[2..9], C: u8[] }").expect("valid");
        let layout = layout(&schema).expect("lays out");
        let node = layout.directions[0].events[0].node;
        let counts: Vec<Count> = layout
            .tails(node)
            .iter()
            .map(|tail| match tail {
                Tail::Array { count, .. } => *count,
                other => panic!("expected arrays, got {other:?}"),
            })
            .collect();
        assert_eq!(
            counts,
            [
                Count::Fixed(3),
                Count::Head { leaf: LeafSlot(0), min: 2, max: 9 },
                Count::Coded { k: ARRAY_K, max: None }
            ]
        );
        assert!(layout.directions[0].events[0].aligned);
        assert_eq!(layout.node(node).min_bits, 3 + 24 + 3);
    }

    #[test]
    fn doubles_take_two_words_and_maps_a_tail() {
        let schema = parse("event A from Server { D: f64, M: map(u8, bool, ..4) }").expect("valid");
        let layout = layout(&schema).expect("lays out");
        let node = layout.directions[0].events[0].node;
        let bits: Vec<u32> = layout.leaves(node).iter().map(|leaf| leaf.bits).collect();
        assert_eq!(bits, [32, 32, 3]);
        assert!(matches!(layout.tails(node)[0], Tail::Map { count: Count::Head { min: 0, max: 4, .. }, .. }));
    }

    #[test]
    fn optional_paths_share_the_inner_node_base() {
        let schema = parse("struct P { X: u8 } event A from Server { O: P? }").expect("valid");
        let layout = layout(&schema).expect("lays out");
        let node = layout.directions[0].events[0].node;
        let Tail::Optional { path, node: inner, .. } = layout.tails(node)[0] else { panic!("an optional") };
        assert_eq!(layout.node(inner).base, path);
        assert_eq!(layout.leaves(inner)[0].path, PathId(path.0 + 1));
    }

    #[test]
    fn errors_name_the_field() {
        let schema = parse("event A from Server { S: float(0..1000000, 0.0000001) }").expect("valid");
        match layout(&schema) {
            Err(Error::Layout { path, problem: LayoutError::LeafTooWide(_) }) => assert_eq!(path, "A.S"),
            other => panic!("expected a layout error, got {other:?}"),
        }
    }
}
