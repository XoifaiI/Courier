//! The parsed schema.

use std::borrow::Cow;

pub type Name<'s> = Cow<'s, str>;

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

id!(TypeId, EnumId, StructId, UnionId, FunctionId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub min: u32,
    pub max: Option<u32>,
}

impl Bounds {
    pub const UNBOUNDED: Bounds = Bounds { min: 0, max: None };

    pub fn fixed(self) -> Option<u32> {
        self.max.filter(|max| *max == self.min)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type<'s> {
    Bool,
    Int { min: i64, max: i64, wraps: bool },
    Float { min: f64, max: f64, step: f64 },
    Vector { min: f64, max: f64, step: f64 },
    Real { wide: bool },
    RealVector,
    Rotation { degrees: f64 },
    CFrame { min: f64, max: f64, step: f64, degrees: f64 },
    Enum(EnumId),
    Struct(StructId),
    Union(UnionId),
    Optional(TypeId),
    Array { element: TypeId, bounds: Bounds },
    Map { key: TypeId, value: TypeId, bounds: Bounds },
    Text(Bounds),
    Buffer(Bounds),
    Instance { class: Option<Name<'s>> },
    Handle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field<'s> {
    pub name: Name<'s>,
    pub ty: TypeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Server,
    Client,
}

impl Side {
    pub const BOTH: [Side; 2] = [Side::Server, Side::Client];

    pub fn other(self) -> Side {
        match self {
            Side::Server => Side::Client,
            Side::Client => Side::Server,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Message,
    Request(FunctionId),
    Reply(FunctionId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event<'s> {
    pub name: Name<'s>,
    pub from: Side,
    pub reliable: bool,
    pub role: Role,
    pub fields: Vec<Field<'s>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function<'s> {
    pub name: Name<'s>,
    pub from: Side,
    pub arguments: Vec<Field<'s>>,
    pub results: Vec<Field<'s>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Enum<'s> {
    pub name: Name<'s>,
    pub values: Vec<Name<'s>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct<'s> {
    pub name: Name<'s>,
    pub fields: Vec<Field<'s>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant<'s> {
    pub name: Name<'s>,
    pub fields: Vec<Field<'s>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Union<'s> {
    pub name: Name<'s>,
    pub variants: Vec<Variant<'s>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declaration {
    Enum(EnumId),
    Struct(StructId),
    Union(UnionId),
}

pub const UNION_TAG: &str = "Kind";
pub const CALL_ID: &str = "CallId";
pub const CALL_RESULT: &str = "Result";
pub const REASON_LENGTH: u32 = 200;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Schema<'s> {
    pub output: Option<Name<'s>>,
    pub runtime: Option<Name<'s>>,
    pub types: Vec<Type<'s>>,
    pub enums: Vec<Enum<'s>>,
    pub structs: Vec<Struct<'s>>,
    pub unions: Vec<Union<'s>>,
    pub events: Vec<Event<'s>>,
    pub functions: Vec<Function<'s>>,
    pub declarations: Vec<Declaration>,
}

impl<'s> Schema<'s> {
    pub fn ty(&self, id: TypeId) -> &Type<'s> {
        &self.types[id.index()]
    }

    pub fn enumeration(&self, id: EnumId) -> &Enum<'s> {
        &self.enums[id.index()]
    }

    pub fn structure(&self, id: StructId) -> &Struct<'s> {
        &self.structs[id.index()]
    }

    pub fn union(&self, id: UnionId) -> &Union<'s> {
        &self.unions[id.index()]
    }

    pub fn function(&self, id: FunctionId) -> &Function<'s> {
        &self.functions[id.index()]
    }

    pub fn intern(&mut self, ty: Type<'s>) -> TypeId {
        if let Some(found) = self.types.iter().position(|existing| *existing == ty) {
            return TypeId(found as u32);
        }
        self.types.push(ty);
        TypeId(self.types.len() as u32 - 1)
    }

    pub fn holds_instance(&self, ty: TypeId) -> bool {
        let any = |fields: &[Field]| fields.iter().any(|field| self.holds_instance(field.ty));
        match self.ty(ty) {
            Type::Instance { .. } | Type::Handle => true,
            Type::Optional(inner) | Type::Array { element: inner, .. } => self.holds_instance(*inner),
            Type::Map { key, value, .. } => self.holds_instance(*key) || self.holds_instance(*value),
            Type::Struct(id) => any(&self.structure(*id).fields),
            Type::Union(id) => self.union(*id).variants.iter().any(|variant| any(&variant.fields)),
            _ => false,
        }
    }

    pub fn events_from(&self, side: Side) -> impl Iterator<Item = &Event<'s>> {
        self.events.iter().filter(move |event| event.from == side)
    }

    pub fn desugar(&mut self) {
        let call_id = self.intern(Type::Int { min: 0, max: 65535, wraps: true });
        let reason = self.intern(Type::Text(Bounds { min: 0, max: Some(REASON_LENGTH) }));
        for index in 0..self.functions.len() {
            let id = FunctionId(index as u32);
            let function = self.functions[index].clone();
            let mut request = vec![Field { name: Cow::Borrowed(CALL_ID), ty: call_id }];
            request.extend(function.arguments);
            self.events.push(Event {
                name: function.name.clone(),
                from: function.from,
                reliable: true,
                role: Role::Request(id),
                fields: request,
            });
            self.unions.push(Union {
                name: Cow::Owned(format!("{}Result", function.name)),
                variants: vec![
                    Variant { name: Cow::Borrowed("Ok"), fields: function.results },
                    Variant {
                        name: Cow::Borrowed("Error"),
                        fields: vec![Field { name: Cow::Borrowed("Reason"), ty: reason }],
                    },
                ],
            });
            let result = self.intern(Type::Union(UnionId(self.unions.len() as u32 - 1)));
            self.events.push(Event {
                name: Cow::Owned(format!("{}Reply", function.name)),
                from: function.from.other(),
                reliable: true,
                role: Role::Reply(id),
                fields: vec![
                    Field { name: Cow::Borrowed(CALL_ID), ty: call_id },
                    Field { name: Cow::Borrowed(CALL_RESULT), ty: result },
                ],
            });
        }
    }
}
