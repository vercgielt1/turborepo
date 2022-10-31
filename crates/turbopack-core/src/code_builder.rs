use std::{
    io::{Result as IoResult, Write},
    ops,
};

use anyhow::Result;
use sourcemap::SourceMapBuilder;
use turbo_tasks_fs::rope::{Rope, RopeBuilder};

use crate::{
    source_map::{GenerateSourceMap, GenerateSourceMapVc, SourceMapSection, SourceMapVc},
    source_pos::SourcePos,
};

/// Code stores combined output code and the source map of that output code.
#[turbo_tasks::value(shared)]
#[derive(Debug, Clone)]
pub struct Code {
    code: Rope,

    /// A mapping of byte-offset in the code string to an associated source map.
    mappings: Vec<(usize, Option<GenerateSourceMapVc>)>,
}

#[derive(Default)]
pub struct CodeBuilder {
    code: RopeBuilder,

    mappings: Vec<(usize, Option<GenerateSourceMapVc>)>,
}

impl Code {
    pub fn source_code(&self) -> &Rope {
        &self.code
    }

    /// Tests if any code in this Code contains an associated source map.
    pub fn has_source_map(&self) -> bool {
        !self.mappings.is_empty()
    }
}

impl CodeBuilder {
    /// Pushes synthetic runtime code without an associated source map. This is
    /// the default concatenation operation, but it's designed to be used
    /// with the `+=` operator.
    fn push_static_bytes(&mut self, code: &'static [u8]) {
        self.push_map(None);
        self.code.push_static_bytes(code);
    }

    /// Pushes original user code with an optional source map if one is
    /// available. If it's not, this is no different than pushing Synthetic
    /// code.
    pub fn push_source(&mut self, code: &Rope, map: Option<GenerateSourceMapVc>) {
        self.push_map(map);
        self.code.concat(code);
    }

    /// Copies the Synthetic/Original code of an already constructed Code
    /// into this instance.
    pub fn push_code(&mut self, prebuilt: &Code) {
        if let Some((index, _)) = prebuilt.mappings.first() {
            if *index > 0 {
                // If the index is positive, then the code starts with a synthetic section. We
                // may need to push an empty map in order to end the current
                // section's mappings.
                self.push_map(None);
            }

            let len = self.code.len();
            self.mappings.extend(
                prebuilt
                    .mappings
                    .iter()
                    .map(|(index, map)| (index + len, *map)),
            );
        } else {
            self.push_map(None);
        }

        self.code.concat(&prebuilt.code);
    }

    /// Setting breakpoints on synthetic code can cause weird behaviors
    /// because Chrome will treat the location as belonging to the previous
    /// original code section. By inserting an empty source map when reaching a
    /// synthetic section directly after an original section, we tell Chrome
    /// that the previous map ended at this point.
    fn push_map(&mut self, map: Option<GenerateSourceMapVc>) {
        if map.is_none() && matches!(self.mappings.last(), None | Some((_, None))) {
            // No reason to push an empty map directly after an empty map
            return;
        }

        debug_assert!(
            map.is_some() || !self.mappings.is_empty(),
            "the first mapping is never a None"
        );
        self.mappings.push((self.code.len(), map));
    }

    /// Tests if any code in this CodeBuilder contains an associated source map.
    pub fn has_source_map(&self) -> bool {
        !self.mappings.is_empty()
    }

    pub fn build(self) -> Code {
        Code {
            code: self.code.build(),
            mappings: self.mappings,
        }
    }
}

impl ops::AddAssign<&'static str> for CodeBuilder {
    fn add_assign(&mut self, rhs: &'static str) {
        self.push_static_bytes(rhs.as_bytes());
    }
}

impl Write for CodeBuilder {
    fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
        self.push_map(None);
        self.code.write(bytes)
    }

    fn flush(&mut self) -> IoResult<()> {
        self.code.flush()
    }
}

#[turbo_tasks::value_impl]
impl GenerateSourceMap for Code {
    /// Generates the source map out of all the pushed Original code.
    /// The SourceMap v3 spec has a "sectioned" source map specifically designed
    /// for concatenation in post-processing steps. This format consists of
    /// a `sections` array, with section item containing a `offset` object
    /// and a `map` object. The section's map applies only after the
    /// starting offset, and until the start of the next section. This is by
    /// far the simplest way to concatenate the source maps of the multiple
    /// chunk items into a single map file.
    #[turbo_tasks::function]
    pub async fn generate_source_map(&self) -> Result<SourceMapVc> {
        let mut pos = SourcePos::new();
        let mut last_byte_pos = 0;

        let mut sections = Vec::with_capacity(self.mappings.len());
        for (byte_pos, map) in &self.mappings {
            pos.update_from_read(&mut self.code.slice(last_byte_pos, *byte_pos))?;
            last_byte_pos = *byte_pos;

            let encoded = match map {
                None => empty_map(),
                Some(map) => map.generate_source_map(),
            };

            sections.push(SourceMapSection::new(pos, encoded))
        }

        Ok(SourceMapVc::new_sectioned(sections))
    }
}

/// A source map that contains no actual source location information (no
/// `sources`, no mappings that point into a source). This is used to tell
/// Chrome that the generated code starting at a particular offset is no longer
/// part of the previous section's mappings.
#[turbo_tasks::function]
fn empty_map() -> SourceMapVc {
    let mut builder = SourceMapBuilder::new(None);
    builder.add(0, 0, 0, 0, None, None);
    SourceMapVc::new_regular(builder.into_sourcemap())
}
