// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/IO/SVG/SvgDocumentBuilder.cs) by Albert Domenech.
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

//! `SvgDocumentBuilder`: an unfinished subclass of the SVG writer in ACadSharp (marked obsolete there).
//!
//! `SvgWriter` never creates it. It is ported for completeness with its behaviour: its `WriteLayout`
//! hides the base method and reads the `Layout` property, which nothing assigns, so it throws a
//! `NullReferenceException` unless the caller sets the layout state first.

use super::converter::{to_svg_units, xyz_to_pixel, UnitsType};
use super::csmath::{BoundingBox, Transform, Xyz};
use super::error::SvgError;
use super::model::{LayoutView, Model};
use super::xml_writer::{paper_units, SvgXmlWriter};
use super::SvgConfiguration;
use crate::tables::BlockRecord;
use std::collections::HashMap;
use std::io::Write;

/// ACadSharp `SvgDocumentBuilder`.
pub struct SvgDocumentBuilder<'a, W: Write> {
    /// The base writer.
    pub writer: SvgXmlWriter<'a, W>,
    /// `LineTypeWriters`: one in-memory writer per line type name.
    pub line_type_writers: HashMap<String, SvgXmlWriter<'a, Vec<u8>>>,
    model: Model<'a>,
    configuration: SvgConfiguration,
}

impl<'a, W: Write> SvgDocumentBuilder<'a, W> {
    pub fn new(out: W, document: &'a crate::document::CadDocument, configuration: SvgConfiguration) -> Self {
        let model = Model::new(document).with_view(&configuration);
        SvgDocumentBuilder {
            writer: SvgXmlWriter::new(out, model.clone(), configuration.clone()),
            line_type_writers: HashMap::new(),
            model,
            configuration,
        }
    }

    /// Sets the layout state that the C# class reads through its `Layout` property (which it never assigns).
    pub fn set_layout(&mut self, layout: Option<LayoutView>) {
        self.writer.set_layout_state(layout);
    }

    /// `WriteLayout(layout)` of the builder.
    pub fn write_layout(&mut self, layout: LayoutView, block: &'a BlockRecord) -> Result<(), SvgError> {
        self.writer.set_units(paper_units(layout.paper_units));
        let (mut paper_width, mut paper_height) = (layout.paper_width, layout.paper_height);
        if matches!(layout.paper_rotation, 1 | 3) {
            paper_width = layout.paper_height;
            paper_height = layout.paper_width;
        }
        let paper = BoundingBox::from_min_max(Xyz::ZERO, Xyz::new(paper_width, paper_height, 0.0));
        // The C# code reads `this.Layout.UnprintableMargin`, and the builder never assigns `Layout`.
        if !self.writer.has_layout() {
            return Err(SvgError::null_reference("Object reference not set to an instance of an object."));
        }
        let lower_margin = Xyz::new(layout.margin_left, layout.margin_bottom, 0.0);
        self.start_document(paper)?;
        let units = self.writer.units();
        let translation = xyz_to_pixel(lower_margin, units)?;
        let transform = Transform::from_trs(translation, Xyz::splat(layout.print_scale), Xyz::ZERO)?;
        for e in self.model.entities_of(block) {
            self.writer.write_entity(e, &transform)?;
        }
        self.end_document()
    }

    /// `writeLineType(lineType)`: registers an (empty) `defs` writer for the line type name.
    pub fn write_line_type(&mut self, name: &str) -> Result<(), SvgError> {
        if self.line_type_writers.contains_key(name) {
            return Ok(());
        }
        let mut writer = SvgXmlWriter::new(Vec::new(), self.model.clone(), self.configuration.clone());
        writer.xml.write_start_element("defs")?;
        writer.xml.write_end_element()?;
        self.line_type_writers.insert(name.to_owned(), writer);
        Ok(())
    }

    fn start_document(&mut self, box_: BoundingBox) -> Result<(), SvgError> {
        let units = self.writer.units();
        let xml = &mut self.writer.xml;
        xml.write_start_document()?;
        xml.write_start_element("svg")?;
        xml.write_attribute_string("xmlns", "http://www.w3.org/2000/svg")?;
        xml.write_attribute_string("width", &to_svg_units(box_.max.x - box_.min.x, units))?;
        xml.write_attribute_string("height", &to_svg_units(box_.max.y - box_.min.y, units))?;
        xml.write_start_attribute("viewBox")?;
        xml.write_value_str(&to_svg_units(box_.min.x, units))?;
        xml.write_value_str(" ")?;
        xml.write_value_str(&to_svg_units(box_.min.y, units))?;
        xml.write_value_str(" ")?;
        xml.write_value_str(&to_svg_units(box_.max.x - box_.min.x, units))?;
        xml.write_value_str(" ")?;
        xml.write_value_str(&to_svg_units(box_.max.y - box_.min.y, units))?;
        xml.write_end_attribute()?;
        xml.write_attribute_string("transform", "scale(1,-1)")?;
        if self.writer.has_layout() {
            self.writer.xml.write_attribute_string("style", "background-color:white")?;
        }
        Ok(())
    }

    fn end_document(&mut self) -> Result<(), SvgError> {
        self.writer.xml.write_end_element()?;
        self.writer.xml.write_end_document()?;
        self.writer.xml.close()
    }

    /// `Units` of the builder's writer, for callers that need the unit state.
    pub fn units(&self) -> UnitsType {
        self.writer.units()
    }
}
