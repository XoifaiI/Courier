//! The whole pipeline: parse, lay out, generate.

use crate::codegen::{File, Options, generate};
use crate::schema::Schema;
use crate::{Error, layout, schema};

pub fn compile(source: &str, options: &Options) -> Result<Vec<File>, Error> {
    compile_schema(&schema::parse(source)?, options)
}

pub fn compile_schema(schema: &Schema, options: &Options) -> Result<Vec<File>, Error> {
    let layout = layout::layout(schema)?;
    generate(schema, &layout, options)
}
