// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/IO/SVG/SvgXmlWriter.cs) by Albert Domenech.
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

//! The SVG element writer (`SvgXmlWriter`): turns block records and layouts into SVG.

use super::configuration::{line_weight_mm, SvgConfiguration};
use super::converter::{
    double_to_string, to_pixel_size, to_svg, to_svg_units, xy_to_svg, xyz_to_pixel, UnitsType,
};
use super::csmath::{rad_to_deg, BoundingBox, Transform, Xyz};
use super::error::SvgError;
use super::geometry::EllipseGeometry;
use super::model::{Item, LayoutView, Model};
use super::xml_text_writer::{Formatting, XmlTextWriter};
use crate::entities::{
    AttachmentPoint, Dimension, EntityCommon, EntityType, Hatch, TextHorizontalAlignment,
    TextVerticalAlignment,
};
use crate::tables::{BlockRecord, LineType};
use crate::types::Color;
use std::f64::consts::PI;
use std::io::Write;

/// The kind of a writer notification (`NotificationType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgNotificationKind {
    NotImplemented,
}

/// A notification raised while writing (ACadSharp's `OnNotification` event).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvgNotification {
    pub message: String,
    pub kind: SvgNotificationKind,
}

/// Nesting limit for inserts (ACadSharp would overflow the stack on a block that contains itself).
const MAX_NESTING: usize = 256;

/// ACadSharp `SvgXmlWriter`.
pub struct SvgXmlWriter<'a, W: Write> {
    pub(crate) xml: XmlTextWriter<W>,
    model: Model<'a>,
    configuration: SvgConfiguration,
    layout: Option<LayoutView>,
    units: UnitsType,
    notifications: Vec<SvgNotification>,
    nesting: usize,
    /// The numbers that stand for ACadSharp's `object.GetHashCode()` in the pattern ids: one per hatch pattern and
    /// pattern line object (by address), numbered in order of first use, so a hatch written twice (a block inserted
    /// twice) repeats its ids as in C# and the output is the same from run to run.
    identity_hashes: std::collections::HashMap<usize, usize>,
}

impl<'a, W: Write> SvgXmlWriter<'a, W> {
    pub fn new(out: W, model: Model<'a>, configuration: SvgConfiguration) -> Self {
        let mut xml = XmlTextWriter::new(out);
        xml.formatting = Formatting::Indented;
        SvgXmlWriter {
            xml,
            model: model.with_view(&configuration),
            configuration,
            layout: None,
            units: UnitsType::UNITLESS,
            notifications: Vec::new(),
            nesting: 0,
            identity_hashes: std::collections::HashMap::new(),
        }
    }

    pub fn notifications(&self) -> &[SvgNotification] {
        &self.notifications
    }

    pub fn into_inner(self) -> W {
        self.xml.into_inner()
    }

    fn notify(&mut self, message: String) {
        self.notifications.push(SvgNotification {
            message,
            kind: SvgNotificationKind::NotImplemented,
        });
    }

    pub(crate) fn set_layout_state(&mut self, layout: Option<LayoutView>) {
        self.layout = layout;
    }

    pub(crate) fn set_units(&mut self, units: UnitsType) {
        self.units = units;
    }

    pub(crate) fn units(&self) -> UnitsType {
        self.units
    }

    pub(crate) fn has_layout(&self) -> bool {
        self.layout.is_some()
    }

    fn write_attr_f64(&mut self, name: &str, value: f64) -> Result<(), SvgError> {
        let units = self.units;
        self.write_attr_f64_units(name, value, units)
    }

    fn write_attr_f64_units(&mut self, name: &str, value: f64, units: UnitsType) -> Result<(), SvgError> {
        self.xml.write_attribute_string(name, &to_svg_units(value, units))
    }

    /// `WriteBlock(record)`.
    pub fn write_block(&mut self, record: &'a BlockRecord) -> Result<(), SvgError> {
        self.units = UnitsType(self.model.record_units(record));
        let box_ = self.model.record_bounding_box(record)?;
        let units = self.units;
        self.start_document(box_, Some(box_), units)?;
        self.write_items(record, None)?;
        self.end_document()
    }

    /// `WriteLayout(layout)`.
    pub fn write_layout(
        &mut self,
        layout: LayoutView,
        block: &'a BlockRecord,
    ) -> Result<(), SvgError> {
        self.layout = Some(layout);
        self.units = paper_units(layout.paper_units);
        let (mut paper_width, mut paper_height) = (layout.paper_width, layout.paper_height);
        if matches!(layout.paper_rotation, 1 | 3) {
            paper_width = layout.paper_height;
            paper_height = layout.paper_width;
        }
        let upper_corner = Xyz::new(paper_width, paper_height, 0.0);
        let paper = BoundingBox::from_min_max(Xyz::ZERO, upper_corner);
        let lower_margin = Xyz::new(layout.margin_left, layout.margin_bottom, 0.0);

        self.start_document(paper, None, UnitsType::MILLIMETERS)?;

        let translation = xyz_to_pixel(lower_margin, UnitsType::MILLIMETERS)?;
        let transform = Transform::from_trs(translation, Xyz::splat(layout.print_scale), Xyz::ZERO)?;
        self.write_items(block, Some(&transform))?;
        self.end_document()
    }

    fn start_document(
        &mut self,
        box_: BoundingBox,
        view_box: Option<BoundingBox>,
        units: UnitsType,
    ) -> Result<(), SvgError> {
        self.xml.write_start_document()?;
        self.xml.write_start_element("svg")?;
        self.xml.write_attribute_string("xmlns", "http://www.w3.org/2000/svg")?;
        self.write_attr_f64_units("width", box_.max.x - box_.min.x, units)?;
        self.write_attr_f64_units("height", box_.max.y - box_.min.y, units)?;
        if let Some(vb) = view_box {
            self.xml.write_start_attribute("viewBox")?;
            self.xml.write_value_f64(to_pixel_size(vb.min.x, units)?)?;
            self.xml.write_value_str(" ")?;
            self.xml.write_value_f64(to_pixel_size(vb.min.y, units)?)?;
            self.xml.write_value_str(" ")?;
            self.xml.write_value_f64(to_pixel_size(vb.length_x(), units)?)?;
            self.xml.write_value_str(" ")?;
            self.xml.write_value_f64(to_pixel_size(vb.length_y(), units)?)?;
            self.xml.write_end_attribute()?;
        }
        self.xml.write_attribute_string("transform", "scale(1,-1)")?;
        if self.layout.is_some() {
            self.xml.write_attribute_string("style", "background-color:white")?;
        }
        Ok(())
    }

    fn end_document(&mut self) -> Result<(), SvgError> {
        self.xml.write_end_element()?;
        self.xml.write_end_document()?;
        self.xml.close()
    }

    fn color_svg(&self, color: Color) -> String {
        let color = if self.layout.is_some() && color == Color::Index(7) {
            Color::Index(250)
        } else {
            color
        };
        let (r, g, b) = self.color_rgb(color);
        format!("rgb({r},{g},{b})")
    }

    /// `Color.R`, `G`, `B`: the stored true colour, or the RGB of the index (ACadSharp's palette in the view, the
    /// fork's ACI table otherwise). An unresolved ByLayer, ByBlock or ByEntity colour prints black natively; C#
    /// truncates the index to a byte (256 and 0 are entry 0, black; 257 is entry 1, red).
    fn color_rgb(&self, color: Color) -> (u8, u8, u8) {
        #[cfg(feature = "acadsharp-compat")]
        if self.model.view {
            let index = match color {
                Color::Rgb { r, g, b } => return (r, g, b),
                Color::ByLayer | Color::ByBlock => 0,
                Color::None => 1,
                Color::Index(i) => usize::from(i),
            };
            let [r, g, b] = crate::compat::acadsharp::view::INDEX_RGB[index];
            return (r, g, b);
        }
        color.rgb().unwrap_or((0, 0, 0))
    }

    /// The members of a block record's collection, each written with `transform` (or the identity).
    fn write_items(&mut self, record: &'a BlockRecord, transform: Option<&Transform>) -> Result<(), SvgError> {
        let identity = Transform::new();
        let transform = transform.unwrap_or(&identity);
        for item in self.model.items_of(record) {
            match item {
                Item::Entity(e) => self.write_entity(e, transform)?,
                Item::Attribute(a) => {
                    self.xml.write_comment(&format!("ATTRIB | {}", a.common.handle.value()))?;
                    let view = self.model.attribute_view(a);
                    self.write_text_view(&a.common, view, None, transform)?;
                }
                Item::Seqend(s) => {
                    self.xml.write_comment(&format!("SEQEND | {}", s.common.handle.value()))?;
                    self.notify("[SEQEND] Entity not implemented.".to_owned());
                }
            }
        }
        Ok(())
    }

    /// `writeEntity(entity, transform)`.
    pub fn write_entity(&mut self, e: &'a EntityType, transform: &Transform) -> Result<(), SvgError> {
        self.xml.write_comment(&format!(
            "{} | {}",
            Model::object_name(e),
            e.common().handle.value()
        ))?;
        match e {
            EntityType::Arc(a) => self.write_arc(a, transform),
            EntityType::Dimension(d) => self.write_dimension(d, transform),
            EntityType::Line(l) => self.write_line(l, transform),
            EntityType::Point(p) => self.write_point(p, transform),
            EntityType::Circle(c) => self.write_circle(c, transform),
            EntityType::Ellipse(el) => self.write_ellipse(el, transform),
            EntityType::Hatch(h) => self.write_hatch(h, transform),
            EntityType::Insert(_) | EntityType::Table(_) => self.write_insert(e, transform),
            EntityType::LwPolyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
            | EntityType::Polyline(_)
            | EntityType::PolyfaceMesh(_)
            | EntityType::PolygonMesh(_) => self.write_polyline(e, transform),
            EntityType::Text(_)
            | EntityType::MText(_)
            | EntityType::AttributeDefinition(_)
            | EntityType::AttributeEntity(_) => self.write_text(e, transform),
            EntityType::Solid(s) => self.write_solid(s, transform),
            other => {
                let name = Model::object_name(other);
                self.notify(format!("[{name}] Entity not implemented."));
                Ok(())
            }
        }
    }

    fn drawable_line_type(lt: Option<&LineType>) -> bool {
        match lt {
            Some(lt) => {
                !lt.elements.is_empty()
                    && !lt
                        .elements
                        .iter()
                        .any(|s| s.complex.as_ref().is_some_and(|c| c.is_shape()))
            }
            None => false,
        }
    }

    fn point_size(&self, common: &EntityCommon) -> Result<f64, SvgError> {
        to_pixel_size(line_weight_mm(self.model.active_line_weight(common)), self.units)
    }

    fn write_entity_header(
        &mut self,
        common: &EntityCommon,
        transform: &Transform,
        draw_stroke: bool,
    ) -> Result<(), SvgError> {
        let color = self.model.active_color(common);
        self.xml.write_attribute_string("vector-effect", "non-scaling-stroke")?;
        let stroke = if draw_stroke { self.color_svg(color) } else { "none".to_owned() };
        self.xml.write_attribute_string("stroke", &stroke)?;
        let weight = self.model.active_line_weight(common);
        let width = to_svg_units(
            self.configuration.line_weight_value(weight, self.units),
            UnitsType::MILLIMETERS,
        );
        self.xml.write_attribute_string("stroke-width", &width)?;
        self.write_transform_of(transform)?;
        let lt = self.model.active_line_type(common);
        if Self::drawable_line_type(lt) {
            let point_size = self.point_size(common)?;
            self.write_dashes_line_type(lt.expect("drawable"), point_size)?;
        }
        Ok(())
    }

    fn write_dashes_line_type(&mut self, lt: &LineType, point_size: f64) -> Result<(), SvgError> {
        let mut sb = String::new();
        for segment in &lt.elements {
            if segment.length == 0.0 {
                sb.push_str(&double_to_string(to_pixel_size(point_size, self.units)?));
            } else {
                sb.push_str(&double_to_string(to_pixel_size(segment.length, self.units)?.abs()));
            }
            sb.push(' ');
        }
        self.xml.write_attribute_string("stroke-dasharray", sb.trim())
    }

    fn write_dashes(&mut self, dashes: &[f64]) -> Result<(), SvgError> {
        let mut sb = String::new();
        for d in dashes {
            sb.push_str(&double_to_string(to_pixel_size(*d, self.units)?.abs()));
            sb.push(' ');
        }
        self.xml.write_attribute_string("stroke-dasharray", sb.trim())
    }

    fn svg_points(&self, points: &[Xyz]) -> Result<String, SvgError> {
        let mut sb = String::new();
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                sb.push(' ');
            }
            let px = xyz_to_pixel(*p, self.units)?;
            sb.push_str(&xy_to_svg(px.x, px.y));
        }
        Ok(sb)
    }

    fn pixel_pair(&self, p: Xyz) -> Result<String, SvgError> {
        let px = xyz_to_pixel(p, self.units)?;
        Ok(xy_to_svg(px.x, px.y))
    }

    fn write_arc(&mut self, arc: &'a crate::entities::Arc, transform: &Transform) -> Result<(), SvgError> {
        self.xml.write_start_element("path")?;
        self.write_entity_header(&arc.common, transform, true)?;
        let geometry = Model::arc_geometry(arc);
        let (start, end) = geometry.end_vertices();
        let large_arc = if geometry.sweep().abs() > PI { 1 } else { 0 };
        let radius = double_to_string(to_pixel_size(geometry.radius, self.units)?);
        let mut sb = String::new();
        sb.push_str(&format!("M {}", self.pixel_pair(start)?));
        sb.push(' ');
        sb.push_str(&format!("A {radius} {radius}"));
        sb.push(' ');
        sb.push_str(&format!("{} {} {} {}", 0, large_arc, 1, self.pixel_pair(end)?));
        self.xml.write_attribute_string("d", &sb)?;
        self.xml.write_attribute_string("fill", "none")?;
        self.xml.write_end_element()
    }

    fn write_dimension(&mut self, dimension: &'a Dimension, transform: &Transform) -> Result<(), SvgError> {
        self.xml.write_start_element("g")?;
        let name = &dimension.base().block_name;
        let record = self
            .model
            .block_by_name(name)
            .ok_or_else(|| SvgError::null_reference("Object reference not set to an instance of an object."))?;
        self.enter()?;
        self.write_items(record, Some(transform))?;
        self.nesting -= 1;
        self.xml.write_end_element()
    }

    fn enter(&mut self) -> Result<(), SvgError> {
        if self.nesting >= MAX_NESTING {
            return Err(SvgError::invalid_operation("Block references nest too deeply."));
        }
        self.nesting += 1;
        Ok(())
    }

    fn write_circle(&mut self, circle: &'a crate::entities::Circle, transform: &Transform) -> Result<(), SvgError> {
        let loc = transform.apply_transform(super::model::v3(circle.center));
        self.xml.write_start_element("circle")?;
        self.write_entity_header(&circle.common, transform, true)?;
        self.write_attr_f64("r", circle.radius)?;
        self.write_attr_f64("cx", loc.x)?;
        self.write_attr_f64("cy", loc.y)?;
        self.xml.write_attribute_string("fill", "none")?;
        self.xml.write_end_element()
    }

    fn write_ellipse(&mut self, ellipse: &'a crate::entities::Ellipse, transform: &Transform) -> Result<(), SvgError> {
        let g: EllipseGeometry = Model::ellipse_geometry(ellipse);
        if g.is_full_ellipse() {
            self.xml.write_start_element("path")?;
            self.write_entity_header(&ellipse.common, transform, true)?;
            let start = g.polar_coordinate_relative_to_center(0.0);
            let end = g.polar_coordinate_relative_to_center(PI);
            let rx = double_to_string(g.major_axis() / 2.0);
            let ry = double_to_string(g.minor_axis() / 2.0);
            let rot = double_to_string(rad_to_deg(g.rotation()));
            let mut sb = String::new();
            sb.push_str(&format!("M {} ", self.pixel_pair(start)?));
            sb.push_str(&format!("A {rx} {ry} {rot} {} {} {} ", 0, 1, self.pixel_pair(end)?));
            let end = g.polar_coordinate_relative_to_center(super::csmath::TWO_PI);
            sb.push_str(&format!("A {rx} {ry} {rot} {} {} {}", 0, 1, self.pixel_pair(end)?));
            self.xml.write_attribute_string("d", &sb)?;
            self.xml.write_attribute_string("fill", "none")?;
            self.xml.write_end_element()
        } else {
            self.xml.write_start_element("polyline")?;
            self.write_entity_header(&ellipse.common, transform, true)?;
            let vertices = g.polygonal_vertexes(256)?;
            let pts = self.svg_points(&vertices)?;
            self.xml.write_attribute_string("points", &pts)?;
            self.xml.write_attribute_string("fill", "none")?;
            self.xml.write_end_element()
        }
    }

    fn create_path(&self, polylines: &[Vec<Xyz>]) -> Result<String, SvgError> {
        let mut sb = String::new();
        for pts in polylines {
            if pts.is_empty() {
                continue;
            }
            sb.push_str(&format!("M {} ", self.pixel_pair(pts[0])?));
            for p in &pts[1..] {
                sb.push_str(&format!("L {} ", self.pixel_pair(*p)?));
            }
        }
        Ok(sb)
    }

    fn write_hatch(&mut self, hatch: &'a Hatch, transform: &Transform) -> Result<(), SvgError> {
        self.xml.write_start_element("g")?;
        let pattern_id = self.write_pattern(hatch)?;
        let mut plines: Vec<Vec<Xyz>> = Vec::new();
        for path in &hatch.paths {
            plines.push(self.model.hatch_path_points(path, self.configuration.arc_points)?);
        }
        self.xml.write_start_element("path")?;
        self.write_entity_header(&hatch.common, transform, false)?;
        let d = self.create_path(&plines)?;
        self.xml.write_attribute_string("d", &d)?;
        self.xml.write_attribute_string("fill", &format!("url(#{pattern_id})"))?;
        self.xml.write_end_element()?;
        self.xml.write_end_element()
    }

    /// `object.GetHashCode()` of `object`, which is told apart by its address.
    fn identity_hash<T>(&mut self, object: &T) -> usize {
        let next = self.identity_hashes.len() + 1;
        *self.identity_hashes.entry(object as *const T as usize).or_insert(next)
    }

    fn write_pattern_header_id(&mut self, id: &str) -> Result<(), SvgError> {
        self.xml.write_start_element("pattern")?;
        self.xml.write_attribute_string("id", id)?;
        self.xml.write_attribute_string("patternUnits", "userSpaceOnUse")
    }

    fn write_pattern_header(&mut self, hatch: &Hatch) -> Result<String, SvgError> {
        let id = format!("{}_{}", self.identity_hash(&hatch.pattern), hatch.pattern.name);
        self.write_pattern_header_id(&id)?;
        Ok(id)
    }

    fn write_solid_pattern(&mut self, hatch: &Hatch) -> Result<String, SvgError> {
        let id = self.write_pattern_header(hatch)?;
        self.xml.write_attribute_string("width", "100%")?;
        self.xml.write_attribute_string("height", "100%")?;
        self.xml.write_start_element("rect")?;
        self.xml.write_attribute_string("width", "100%")?;
        self.xml.write_attribute_string("height", "100%")?;
        // `hatch.Color`, not the active colour.
        let fill = self.color_svg(self.model.own_color(&hatch.common));
        self.xml.write_attribute_string("fill", &fill)?;
        self.xml.write_end_element()?;
        self.xml.write_end_element()?;
        Ok(id)
    }

    fn write_pattern(&mut self, hatch: &Hatch) -> Result<String, SvgError> {
        if hatch.is_solid {
            return self.write_solid_pattern(hatch);
        }
        let mut patterns: Vec<(String, BoundingBox)> = Vec::new();
        for item in &hatch.pattern.lines {
            let id = format!("{}_line", self.identity_hash(item));
            let line_offset = {
                let cos = (0.0 - item.angle).cos();
                let sin = (0.0 - item.angle).sin();
                item.offset.x * sin + item.offset.y * cos
            };
            patterns.push((
                id.clone(),
                BoundingBox::from_min_max(Xyz::ZERO, Xyz::new(line_offset, line_offset, 0.0)),
            ));
            self.write_pattern_header_id(&id)?;
            let units = self.units;
            self.xml.write_attribute_string("width", &to_svg_units(line_offset, units))?;
            self.xml.write_attribute_string("height", &to_svg_units(line_offset, units))?;
            let base = xyz_to_pixel(Xyz::new(item.base_point.x, item.base_point.y, 0.0), units)?;
            self.write_transform("patternTransform", Some(base), None, None)?;
            self.xml.write_start_element("line")?;
            let x = super::csmath::cos(item.angle) * 10.0;
            let y = super::csmath::sin(item.angle) * 10.0;
            self.xml.write_attribute_string("x1", &to_svg_units(0.0, units))?;
            self.xml.write_attribute_string("y1", &to_svg_units(0.0, units))?;
            self.xml.write_attribute_string("x2", &to_svg_units(x, units))?;
            self.xml.write_attribute_string("y2", &to_svg_units(y, units))?;
            let stroke = self.color_svg(self.model.active_color(&hatch.common));
            self.xml.write_attribute_string("stroke", &stroke)?;
            let weight = self.model.active_line_weight(&hatch.common);
            let width = to_svg_units(
                self.configuration.line_weight_value(weight, units),
                UnitsType::MILLIMETERS,
            );
            self.xml.write_attribute_string("stroke-width", &width)?;
            if !item.dash_lengths.is_empty() {
                self.write_dashes(&item.dash_lengths)?;
            }
            self.xml.write_end_element()?;
            self.xml.write_end_element()?;
        }
        let id = self.write_pattern_header(hatch)?;
        if patterns.is_empty() {
            return Err(SvgError::invalid_operation("Sequence contains no elements"));
        }
        let width = patterns.iter().map(|p| p.1.length_x()).fold(f64::NEG_INFINITY, super::csmath::dmax);
        let height = patterns.iter().map(|p| p.1.length_y()).fold(f64::NEG_INFINITY, super::csmath::dmax);
        let units = self.units;
        self.xml.write_attribute_string("width", &to_svg_units(width, units))?;
        self.xml.write_attribute_string("height", &to_svg_units(height, units))?;
        for (key, b) in &patterns {
            self.xml.write_start_element("rect")?;
            self.xml.write_attribute_string("width", &to_svg_units(b.length_x(), units))?;
            self.xml.write_attribute_string("height", &to_svg_units(b.length_y(), units))?;
            self.xml.write_attribute_string("fill", &format!("url(#{key})"))?;
            self.xml.write_end_element()?;
        }
        self.xml.write_end_element()?;
        Ok(id)
    }

    fn write_insert(&mut self, e: &'a EntityType, transform: &Transform) -> Result<(), SvgError> {
        let view = self.model.insert_view(e).expect("insert view");
        let insert_transform = self.model.insert_transform(&view);
        let merged = Transform::from_matrix(transform.matrix().mul(insert_transform.matrix()));
        self.xml.write_start_element("g")?;
        self.write_transform_of(&merged)?;
        let record = view
            .record
            .ok_or_else(|| SvgError::null_reference("Object reference not set to an instance of an object."))?;
        self.enter()?;
        self.write_items(record, None)?;
        self.nesting -= 1;
        self.xml.write_end_element()
    }

    fn write_line(&mut self, line: &'a crate::entities::Line, transform: &Transform) -> Result<(), SvgError> {
        self.xml.write_start_element("line")?;
        self.write_entity_header(&line.common, transform, true)?;
        let u = self.units;
        self.xml.write_attribute_string("x1", &to_svg_units(line.start.x, u))?;
        self.xml.write_attribute_string("y1", &to_svg_units(line.start.y, u))?;
        self.xml.write_attribute_string("x2", &to_svg_units(line.end.x, u))?;
        self.xml.write_attribute_string("y2", &to_svg_units(line.end.y, u))?;
        self.xml.write_end_element()
    }

    fn write_point(&mut self, point: &'a crate::entities::Point, transform: &Transform) -> Result<(), SvgError> {
        self.xml.write_start_element("circle")?;
        self.write_entity_header(&point.common, transform, true)?;
        self.write_attr_f64_units("r", self.configuration.point_radius, UnitsType::UNITLESS)?;
        self.write_attr_f64("cx", point.location.x)?;
        self.write_attr_f64("cy", point.location.y)?;
        let fill = self.color_svg(self.model.active_color(&point.common));
        self.xml.write_attribute_string("fill", &fill)?;
        self.xml.write_end_element()
    }

    fn write_polyline(&mut self, e: &'a EntityType, transform: &Transform) -> Result<(), SvgError> {
        let kind = Model::polyline_kind(e).expect("polyline entity");
        let closed = Model::polyline_closed(&kind);
        self.xml.write_start_element(if closed { "polygon" } else { "polyline" })?;
        self.write_entity_header(e.common(), transform, true)?;
        let points = self.model.polyline_xy_points(&kind, self.configuration.arc_points)?;
        let pts = self.svg_points(&points)?;
        self.xml.write_attribute_string("points", &pts)?;
        self.xml.write_attribute_string("fill", "none")?;
        self.xml.write_end_element()
    }

    fn write_solid(&mut self, solid: &'a crate::entities::Solid, transform: &Transform) -> Result<(), SvgError> {
        use super::model::v3;
        self.xml.write_start_element("polygon")?;
        self.write_entity_header(&solid.common, transform, true)?;
        let pts = self.svg_points(&[
            v3(solid.first_corner),
            v3(solid.second_corner),
            v3(solid.third_corner),
            v3(solid.fourth_corner),
        ])?;
        self.xml.write_attribute_string("points", &pts)?;
        let fill = self.color_svg(self.model.active_color(&solid.common));
        self.xml.write_attribute_string("fill", &fill)?;
        self.xml.write_end_element()
    }

    fn write_text(&mut self, e: &'a EntityType, transform: &Transform) -> Result<(), SvgError> {
        let t = self.model.text_view(e);
        let attachment = match e {
            EntityType::MText(m) => Some(m.attachment_point),
            _ => None,
        };
        self.write_text_view(e.common(), t, attachment, transform)
    }

    fn write_text_view(
        &mut self,
        common: &EntityCommon,
        t: super::model::TextView<'a>,
        attachment: Option<AttachmentPoint>,
        transform: &Transform,
    ) -> Result<(), SvgError> {
        let insert = if t.is_line_text
            && (t.horizontal != TextHorizontalAlignment::Left || t.vertical != TextVerticalAlignment::Baseline)
            && !(t.horizontal == TextHorizontalAlignment::Fit || t.horizontal == TextHorizontalAlignment::Aligned)
        {
            t.alignment_point
        } else {
            t.insert_point
        };
        self.xml.write_start_element("g")?;
        self.write_transform_of(transform)?;
        self.xml.write_start_element("text")?;
        let translation = xyz_to_pixel(insert, self.units)?;
        self.write_transform(
            "transform",
            Some(translation),
            Some(Xyz::new(1.0, -1.0, 0.0)),
            if t.rotation != 0.0 { Some(t.rotation) } else { None },
        )?;
        let fill = self.color_svg(self.model.active_color(common));
        self.xml.write_attribute_string("fill", &fill)?;
        self.xml.write_start_attribute("style")?;
        self.xml.write_value_str("font:")?;
        self.xml.write_value_str(&to_svg_units(t.height, self.units))?;
        if self.units == UnitsType::UNITLESS {
            self.xml.write_value_str("px")?;
        }
        let style = self.model.text_style(t.style_name);
        if style.map_or(0, |s| s.font_flags) & 2 != 0 {
            self.xml.write_value_str("bold")?;
        }
        if style.map_or(0, |s| s.font_flags) & 1 != 0 {
            self.xml.write_value_str("italic")?;
        }
        self.xml.write_value_str(" ")?;
        let filename = style.map(|s| s.font_file.as_str()).unwrap_or("");
        self.xml.write_value_str(&file_name_without_extension(filename))?;
        self.xml.write_end_attribute()?;

        match attachment {
            Some(attachment_point) => {
                let (baseline, anchor) = match attachment_point {
                    AttachmentPoint::TopLeft => ("hanging", "start"),
                    AttachmentPoint::TopCenter => ("hanging", "middle"),
                    AttachmentPoint::TopRight => ("hanging", "end"),
                    AttachmentPoint::MiddleLeft => ("middle", "start"),
                    AttachmentPoint::MiddleCenter => ("middle", "middle"),
                    AttachmentPoint::MiddleRight => ("middle", "end"),
                    AttachmentPoint::BottomLeft => ("baseline", "start"),
                    AttachmentPoint::BottomCenter => ("baseline", "middle"),
                    AttachmentPoint::BottomRight => ("baseline", "end"),
                };
                self.xml.write_attribute_string("alignment-baseline", baseline)?;
                self.xml.write_attribute_string("text-anchor", anchor)?;
                let plain = self.model.plain_text(&t.value).ok_or_else(|| {
                    SvgError::out_of_range("Index and length must refer to a location within the string.")
                })?;
                for line in split_plain_lines(&plain) {
                    self.xml.write_start_element("tspan")?;
                    self.write_attr_f64("x", 0.0)?;
                    self.xml.write_attribute_string("dy", "1em")?;
                    self.xml.write_string(line)?;
                    self.xml.write_end_element()?;
                }
                self.xml.write_start_element("tspan")?;
                self.write_attr_f64("x", 0.0)?;
                self.xml.write_attribute_string("dy", "1em")?;
                self.xml.write_attribute_string("visibility", "hidden")?;
                self.xml.write_string(".")?;
                self.xml.write_end_element()?;
            }
            None => {
                match t.horizontal {
                    TextHorizontalAlignment::Left => self.xml.write_attribute_string("text-anchor", "start")?,
                    TextHorizontalAlignment::Middle | TextHorizontalAlignment::Center => {
                        self.xml.write_attribute_string("text-anchor", "middle")?
                    }
                    TextHorizontalAlignment::Right => self.xml.write_attribute_string("text-anchor", "end")?,
                    _ => {}
                }
                match t.vertical {
                    TextVerticalAlignment::Baseline | TextVerticalAlignment::Bottom => {
                        self.xml.write_attribute_string("alignment-baseline", "baseline")?
                    }
                    TextVerticalAlignment::Middle => self.xml.write_attribute_string("alignment-baseline", "middle")?,
                    TextVerticalAlignment::Top => self.xml.write_attribute_string("alignment-baseline", "hanging")?,
                }
                self.xml.write_string(&t.value)?;
            }
        }
        self.xml.write_end_element()?;
        self.xml.write_end_element()
    }

    fn write_transform_of(&mut self, transform: &Transform) -> Result<(), SvgError> {
        let t = transform.translation();
        let translation = if t != Xyz::ZERO { Some(t) } else { None };
        let s = transform.scale();
        let scale = if s != Xyz::splat(1.0) { Some(s) } else { None };
        let z = transform.euler_rotation().z;
        let rotation = if z != 0.0 { Some(z) } else { None };
        self.write_transform("transform", translation, scale, rotation)
    }

    fn write_transform(
        &mut self,
        name: &str,
        translation: Option<Xyz>,
        scale: Option<Xyz>,
        rotation: Option<f64>,
    ) -> Result<(), SvgError> {
        let mut sb = String::new();
        if let Some(t) = translation {
            sb.push_str(&format!("translate({},{})", to_svg(t.x), to_svg(t.y)));
        }
        if let Some(s) = scale {
            sb.push_str(&format!("scale({},{})", to_svg(s.x), to_svg(s.y)));
        }
        if let Some(r) = rotation {
            let r = -rad_to_deg(r);
            sb.push_str(&format!("rotate({})", to_svg(r)));
        }
        if sb.is_empty() {
            return Ok(());
        }
        self.xml.write_attribute_string(name, &sb)
    }
}

/// `PlotPaperUnits.ToUnits()`.
pub(crate) fn paper_units(units: i16) -> UnitsType {
    match units {
        0 => UnitsType::INCHES,
        1 => UnitsType::MILLIMETERS,
        _ => UnitsType::UNITLESS,
    }
}

/// `Path.GetFileNameWithoutExtension` with `/` as the directory separator.
fn file_name_without_extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or("");
    match name.rfind('.') {
        Some(i) => name[..i].to_owned(),
        None => name.to_owned(),
    }
}

/// `string.Split(new[] { "\r\n", "\r", "\n", "\\P" }, StringSplitOptions.None)`.
fn split_plain_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let sep = if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
            2
        } else if bytes[i] == b'\r' || bytes[i] == b'\n' {
            1
        } else if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'P') {
            2
        } else {
            0
        };
        if sep > 0 {
            lines.push(&text[start..i]);
            i += sep;
            start = i;
        } else {
            i += 1;
        }
    }
    lines.push(&text[start..]);
    lines
}
