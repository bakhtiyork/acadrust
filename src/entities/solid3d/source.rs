// SPDX-License-Identifier: MPL-2.0
//! Compact source provenance for modeler payloads. The native SAT string stays
//! normalized; source retention never duplicates the complete SAT/SAB payload.

use crate::types::DxfVersion;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum AcisSource {
    Dwg(DwgAcisSource),
    Dxf(DxfAcisSource),
}

#[derive(Debug, Clone, PartialEq)]
pub struct DwgAcisSource {
    pub data: Arc<[u8]>,
    pub start_bit: i64,
    pub end_bit: i64,
    pub version: DxfVersion,
}

impl DwgAcisSource {
    pub(crate) fn capture(
        reader: &mut crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader,
        version: DxfVersion,
    ) -> Self {
        Self {
            data: reader.shared_data(),
            start_bit: reader.position_in_bits(),
            end_bit: reader.position_in_bits() + reader.remaining_bits(),
            version,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DxfAcisChunk {
    /// A group 1 separator inserted before this chunk in the joined SAT text.
    pub new_line: bool,
    pub char_count: usize,
    /// Sparse differences from native decoded text; shared payload text is never retained.
    pub source_override: Option<Vec<SatTextPatch>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SatTextPatch {
    pub offset: usize,
    pub remove: usize,
    pub insert: String,
}

impl DxfAcisChunk {
    pub(crate) fn capture(new_line: bool, native: &str, raw: Option<String>) -> Self {
        let mut result = Self {
            new_line,
            char_count: native.chars().count(),
            source_override: None,
        };
        let Some(raw) = raw else { return result };
        // The oracle and native reader share caret decoding. Only differences
        // left by legacy decoding need retention; common encoded-SAT caret
        // escapes therefore do not retain any extra payload text.
        let raw = raw
            .replace("^J", "\n")
            .replace("^M", "\r")
            .replace("^I", "\t")
            .replace("^ ", "^");
        let mut expected = raw.chars().peekable();
        let mut actual = native.chars().peekable();
        let mut offset = 0;
        let mut patches: Vec<SatTextPatch> = Vec::new();
        while expected.peek().is_some() || actual.peek().is_some() {
            if expected.peek() == actual.peek() {
                expected.next();
                actual.next();
                offset += 1;
                continue;
            }
            // A bounded lookahead resynchronizes after short escape sequences.
            // This is a lossless delta even when a malformed escape is longer:
            // unmatched characters simply form a replacement run.
            let mut alignment = None;
            for (i, left) in expected.clone().take(16).enumerate() {
                for (j, right) in actual.clone().take(16).enumerate() {
                    if left == right && alignment.is_none_or(|(a, b)| i + j < a + b) {
                        alignment = Some((i, j));
                    }
                }
            }
            let (insert, remove) = alignment.unwrap_or((
                usize::from(expected.peek().is_some()),
                usize::from(actual.peek().is_some()),
            ));
            if patches.last().is_none_or(|p| p.offset + p.remove != offset) {
                patches.push(SatTextPatch {
                    offset,
                    remove: 0,
                    insert: String::new(),
                });
            }
            let patch = patches.last_mut().unwrap();
            patch.insert.extend(expected.by_ref().take(insert));
            for _ in 0..remove {
                actual.next();
            }
            patch.remove += remove;
            offset += remove;
        }
        if !patches.is_empty() {
            result.source_override = Some(patches);
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DxfAcisSource {
    pub modeler_version: i16,
    pub native_encoded: bool,
    pub chunks: Vec<DxfAcisChunk>,
    pub layout: SatLayout,
}

/// One byte per source line, plus only the suffix removed by normalization.
/// No copy of the normalized body is kept. A stripped suffix is not present
/// anywhere else in the entity, so retaining it does not duplicate the body.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SatLayout {
    pub line_endings: Vec<u8>,
    pub suffix: String,
}

impl SatLayout {
    pub fn capture(text: &str) -> Self {
        let mut layout = Self::default();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if line.starts_with("End-of-ACIS-data") || line.starts_with("End-of-ASM-data") {
                layout.suffix = text[offset..].to_owned();
                break;
            }
            layout.line_endings.push(if line.ends_with("\r\n") {
                2
            } else if line.ends_with('\n') {
                1
            } else {
                0
            });
            offset += line.len();
        }
        layout
    }

    /// Borrow the original text as segments, without materializing a second
    /// large payload while exporting the canonical representation.
    pub fn parts<'a>(&'a self, normalized: &'a str) -> impl Iterator<Item = &'a str> {
        normalized
            .split_inclusive('\n')
            .enumerate()
            .flat_map(|(i, line)| {
                let body = line.strip_suffix('\n').unwrap_or(line);
                let ending = match self.line_endings.get(i) {
                    Some(0) => "",
                    Some(2) => "\r\n",
                    _ => "\n",
                };
                [body, ending]
            })
            .chain(std::iter::once(self.suffix.as_str()))
    }
}
