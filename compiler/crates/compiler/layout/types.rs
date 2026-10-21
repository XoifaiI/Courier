//! The layout's data: each message as nodes of leaves and tails, flattened into shared lists.

use std::ops::Range;

use super::numbers::IdCodes;
use super::packing::Chunk;
use crate::schema::{EnumId, Role, Side, UnionId};

macro_rules! id {
    ($($id:ident),*) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $id(pub u32);

        impl $id {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    )*};
}

id!(NodeId, PathId, ShapeId, GroupId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeafSlot(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TailSlot(pub u32);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Codec {
    Bool,
    Int { min: i64, span: u64, wraps: bool },
    Float { min: f64, step: f64, steps: u64 },
    F32,
    F64 { high: bool },
    Enum { id: EnumId, optional: bool },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeafKind {
    Value(Codec),
    Presence,
    Count { min: u32 },
    Tag { union: UnionId, optional: bool },
    Rotation { part: u8, steps: u32 },
    Group { group: GroupId, part: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Digit {
    pub group: GroupId,
    pub place: u64,
    pub base: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub product: u64,
    pub bits: u32,
    pub low: LeafSlot,
    pub high: Option<LeafSlot>,
    pub members: Vec<LeafSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leaf {
    pub kind: LeafKind,
    pub bits: u32,
    pub position: u32,
    pub check_max: Option<u64>,
    pub path: PathId,
    pub digit: Option<Digit>,
}

impl Leaf {
    pub fn is_flag(&self) -> bool {
        matches!(self.kind, LeafKind::Presence | LeafKind::Value(Codec::Bool))
    }

    pub fn is_placed(&self) -> bool {
        self.digit.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Fixed(u32),
    Head { leaf: LeafSlot, min: u32, max: u32 },
    Coded { k: u32, max: Option<u32> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tail<'a> {
    Optional { path: PathId, present: LeafSlot, node: NodeId },
    Array { path: PathId, count: Count, element: NodeId, chunk: Option<Chunk> },
    Map { path: PathId, count: Count, key: NodeId, value: NodeId },
    Bytes { path: PathId, count: Count, buffer: bool },
    Instance { path: PathId, class: Option<&'a str>, handle: bool },
    Union { path: PathId, union: UnionId, tag: LeafSlot, variants: Range<u32>, optional: bool },
}

impl Tail<'_> {
    pub fn path(&self) -> PathId {
        match self {
            Tail::Optional { path, .. }
            | Tail::Array { path, .. }
            | Tail::Map { path, .. }
            | Tail::Bytes { path, .. }
            | Tail::Instance { path, .. }
            | Tail::Union { path, .. } => *path,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Leaf(LeafSlot),
    Vector([LeafSlot; 3]),
    Rotation { parts: [LeafSlot; 4], position: Option<[LeafSlot; 3]>, steps: u32 },
    Struct(Range<u32>),
    Tail(TailSlot),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeField<'a> {
    pub name: &'a str,
    pub shape: ShapeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path<'a> {
    pub parent: Option<PathId>,
    pub field: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub leaves: Range<u32>,
    pub tails: Range<u32>,
    pub groups: Range<u32>,
    pub bits: u32,
    pub min_bits: u32,
    pub shape: ShapeId,
    pub base: PathId,
}

impl Node {
    pub fn words(&self) -> u32 {
        self.bits.div_ceil(32)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventLayout {
    pub event: usize,
    pub id: u32,
    pub code: u32,
    pub code_bits: u32,
    pub aligned: bool,
    pub reliable: bool,
    pub role: Role,
    pub node: NodeId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Direction {
    pub from: Side,
    pub ids: IdCodes,
    pub events: Vec<EventLayout>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layout<'a> {
    pub nodes: Vec<Node>,
    pub leaves: Vec<Leaf>,
    pub tails: Vec<Tail<'a>>,
    pub shapes: Vec<Shape>,
    pub shape_fields: Vec<ShapeField<'a>>,
    pub groups: Vec<Group>,
    pub paths: Vec<Path<'a>>,
    pub variants: Vec<NodeId>,
    pub directions: Vec<Direction>,
}

impl<'a> Layout<'a> {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    pub fn is_bare_bool(&self, node: NodeId) -> bool {
        matches!(self.leaves(node), [leaf] if matches!(leaf.kind, LeafKind::Value(Codec::Bool)) && leaf.is_placed())
            && matches!(self.shape(self.node(node).shape), Shape::Leaf(_))
            && self.tails(node).is_empty()
    }

    pub fn leaves(&self, node: NodeId) -> &[Leaf] {
        let range = &self.node(node).leaves;
        &self.leaves[range.start as usize..range.end as usize]
    }

    pub fn tails(&self, node: NodeId) -> &[Tail<'a>] {
        let range = &self.node(node).tails;
        &self.tails[range.start as usize..range.end as usize]
    }

    pub fn leaf(&self, node: NodeId, slot: LeafSlot) -> &Leaf {
        &self.leaves(node)[slot.0 as usize]
    }

    pub fn groups(&self, node: NodeId) -> &[Group] {
        let range = &self.node(node).groups;
        &self.groups[range.start as usize..range.end as usize]
    }

    pub fn group(&self, id: GroupId) -> &Group {
        &self.groups[id.index()]
    }

    pub fn tail(&self, node: NodeId, slot: TailSlot) -> &Tail<'a> {
        &self.tails(node)[slot.0 as usize]
    }

    pub fn shape(&self, id: ShapeId) -> &Shape {
        &self.shapes[id.index()]
    }

    pub fn fields(&self, range: &Range<u32>) -> &[ShapeField<'a>] {
        &self.shape_fields[range.start as usize..range.end as usize]
    }

    pub fn path(&self, id: PathId) -> &Path<'a> {
        &self.paths[id.index()]
    }

    pub fn variant_nodes(&self, range: &Range<u32>) -> &[NodeId] {
        &self.variants[range.start as usize..range.end as usize]
    }

    pub fn events(&self) -> impl Iterator<Item = (&Direction, &EventLayout)> {
        self.directions.iter().flat_map(|direction| direction.events.iter().map(move |event| (direction, event)))
    }

    pub fn any_tail(&self, node: NodeId, found: &impl Fn(&Tail) -> bool) -> bool {
        self.tails(node).iter().any(|tail| {
            found(tail)
                || match tail {
                    Tail::Optional { node, .. } | Tail::Array { element: node, .. } => self.any_tail(*node, found),
                    Tail::Map { key, value, .. } => self.any_tail(*key, found) || self.any_tail(*value, found),
                    Tail::Union { variants, .. } => {
                        self.variant_nodes(variants).iter().any(|variant| self.any_tail(*variant, found))
                    }
                    Tail::Bytes { .. } | Tail::Instance { .. } => false,
                }
        })
    }

    pub fn any_leaf(&self, node: NodeId, found: &impl Fn(&Leaf) -> bool) -> bool {
        self.leaves(node).iter().any(found)
            || self.tails(node).iter().any(|tail| match tail {
                Tail::Optional { node, .. } | Tail::Array { element: node, .. } => self.any_leaf(*node, found),
                Tail::Map { key, value, .. } => self.any_leaf(*key, found) || self.any_leaf(*value, found),
                Tail::Union { variants, .. } => {
                    self.variant_nodes(variants).iter().any(|variant| self.any_leaf(*variant, found))
                }
                Tail::Bytes { .. } | Tail::Instance { .. } => false,
            })
    }
}
