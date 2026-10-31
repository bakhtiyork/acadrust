// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/IO/SvgWriter.cs) by Albert Domenech.
// Copyright (c) 2021 Albert Domenech
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! A port of ACadSharp's SVG writer (`ACadSharp.IO.SvgWriter` and `ACadSharp.IO.SVG.*`) over the
//! fork's own document model.
//!
//! ```no_run
//! use acadrust::svg::{SvgConfiguration, SvgWriter};
//! # fn run(doc: &acadrust::CadDocument) -> Result<(), acadrust::svg::SvgError> {
//! let mut out = Vec::new();
//! SvgWriter::new(&mut out, doc)
//!     .with_configuration(SvgConfiguration::default())
//!     .write()?; // the model space
//! # Ok(()) }
//! ```
//!
//! The output is byte for byte what ACadSharp writes (the pattern ids of hatches excepted: ACadSharp
//! derives them from object identity hashes, which differ from run to run). Where ACadSharp throws,
//! the writer returns an [`SvgError`] whose [`SvgError::csharp_exception`] names the C# exception type.

mod configuration;
mod converter;
mod csmath;
mod document_builder;
mod error;
mod geometry;
mod model;
pub(crate) mod mtext_plain_text;
pub(crate) mod palette;
mod xml_text_writer;
mod xml_writer;

#[cfg(test)]
mod tests;

pub use configuration::SvgConfiguration;
pub use converter::UnitsType;
pub use document_builder::SvgDocumentBuilder;
pub use error::SvgError;
pub use model::LayoutView;
pub use xml_text_writer::{Formatting, XmlTextWriter};
pub use xml_writer::{SvgNotification, SvgNotificationKind, SvgXmlWriter};

use crate::document::CadDocument;
use crate::objects::Layout;
use crate::tables::BlockRecord;
use model::Model;
#[cfg(feature = "acadsharp-compat")]
pub(crate) use model::acadsharp_entity_color;
use std::io::Write;

/// Writer to create SVG from a [`BlockRecord`] or a [`Layout`] (ACadSharp `SvgWriter`).
pub struct SvgWriter<'a, W: Write> {
    out: W,
    model: Model<'a>,
    /// The configuration of the next writes.
    pub configuration: SvgConfiguration,
    notifications: Vec<SvgNotification>,
}

impl<'a, W: Write> SvgWriter<'a, W> {
    /// Creates a writer for `document` over `out`, with the default configuration.
    pub fn new(out: W, document: &'a CadDocument) -> Self {
        SvgWriter {
            out,
            model: Model::new(document, false),
            configuration: SvgConfiguration::default(),
            notifications: Vec::new(),
        }
    }

    /// Replaces the configuration.
    pub fn with_configuration(mut self, configuration: SvgConfiguration) -> Self {
        self.model.view = configuration.acadsharp_view;
        self.configuration = configuration;
        self
    }

    /// The notifications raised so far (entities the writer does not implement).
    pub fn notifications(&self) -> &[SvgNotification] {
        &self.notifications
    }

    /// Writes the model space.
    pub fn write(&mut self) -> Result<(), SvgError> {
        let record = self
            .model
            .model_space()
            .ok_or_else(|| SvgError::null_reference("Object reference not set to an instance of an object."))?;
        self.write_block_record(record)
    }

    /// Writes the entities of a block record, sized to their bounding box.
    pub fn write_block_record(&mut self, record: &'a BlockRecord) -> Result<(), SvgError> {
        let mut writer = SvgXmlWriter::new(&mut self.out, self.model.clone(), self.configuration.clone());
        let result = writer.write_block(record);
        self.notifications.extend(writer.notifications().iter().cloned());
        result
    }

    /// Writes a layout of the document.
    pub fn write_layout(&mut self, layout: &'a Layout) -> Result<(), SvgError> {
        let block = self
            .model
            .block_by_handle(layout.block_record)
            .ok_or_else(|| SvgError::null_reference("Object reference not set to an instance of an object."))?;
        let mut writer = SvgXmlWriter::new(&mut self.out, self.model.clone(), self.configuration.clone());
        let result = writer.write_layout(LayoutView::of(layout), block);
        self.notifications.extend(writer.notifications().iter().cloned());
        result
    }

    /// The paper-space layouts of the document, in the order of `CadDocument.Layouts`.
    pub fn paper_space_layouts(&self) -> Vec<&'a Layout> {
        self.model
            .layouts()
            .into_iter()
            .filter(|l| !l.name.eq_ignore_ascii_case("Model"))
            .collect()
    }

    /// Hands the output stream back.
    pub fn into_inner(self) -> W {
        self.out
    }
}

/// The name of the model-space block record (`doc.ModelSpace.Name`).
pub fn model_space_name(document: &CadDocument) -> String {
    Model::new(document, false)
        .model_space()
        .map(|b| b.name.clone())
        .unwrap_or_default()
}

/// The block records in the order of ACadSharp's `CadDocument.BlockRecords`: for a DWG the BLOCK_CONTROL
/// object's list followed by the model and paper space records, otherwise the table order.
pub fn acadsharp_block_records(document: &CadDocument) -> Vec<&BlockRecord> {
    let Some(order) = &document.source_block_record_order else {
        return document.block_records.iter().collect();
    };
    let mut seen = std::collections::HashSet::new();
    order
        .iter()
        .filter(|h| seen.insert(**h))
        .filter_map(|h| document.block_records.iter().find(|b| b.handle == *h))
        .collect()
}

/// The text ACadSharp reads for a TEXT or attribute value: the file's spelling (`\U+00B0` escapes rather than the
/// decoded character) while the native value is still the decoded form of that spelling.
pub(crate) fn text_source_value(value: &str, source: Option<&crate::entities::text::TextSource>) -> String {
    if let Some(source) = source.filter(|s| s.decoded_value.as_deref() == Some(value)) {
        // .NET's default encoding is UTF-8; preserve native legacy fallback and
        // reproduce replacement decoding only with original undeclared bytes.
        let raw = source
            .undecoded_bytes
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .or_else(|| source.value.clone());
        if let Some(raw) = raw {
            return if source.dxf {
                raw.replace("^J", "\n")
                    .replace("^M", "\r")
                    .replace("^I", "\t")
                    .replace("^ ", "^")
            } else {
                raw
            };
        }
    }
    value.into()
}

/// A DWG before R2007 decodes text with .NET's DBCS decoders, which substitute one `?` for an undecodable lead byte
/// and the ASCII byte that follows it.
pub(crate) fn dwg_legacy_replacement(doc: &CadDocument, text: String) -> String {
    if doc.dwg_source_version.is_some() && doc.version < crate::types::DxfVersion::AC1021 && text.contains('\u{FFFD}') {
        let mut replaced = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{FFFD}' {
                replaced.push('?');
                if chars.peek().is_some_and(char::is_ascii) {
                    chars.next();
                }
            } else {
                replaced.push(c);
            }
        }
        return replaced;
    }
    text
}
