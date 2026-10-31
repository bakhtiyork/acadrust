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
//! The writer follows ACadSharp's element by element and draws the native model. With the `acadsharp-compat`
//! feature, `SvgConfiguration::acadsharp_view` reads the document as ACadSharp reads the same file instead (its
//! text spellings, colours, palette, R12 reading...), and the output is byte for byte what ACadSharp writes (the
//! pattern ids of hatches excepted: ACadSharp derives them from object identity hashes, which differ from run to
//! run; the port numbers the same objects in order of first use). Where ACadSharp throws, the writer returns an
//! [`SvgError`] whose [`SvgError::csharp_exception`] names the C# exception type.

mod configuration;
mod converter;
mod csmath;
mod document_builder;
mod error;
mod geometry;
mod model;
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
            model: Model::new(document),
            configuration: SvgConfiguration::default(),
            notifications: Vec::new(),
        }
    }

    /// Replaces the configuration.
    pub fn with_configuration(mut self, configuration: SvgConfiguration) -> Self {
        self.configuration = configuration;
        self
    }

    /// The document as the current configuration reads it.
    fn model(&self) -> Model<'a> {
        self.model.with_view(&self.configuration)
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
        self.model()
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
