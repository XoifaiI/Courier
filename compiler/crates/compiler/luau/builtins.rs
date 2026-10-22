//! The Luau globals generated code calls, and which ones native code inlines.

pub const BAND: &str = "bit32.band";
pub const BOR: &str = "bit32.bor";
pub const BTEST: &str = "bit32.btest";
pub const COUNTLZ: &str = "bit32.countlz";
pub const COUNTRZ: &str = "bit32.countrz";
pub const EXTRACT: &str = "bit32.extract";
pub const LSHIFT: &str = "bit32.lshift";
pub const RSHIFT: &str = "bit32.rshift";

pub const READU8: &str = "buffer.readu8";
pub const READU16: &str = "buffer.readu16";
pub const READU32: &str = "buffer.readu32";
pub const WRITEU8: &str = "buffer.writeu8";
pub const WRITEU16: &str = "buffer.writeu16";
pub const WRITEU32: &str = "buffer.writeu32";
pub const READF32: &str = "buffer.readf32";
pub const WRITEF32: &str = "buffer.writef32";
pub const READSTRING: &str = "buffer.readstring";
pub const WRITESTRING: &str = "buffer.writestring";
pub const COPY: &str = "buffer.copy";
pub const CREATE: &str = "buffer.create";
pub const FILL: &str = "buffer.fill";
pub const LEN: &str = "buffer.len";

pub const ABS: &str = "math.abs";
pub const FLOOR: &str = "math.floor";
pub const MAX: &str = "math.max";
pub const MIN: &str = "math.min";
pub const ROUND: &str = "math.round";

pub const TABLE_CREATE: &str = "table.create";
pub const TABLE_CLONE: &str = "table.clone";
pub const TYPEOF: &str = "typeof";
pub const ERROR: &str = "error";
pub const REQUIRE: &str = "require";
pub const VECTOR3_NEW: &str = "Vector3.new";
pub const CFRAME_NEW: &str = "CFrame.new";

pub const FAST: [&str; 21] = [
    BAND, BOR, BTEST, COUNTLZ, COUNTRZ, EXTRACT, LSHIFT, RSHIFT, READU8, READU16, READU32, WRITEU8, WRITEU16, WRITEU32,
    READF32, WRITEF32, ABS, FLOOR, MAX, MIN, ROUND,
];
