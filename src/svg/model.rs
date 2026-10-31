// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/{Entities,Tables,Objects}/*.cs) by Albert Domenech.
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

//! Access to the fork's document the way ACadSharp's SVG writer sees it: the entities of a block
//! record, the active colour, line type and line weight, the object names and the bounding boxes.

use super::csmath::{BoundingBox, Extent, Matrix4, Transform, Xy, Xyz};
use super::error::SvgError;
use super::geometry::{
    polyline_points_with_arcs, ArcGeometry, EllipseGeometry, SplineGeometry,
    VertexPoint,
};
use crate::document::CadDocument;
use crate::entities::{
    BoundaryEdge, BoundaryPath, Dimension, EntityCommon, EntityType, TextHorizontalAlignment,
    TextVerticalAlignment,
};
use crate::objects::{Layout, ObjectType};
use crate::tables::{BlockRecord, LineType, TextStyle};
use crate::types::{Color, Vector2, Vector3};

pub(crate) fn v3(v: Vector3) -> Xyz {
    Xyz::new(v.x, v.y, v.z)
}

pub(crate) fn v2(v: Vector2) -> Xy {
    Xy::new(v.x, v.y)
}

/// The colour of an entity as ACadSharp reads it from a source file. A colour-book colour of a DWG leaves
/// `Color` ByBlock (the reference is kept apart), and ACadSharp's ENC reader treats every other complex colour
/// as RGB, including the method-encoded ByLayer and ACI values.
pub(crate) fn acadsharp_entity_color(common: &EntityCommon) -> Color {
    let mut value = common.color;
    if let Some((word, _)) = common.dwg_color_source {
        if word & 0x4000 != 0 {
            value = Color::ByBlock;
        } else if word & 0x8000 != 0 {
            value = match value {
                Color::ByLayer | Color::ByBlock => Color::from_rgb(0, 0, 0),
                Color::Index(i) => Color::from_rgb(0, 0, i),
                other => other,
            };
        }
    }
    value
}

/// A recursion guard for block references that contain themselves (ACadSharp would overflow the stack).
const MAX_DEPTH: usize = 256;

/// The document plus the lookups the writer needs.
#[derive(Clone)]
pub struct Model<'a> {
    pub doc: &'a CadDocument,
    /// Read the document as ACadSharp does (`SvgConfiguration::acadsharp_view`).
    pub view: bool,
    cache: std::rc::Rc<ModelCache<'a>>,
}

/// Lookups built on first use.
#[derive(Default)]
struct ModelCache<'a> {
    /// The BLOCK marker entity of each block record, by record handle.
    markers: std::cell::OnceCell<std::collections::HashMap<crate::types::Handle, &'a EntityType>>,
}

/// The pieces of a layout (a `Layout` / `PlotSettings`) that the writer reads.
#[derive(Debug, Clone, Copy)]
pub struct LayoutView {
    pub paper_width: f64,
    pub paper_height: f64,
    /// `PlotRotation`: 0 none, 1 90 degrees, 2 180, 3 270.
    pub paper_rotation: i16,
    /// `PlotPaperUnits`: 0 inches, 1 millimeters, 2 pixels.
    pub paper_units: i16,
    pub margin_left: f64,
    pub margin_bottom: f64,
    pub margin_right: f64,
    pub margin_top: f64,
    pub print_scale: f64,
}

impl LayoutView {
    pub fn of(layout: &Layout) -> LayoutView {
        LayoutView {
            paper_width: layout.paper_width,
            paper_height: layout.paper_height,
            paper_rotation: layout.plot_rotation,
            paper_units: layout.plot_paper_units,
            margin_left: layout.plot_margin_left,
            margin_bottom: layout.plot_margin_bottom,
            margin_right: layout.plot_margin_right,
            margin_top: layout.plot_margin_top,
            print_scale: layout.plot_scale_numerator / layout.plot_scale_denominator,
        }
    }
}

/// A member of a block record's entity collection. ACadSharp's R12 reader promotes the attributes and the
/// SEQEND of an INSERT into the block collection.
pub(crate) enum Item<'a> {
    Entity(&'a EntityType),
    Attribute(&'a crate::entities::AttributeEntity),
    Seqend(&'a crate::entities::Seqend),
}

/// The `Insert` view of an INSERT or a TABLE.
pub(crate) struct InsertView<'a> {
    pub record: Option<&'a BlockRecord>,
    pub insert_point: Xyz,
    pub normal: Xyz,
    pub rotation: f64,
    pub scale: Xyz,
}

/// The `IText` view of a text entity.
pub(crate) struct TextView<'a> {
    pub is_line_text: bool,
    pub horizontal: TextHorizontalAlignment,
    pub vertical: TextVerticalAlignment,
    pub alignment_point: Xyz,
    pub insert_point: Xyz,
    pub rotation: f64,
    pub height: f64,
    pub style_name: &'a str,
    pub value: String,
}

/// A DXF attribute without a height reads as 1 in ACadSharp.
fn attribute_height(height: f64, source: Option<&crate::entities::text::TextSource>) -> f64 {
    if source.is_some_and(|s| s.dxf && !s.has_height) {
        1.0
    } else {
        height
    }
}

fn horizontal_of(value: i64) -> TextHorizontalAlignment {
    match value {
        1 => TextHorizontalAlignment::Center,
        2 => TextHorizontalAlignment::Right,
        3 => TextHorizontalAlignment::Aligned,
        4 => TextHorizontalAlignment::Middle,
        5 => TextHorizontalAlignment::Fit,
        _ => TextHorizontalAlignment::Left,
    }
}

fn vertical_of(value: i64) -> TextVerticalAlignment {
    match value {
        1 => TextVerticalAlignment::Bottom,
        2 => TextVerticalAlignment::Middle,
        3 => TextVerticalAlignment::Top,
        _ => TextVerticalAlignment::Baseline,
    }
}

/// An entity of a block as the writer sees it.
pub(crate) enum PolylineKind<'a> {
    Lw(&'a crate::entities::LwPolyline),
    Polyline2D(&'a crate::entities::Polyline2D),
    Polyline3D(&'a crate::entities::Polyline3D),
    Polyface(&'a crate::entities::PolyfaceMesh),
    PolygonMesh(&'a crate::entities::PolygonMeshEntity),
}

impl<'a> Model<'a> {
    pub fn new(doc: &'a CadDocument, view: bool) -> Self {
        Model {
            doc,
            view,
            cache: std::rc::Rc::new(ModelCache::default()),
        }
    }

    pub fn block_by_name(&self, name: &str) -> Option<&'a BlockRecord> {
        self.doc.block_records.get(name)
    }

    pub fn block_by_handle(&self, handle: crate::types::Handle) -> Option<&'a BlockRecord> {
        self.doc.block_records.iter().find(|b| b.handle == handle)
    }

    /// `BlockRecord.Units`: ACadSharp reads the units of a DWG block record only from R2007 on, and of a DXF only
    /// when the record states them.
    pub fn record_units(&self, record: &BlockRecord) -> i16 {
        if self.view
            && ((self.doc.dwg_source_version.is_some() && self.doc.version < crate::types::DxfVersion::AC1021)
                || record.dxf_present.is_some_and(|p| p & 32 == 0))
        {
            0
        } else {
            record.units
        }
    }

    /// `BlockRecord.Entities`. ACadSharp registers only the entities the record itself lists.
    pub fn entities_of(&self, record: &'a BlockRecord) -> Vec<&'a EntityType> {
        #[cfg(feature = "acadsharp-compat")]
        let handles: Vec<crate::types::Handle> = if self.view {
            crate::compat::acadsharp::view_entity_handles(self.doc, record).to_vec()
        } else {
            record.entity_handles.clone()
        };
        #[cfg(not(feature = "acadsharp-compat"))]
        let handles: Vec<crate::types::Handle> = record.entity_handles.clone();
        let r12 = self.r12_dxf();
        let model_space = self.doc.header.model_space_block_handle;
        let ownerless = |v: &crate::entities::Viewport| {
            self.doc
                .source_entity_owners
                .as_ref()
                .and_then(|owners| owners.get(&v.common.handle))
                .is_some_and(|owner| owner.is_null())
        };
        // An ENTITIES record without a stated owner enters the model space, after the records it lists.
        let mut adopted: Vec<&'a EntityType> = Vec::new();
        if r12 && record.handle == model_space {
            for other in self.doc.block_records.iter().filter(|b| b.handle != model_space) {
                for h in &other.entity_handles {
                    if let Some(e @ EntityType::Viewport(v)) = self.doc.get_entity(*h) {
                        if ownerless(v) && !handles.contains(h) {
                            adopted.push(e);
                        }
                    }
                }
            }
        }
        handles
            .iter()
            .filter_map(|h| self.doc.get_entity(*h))
            .filter(|e| {
                if !r12 {
                    return true;
                }
                match e {
                    // The reader of ACadSharp rejects a diameter dimension with user positioned text.
                    EntityType::Dimension(d) => {
                        !(matches!(d.as_ref(), Dimension::Diameter(_)) && d.base().text_user_positioned)
                    }
                    // An ENTITIES record without a stated owner enters the model space.
                    EntityType::Viewport(v) => !ownerless(v) || record.handle == model_space,
                    _ => true,
                }
            })
            .chain(adopted)
            .collect()
    }

    /// `BlockRecord.Entities` with the children ACadSharp's R12 reader adds to the collection.
    pub(crate) fn items_of(&self, record: &'a BlockRecord) -> Vec<Item<'a>> {
        let r12 = self.r12_dxf();
        let mut items = Vec::new();
        for e in self.entities_of(record) {
            items.push(Item::Entity(e));
            if let (true, EntityType::Insert(insert)) = (r12, e) {
                items.extend(insert.attributes.iter().map(Item::Attribute));
                if let Some(seqend) = insert.seqend.as_deref() {
                    items.push(Item::Seqend(seqend));
                }
            }
        }
        items
    }

    /// An R12 DXF: ACadSharp reads no layouts and builds `Polyline2D` from every kind of POLYLINE.
    fn r12_dxf(&self) -> bool {
        self.view
            && self.doc.dwg_source_version.is_none()
            && matches!(self.doc.version, crate::types::DxfVersion::AC1009 | crate::types::DxfVersion::Unknown)
    }

    /// The layouts in `CadDocument.Layouts` order (the entries of the ACAD_LAYOUT dictionary).
    pub fn layouts(&self) -> Vec<&'a Layout> {
        let mut result = Vec::new();
        if self.r12_dxf() {
            return result;
        }
        let mut dict = self.doc.header.acad_layout_dict_handle;
        if !matches!(self.doc.objects.get(&dict), Some(ObjectType::Dictionary(_))) {
            // A DXF names the dictionary only through the named objects dictionary.
            dict = match self.doc.objects.get(&self.doc.header.named_objects_dict_handle) {
                Some(ObjectType::Dictionary(root)) => root
                    .entries
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("ACAD_LAYOUT"))
                    .map(|(_, h)| *h)
                    .unwrap_or(dict),
                _ => dict,
            };
        }
        if let Some(ObjectType::Dictionary(d)) = self.doc.objects.get(&dict) {
            for (_, h) in &d.entries {
                if let Some(ObjectType::Layout(l)) = self.doc.objects.get(h) {
                    result.push(l);
                }
            }
        }
        result
    }

    pub fn model_space(&self) -> Option<&'a BlockRecord> {
        self.block_by_handle(self.doc.header.model_space_block_handle)
            .or_else(|| self.block_by_name("*Model_Space"))
    }

    /// `Entity.ObjectName`.
    pub fn object_name(e: &EntityType) -> String {
        match e {
            EntityType::Insert(_) => "INSERT".into(),
            EntityType::Dimension(d) => match d.as_ref() {
                Dimension::Arc(_) => "ARC_DIMENSION".into(),
                _ => "DIMENSION".into(),
            },
            EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
            | EntityType::PolyfaceMesh(_)
            | EntityType::PolygonMesh(_) => "POLYLINE".into(),
            EntityType::Spline(_) | EntityType::Helix(_) => "SPLINE".into(),
            EntityType::Underlay(u) => match u.underlay_type {
                crate::entities::UnderlayType::Pdf => "PDFUNDERLAY".into(),
                crate::entities::UnderlayType::Dwf => "DWFUNDERLAY".into(),
                crate::entities::UnderlayType::Dgn => "DGNUNDERLAY".into(),
            },
            other => crate::entities::Entity::entity_type(other.as_entity()).to_owned(),
        }
    }

    fn layer_of(&self, common: &EntityCommon) -> Option<&'a crate::tables::Layer> {
        common
            .layer_handle
            .filter(|h| !h.is_null())
            .and_then(|h| self.doc.layers.iter().find(|l| l.handle == h))
            .or_else(|| self.doc.layers.get(&common.layer))
            .or_else(|| self.doc.layers.get("0"))
    }

    /// The line type object of an entity (`Entity.LineType`): the stored handle, else the name (empty is ByLayer).
    fn own_line_type(&self, common: &EntityCommon) -> Option<&'a LineType> {
        common
            .linetype_handle
            .filter(|h| !h.is_null())
            .and_then(|h| self.doc.line_types.iter().find(|l| l.handle == h))
            .or_else(|| {
                let name = if common.linetype.is_empty() { "ByLayer" } else { common.linetype.as_str() };
                self.doc.line_types.get(name)
            })
    }

    /// `BlockRecord.BlockEntity`: the BLOCK entity the record names, else the one that names it as owner.
    fn marker_entity(&self, record: &BlockRecord) -> Option<&'a EntityType> {
        let markers = self.cache.markers.get_or_init(|| {
            let mut map = std::collections::HashMap::new();
            for e in &self.doc.entities {
                if let EntityType::Block(b) = e.as_ref() {
                    map.entry(b.common.handle).or_insert(&**e);
                }
            }
            map
        });
        let by_handle = markers.get(&record.block_entity_handle);
        let found = by_handle.or_else(|| {
            markers.values().find(|e| matches!(e, EntityType::Block(b) if b.common.owner_handle == record.handle))
        });
        found.copied()
    }

    fn marker_of(&self, record: &'a BlockRecord) -> Option<&'a EntityCommon> {
        match self.marker_entity(record) {
            Some(EntityType::Block(b)) => Some(&b.common),
            _ => record.block_entity_common.as_deref(),
        }
    }

    fn owner_marker(&self, common: &EntityCommon) -> Option<&'a EntityCommon> {
        let record = self.block_by_handle(common.owner_handle)?;
        self.marker_of(record)
    }

    fn own_color(&self, common: &EntityCommon) -> Color {
        if !self.view {
            return common.color;
        }
        #[cfg(feature = "acadsharp-compat")]
        return crate::compat::acadsharp::view_entity_color(self.doc, common);
        #[cfg(not(feature = "acadsharp-compat"))]
        acadsharp_entity_color(common)
    }

    fn layer_color(&self, layer: &crate::tables::Layer) -> Color {
        #[cfg(feature = "acadsharp-compat")]
        if self.view {
            return crate::compat::acadsharp::view_layer_color(self.doc, layer);
        }
        layer.color
    }

    fn layer_line_weight(&self, layer: &crate::tables::Layer) -> i16 {
        #[cfg(feature = "acadsharp-compat")]
        if self.view {
            return crate::compat::acadsharp::view_layer_line_weight(self.doc, layer).value();
        }
        layer.line_weight.value()
    }

    fn own_line_weight(&self, common: &EntityCommon) -> crate::types::LineWeight {
        #[cfg(feature = "acadsharp-compat")]
        if self.view {
            return crate::compat::acadsharp::view_entity_line_weight(self.doc, common);
        }
        common.line_weight
    }

    /// `Entity.GetActiveColor`.
    pub fn active_color(&self, common: &EntityCommon) -> Color {
        let layer_color = || self.layer_of(common).map(|l| self.layer_color(l)).unwrap_or(Color::ByLayer);
        let own = self.own_color(common);
        match own {
            Color::ByLayer => layer_color(),
            Color::ByBlock => match self.owner_marker(common) {
                Some(marker) => {
                    let c = self.own_color(marker);
                    if c == Color::ByLayer {
                        layer_color()
                    } else {
                        c
                    }
                }
                None => own,
            },
            c => c,
        }
    }

    /// `Entity.GetActiveLineType`.
    pub fn active_line_type(&self, common: &EntityCommon) -> Option<&'a LineType> {
        let own = self.own_line_type(common);
        let name = own.map_or("ByLayer", |l| l.name.as_str());
        if name.eq_ignore_ascii_case("ByLayer") {
            let layer = self.layer_of(common)?;
            return self.doc.line_types.get(&layer.line_type);
        }
        if name.eq_ignore_ascii_case("ByBlock") {
            if let Some(marker) = self.owner_marker(common) {
                return self.own_line_type(marker);
            }
        }
        own
    }

    /// `Entity.GetActiveLineWeightType`, as the `LineWeightType` number.
    pub fn active_line_weight(&self, common: &EntityCommon) -> i16 {
        self.line_weight_depth(common, 0)
    }

    fn line_weight_depth(&self, common: &EntityCommon, depth: usize) -> i16 {
        use crate::types::LineWeight;
        let own = self.own_line_weight(common);
        match own {
            LineWeight::ByLayer => self
                .layer_of(common)
                .map(|l| self.layer_line_weight(l))
                .unwrap_or(-1),
            LineWeight::ByBlock => match self.owner_marker(common) {
                Some(marker) if depth < MAX_DEPTH && !std::ptr::eq(marker, common) => {
                    self.line_weight_depth(marker, depth + 1)
                }
                _ => own.value(),
            },
            other => other.value(),
        }
    }

    fn block_base_point(&self, record: &'a BlockRecord) -> Option<Xyz> {
        match self.marker_entity(record) {
            Some(EntityType::Block(b)) => Some(v3(b.base_point)),
            _ => Some(v3(record.base_point)),
        }
    }

    /// The `Insert` view of an INSERT (or of a TABLE, which ACadSharp derives from `Insert`).
    pub(crate) fn insert_view(&self, e: &'a EntityType) -> Option<InsertView<'a>> {
        match e {
            EntityType::Insert(i) => Some(InsertView {
                record: self.block_by_name(&i.block_name),
                insert_point: v3(i.insert_point),
                normal: v3(i.normal),
                rotation: i.rotation,
                scale: Xyz::new(i.x_scale(), i.y_scale(), i.z_scale()),
            }),
            EntityType::Table(t) => {
                let scale = t.dxf_insert_scale.unwrap_or(t.insert_scale);
                let dxf = self.doc.dxf_source_version.is_some();
                Some(InsertView {
                    record: self.block_by_name(&t.block_name).or_else(|| {
                        if dxf {
                            None
                        } else {
                            t.block_record_handle.and_then(|h| self.block_by_handle(h))
                        }
                    }),
                    insert_point: v3(t.insertion_point),
                    normal: v3(t.normal),
                    rotation: t.dxf_insert_rotation.unwrap_or(t.insert_rotation),
                    scale: v3(scale),
                })
            }
            _ => None,
        }
    }

    /// `Insert.GetTransform`.
    pub(crate) fn insert_transform(&self, insert: &InsertView<'a>) -> Transform {
        let world = Matrix4::arbitrary_axis(insert.normal);
        let base_point = insert
            .record
            .and_then(|b| self.block_base_point(b))
            .unwrap_or(Xyz::ZERO);
        let translation = Transform::from_trs(insert.insert_point.sub(base_point), Xyz::splat(1.0), Xyz::ZERO)
            .expect("a unit scale is valid");
        let rotation = Matrix4::create_from_axis_angle(Xyz::AXIS_Z, insert.rotation);
        let scale = Matrix4::create_scale(insert.scale);
        Transform::from_matrix(world.mul(translation.matrix()).mul(&rotation).mul(&scale))
    }

    /// `Style` of a text entity (falls back to `Standard`).
    pub fn text_style(&self, name: &str) -> Option<&'a TextStyle> {
        self.doc
            .text_styles
            .get(name)
            .or_else(|| self.doc.text_styles.get("Standard"))
    }

    pub(crate) fn attribute_view(&self, t: &'a crate::entities::AttributeEntity) -> TextView<'a> {
        let view = self.view;
        TextView {
            is_line_text: true,
            horizontal: horizontal_of(i64::from(t.horizontal_alignment.to_value())),
            vertical: vertical_of(i64::from(t.vertical_alignment.to_value())),
            alignment_point: v3(t.alignment_point),
            insert_point: v3(t.insertion_point),
            rotation: t.rotation,
            height: if view { attribute_height(t.height, t.source.as_deref()) } else { t.height },
            style_name: &t.text_style,
            value: if view {
                super::text_source_value(&t.value, t.source.as_deref())
            } else {
                t.value.clone()
            },
        }
    }

    /// The `IText` view of a text entity (`Text`, `MText`, `AttributeDefinition`, `AttributeEntity`).
    pub(crate) fn text_view(&self, e: &'a EntityType) -> TextView<'a> {
        let doc = self.doc;
        let view = self.view;
        let text_value = move |value: &str, source: Option<&crate::entities::text::TextSource>| {
            if view {
                super::text_source_value(value, source)
            } else {
                value.to_owned()
            }
        };
        match e {
            EntityType::Text(t) => TextView {
                is_line_text: true,
                horizontal: t.horizontal_alignment,
                vertical: t.vertical_alignment,
                alignment_point: t.alignment_point.map(v3).unwrap_or(Xyz::ZERO),
                insert_point: v3(t.insertion_point),
                rotation: t.rotation,
                height: t.height,
                style_name: &t.style,
                value: if view {
                    super::dwg_legacy_replacement(doc, text_value(&t.value, t.source.as_deref()))
                } else {
                    t.value.clone()
                },
            },
            EntityType::AttributeDefinition(t) => TextView {
                is_line_text: true,
                horizontal: horizontal_of(i64::from(t.horizontal_alignment.to_value())),
                vertical: vertical_of(i64::from(t.vertical_alignment.to_value())),
                alignment_point: v3(t.alignment_point),
                insert_point: v3(t.insertion_point),
                rotation: t.rotation,
                height: if view { attribute_height(t.height, t.source.as_deref()) } else { t.height },
                style_name: &t.text_style,
                value: text_value(&t.default_value, t.source.as_deref()),
            },
            EntityType::AttributeEntity(t) => self.attribute_view(t),
            EntityType::MText(t) => TextView {
                is_line_text: false,
                horizontal: TextHorizontalAlignment::Left,
                vertical: TextVerticalAlignment::Baseline,
                alignment_point: Xyz::ZERO,
                insert_point: v3(t.insertion_point),
                rotation: t.rotation,
                height: t.height,
                style_name: &t.style,
                value: if view {
                    let value = t.unchanged_source_value().map_or_else(|| t.value.clone(), str::to_string);
                    // A drawing before R2004 states the original text of an MTEXT in AutoCAD's round-trip record.
                    #[cfg(feature = "acadsharp-compat")]
                    let value = crate::io::dwg::autocad_records::mtext_file_value(doc, t.common.handle, &value).into_owned();
                    value
                } else {
                    t.value.clone()
                },
            },
            _ => unreachable!("not a text entity"),
        }
    }

    /// The `IPolyline` view of an entity, if it is one.
    pub(crate) fn polyline_kind(e: &'a EntityType) -> Option<PolylineKind<'a>> {
        match e {
            EntityType::LwPolyline(p) => Some(PolylineKind::Lw(p)),
            EntityType::Polyline2D(p) => Some(PolylineKind::Polyline2D(p)),
            EntityType::Polyline3D(p) => Some(PolylineKind::Polyline3D(p)),
            EntityType::PolyfaceMesh(p) => Some(PolylineKind::Polyface(p)),
            EntityType::PolygonMesh(p) => Some(PolylineKind::PolygonMesh(p)),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Bounding boxes
    // -----------------------------------------------------------------------------------------

    /// `BlockRecord.GetBoundingBox()`.
    pub fn record_bounding_box(&self, record: &'a BlockRecord) -> Result<BoundingBox, SvgError> {
        self.record_bounding_box_depth(record, 0)
    }

    fn record_bounding_box_depth(
        &self,
        record: &'a BlockRecord,
        depth: usize,
    ) -> Result<BoundingBox, SvgError> {
        if depth > MAX_DEPTH {
            return Err(SvgError::invalid_operation("Block references nest too deeply."));
        }
        let mut result = BoundingBox::NULL;
        for entry in self.items_of(record) {
            let item = match entry {
                Item::Entity(e) => self.entity_bounding_box(e, depth)?,
                Item::Attribute(a) => BoundingBox::from_point(v3(a.insertion_point)),
                Item::Seqend(_) => BoundingBox::NULL,
            };
            if item.extent == Extent::Infinite {
                continue;
            }
            result = result.merge(item);
        }
        Ok(result)
    }

    /// `Entity.GetBoundingBox()`.
    pub fn entity_bounding_box(&self, e: &'a EntityType, depth: usize) -> Result<BoundingBox, SvgError> {
        use EntityType as E;
        Ok(match e {
            E::Line(l) => {
                let (s, t) = (v3(l.start), v3(l.end));
                BoundingBox::from_min_max(
                    Xyz::new(
                        super::csmath::dmin(s.x, t.x),
                        super::csmath::dmin(s.y, t.y),
                        super::csmath::dmin(s.z, t.z),
                    ),
                    Xyz::new(
                        super::csmath::dmax(s.x, t.x),
                        super::csmath::dmax(s.y, t.y),
                        super::csmath::dmax(s.z, t.z),
                    ),
                )
            }
            E::Point(p) => BoundingBox::from_point(v3(p.location)),
            E::Circle(c) => {
                use super::csmath::{dmax, dmin};
                let center = v3(c.center);
                let min = Xyz::new(
                    dmin(center.x - c.radius, center.x + c.radius),
                    dmin(center.y - c.radius, center.y + c.radius),
                    dmin(center.z, center.z),
                );
                let max = Xyz::new(
                    dmax(center.x - c.radius, center.x + c.radius),
                    dmax(center.y - c.radius, center.y + c.radius),
                    dmax(center.z, center.z),
                );
                BoundingBox::from_min_max(min, max)
            }
            E::Arc(a) => BoundingBox::from_points(Self::arc_geometry(a).polygonal_vertexes(256)?),
            E::Ellipse(el) => {
                BoundingBox::from_points(Self::ellipse_geometry(el).polygonal_vertexes(100)?)
            }
            E::Solid(s) => BoundingBox::from_points([
                v3(s.first_corner),
                v3(s.second_corner),
                v3(s.third_corner),
                v3(s.fourth_corner),
            ]),
            E::Face3D(s) => BoundingBox::from_points([
                v3(s.first_corner),
                v3(s.second_corner),
                v3(s.third_corner),
                v3(s.fourth_corner),
            ]),
            E::Shape(s) => BoundingBox::from_point(v3(s.insertion_point)),
            E::Tolerance(t) => BoundingBox::from_point(v3(t.insertion_point)),
            E::Ray(_) | E::XLine(_) => BoundingBox::INFINITE,
            E::Text(t) => BoundingBox::from_point(v3(t.insertion_point)),
            E::MText(t) => BoundingBox::from_point(v3(t.insertion_point)),
            E::AttributeDefinition(t) => BoundingBox::from_point(v3(t.insertion_point)),
            E::AttributeEntity(t) => BoundingBox::from_point(v3(t.insertion_point)),
            E::Seqend(_) | E::Block(_) | E::BlockEnd(_) => BoundingBox::NULL,
            E::MultiLeader(_) | E::Table(_) | E::Underlay(_) => BoundingBox::NULL,
            E::Solid3D(_) | E::Region(_) | E::Body(_) | E::Surface(_) => BoundingBox::NULL,
            E::Viewport(v) => {
                let c = v3(v.center);
                BoundingBox::from_min_max(
                    Xyz::new(c.x - v.width / 2.0, c.y - v.height / 2.0, c.z),
                    Xyz::new(c.x + v.width / 2.0, c.y + v.height / 2.0, c.z),
                )
            }
            E::Leader(l) => BoundingBox::from_points(l.vertices.iter().map(|v| v3(*v))),
            E::MLine(m) => BoundingBox::from_points(m.vertices.iter().map(|v| v3(v.position))),
            E::Mesh(m) => BoundingBox::from_points(m.vertices.iter().map(|v| v3(*v))),
            E::Ole2Frame(o) => BoundingBox::from_points([v3(o.lower_right_corner), v3(o.upper_left_corner)]),
            E::Wipeout(w) => {
                Self::wipeout_box(v3(w.insertion_point), w.clip_boundary_vertices.iter().map(|v| v2(*v)))
            }
            E::RasterImage(r) => Self::wipeout_box(
                v3(r.insertion_point),
                r.clip_boundary.vertices.iter().map(|v| v2(*v)),
            ),
            E::Spline(s) => Self::spline_box(&Self::spline_entity_geometry(s), &s.fit_points),
            E::Helix(h) => Self::spline_box(&Self::spline_entity_geometry(&h.spline), &h.spline.fit_points),
            E::Insert(_) => {
                let view = self.insert_view(e).expect("insert view");
                let record = view
                    .record
                    .ok_or_else(|| SvgError::null_reference("Object reference not set to an instance of an object."))?;
                let b = self.record_bounding_box_depth(record, depth + 1)?;
                let t = self.insert_transform(&view);
                BoundingBox::from_min_max(t.apply_transform(b.min), t.apply_transform(b.max))
            }
            E::Hatch(h) => {
                let mut b = BoundingBox::NULL;
                for path in &h.paths {
                    b = b.merge(self.hatch_path_box(path)?);
                }
                b
            }
            E::Dimension(d) => Self::dimension_box(d),
            E::LwPolyline(_) | E::Polyline2D(_) | E::Polyline3D(_) | E::PolyfaceMesh(_) | E::PolygonMesh(_) => {
                let kind = Self::polyline_kind(e).expect("polyline");
                let vertices = self.polyline_vertices(&kind);
                if vertices.iter().any(|v| v.bulge != 0.0) {
                    BoundingBox::from_points(polyline_points_with_arcs(
                        &vertices,
                        Self::polyline_closed(&kind),
                        255,
                        3,
                    )?)
                } else {
                    BoundingBox::from_points(vertices.iter().map(|v| v.location))
                }
            }
            E::Polyline(_) => BoundingBox::NULL,
            E::Unknown(u) if u.dxf_name == "AEC_WALL" => {
                return Err(SvgError::not_implemented("The method or operation is not implemented."))
            }
            E::Unknown(_) | E::Extended(_) | E::Light(_) | E::SectionSymbol(_) | E::ViewBorder(_) => {
                BoundingBox::NULL
            }
        })
    }

    fn wipeout_box(insert: Xyz, vertices: impl Iterator<Item = Xy>) -> BoundingBox {
        let v: Vec<Xy> = vertices.collect();
        if v.is_empty() {
            return BoundingBox::NULL;
        }
        let min_x = v.iter().map(|p| p.x).fold(f64::NAN, nan_min);
        let min_y = v.iter().map(|p| p.y).fold(f64::NAN, nan_min);
        let max_x = v.iter().map(|p| p.x).fold(f64::NAN, nan_max);
        let max_y = v.iter().map(|p| p.y).fold(f64::NAN, nan_max);
        BoundingBox::from_min_max(
            Xyz::new(min_x, min_y, 0.0).add(insert),
            Xyz::new(max_x, max_y, 0.0).add(insert),
        )
    }

    fn spline_box(geometry: &SplineGeometry, fit_points: &[Vector3]) -> BoundingBox {
        match geometry.polygonal_vertexes(256) {
            Ok(points) => BoundingBox::from_points(points),
            Err(_) => BoundingBox::from_points(fit_points.iter().map(|p| v3(*p))),
        }
    }

    fn dimension_box(d: &Dimension) -> BoundingBox {
        match d {
            Dimension::Aligned(a) => BoundingBox::from_min_max(v3(a.first_point), v3(a.second_point)),
            Dimension::Linear(a) => BoundingBox::from_min_max(v3(a.first_point), v3(a.second_point)),
            Dimension::Angular2Ln(a) => BoundingBox::from_min_max(v3(a.first_point), v3(a.second_point)),
            Dimension::Angular3Pt(a) => BoundingBox::from_min_max(v3(a.first_point), v3(a.second_point)),
            Dimension::Arc(a) => {
                BoundingBox::from_min_max(v3(a.first_extension_point), v3(a.second_extension_point))
            }
            Dimension::Radius(r) => {
                // ACadSharp's AngleVertex is the native definition point.
                let ins = v3(r.base.insertion_point);
                let vertex = v3(r.definition_point);
                BoundingBox::from_min_max(ins.sub(vertex), ins.add(vertex))
            }
            Dimension::Diameter(r) => {
                let ins = v3(r.base.insertion_point);
                let vertex = v3(r.angle_vertex);
                BoundingBox::from_min_max(ins.sub(vertex), ins.add(vertex))
            }
            Dimension::Ordinate(o) => {
                BoundingBox::from_min_max(v3(o.feature_location), v3(o.leader_endpoint))
            }
            Dimension::LargeRadial(r) => {
                let ins = v3(r.base.insertion_point);
                let vertex = v3(r.definition_point);
                BoundingBox::from_min_max(ins.sub(vertex), ins.add(vertex))
            }
        }
    }

    fn hatch_path_box(&self, path: &BoundaryPath) -> Result<BoundingBox, SvgError> {
        let mut b = BoundingBox::NULL;
        for edge in &path.edges {
            let item = match edge {
                BoundaryEdge::Line(l) => BoundingBox::from_points([v2(l.start).to_xyz(), v2(l.end).to_xyz()]),
                BoundaryEdge::CircularArc(a) => {
                    BoundingBox::from_points(Self::hatch_arc(a)?.polygonal_vertexes(256)?)
                }
                BoundaryEdge::EllipticArc(e) => {
                    BoundingBox::from_points(Self::hatch_ellipse(e)?.polygonal_vertexes(100)?)
                }
                BoundaryEdge::Spline(s) => {
                    BoundingBox::from_points(s.control_points.iter().map(|p| v3(*p)))
                }
                BoundaryEdge::Polyline(p) => BoundingBox::from_points(p.vertices.iter().map(|p| v3(*p))),
            };
            b = b.merge(item);
        }
        Ok(b)
    }

    // -----------------------------------------------------------------------------------------
    // Geometry adapters
    // -----------------------------------------------------------------------------------------

    pub(crate) fn arc_geometry(a: &crate::entities::Arc) -> ArcGeometry {
        ArcGeometry {
            center: v3(a.center),
            radius: a.radius,
            start_angle: a.start_angle,
            end_angle: a.end_angle,
            normal: v3(a.normal),
        }
    }

    pub(crate) fn ellipse_geometry(e: &crate::entities::Ellipse) -> EllipseGeometry {
        EllipseGeometry {
            center: v3(e.center),
            major_axis_end_point: v3(e.major_axis),
            radius_ratio: e.minor_axis_ratio,
            start_parameter: e.start_parameter,
            end_parameter: e.end_parameter,
            normal: v3(e.normal),
        }
    }

    pub(crate) fn spline_entity_geometry(s: &crate::entities::Spline) -> SplineGeometry {
        let flags = s.dxf_flags;
        let _ = flags;
        SplineGeometry {
            control_points: s.control_points.iter().map(|p| v3(*p)).collect(),
            weights: s.weights.clone(),
            knots: s.knots.clone(),
            degree: s.degree,
            is_closed: s.flags.closed,
            is_periodic: s.flags.periodic,
        }
    }

    /// `Hatch.BoundaryPath.Arc.ToEntity()`.
    pub(crate) fn hatch_arc(a: &crate::entities::CircularArcEdge) -> Result<ArcGeometry, SvgError> {
        let two_pi = std::f64::consts::PI * 2.0;
        let (start, end) = if a.counter_clockwise {
            (a.start_angle, a.end_angle)
        } else {
            (2.0 * std::f64::consts::PI - a.end_angle, 2.0 * std::f64::consts::PI - a.start_angle)
        };
        let _ = two_pi;
        if a.radius <= 0.0 {
            return Err(SvgError::out_of_range(format!(
                "The radius must be greater than 0. (Parameter 'value')\nActual value was {}.",
                super::converter::double_to_string(a.radius)
            )));
        }
        Ok(ArcGeometry {
            center: Xyz::new(a.center.x, a.center.y, 0.0),
            radius: a.radius,
            start_angle: start,
            end_angle: end,
            normal: Xyz::AXIS_Z,
        })
    }

    /// `Hatch.BoundaryPath.Ellipse.ToEntity()`.
    pub(crate) fn hatch_ellipse(e: &crate::entities::EllipticArcEdge) -> Result<EllipseGeometry, SvgError> {
        let pi = std::f64::consts::PI;
        let (start, end) = if e.counter_clockwise {
            (e.start_angle, e.end_angle)
        } else {
            (2.0 * pi - e.end_angle, 2.0 * pi - e.start_angle)
        };
        EllipseGeometry::check_ratio(e.minor_axis_ratio)?;
        Ok(EllipseGeometry {
            center: Xyz::new(e.center.x, e.center.y, 0.0),
            major_axis_end_point: Xyz::new(e.major_axis_endpoint.x, e.major_axis_endpoint.y, 0.0),
            radius_ratio: e.minor_axis_ratio,
            start_parameter: start,
            end_parameter: end,
            normal: Xyz::AXIS_Z,
        })
    }

    /// `Hatch.BoundaryPath.GetPoints(precision)`.
    pub(crate) fn hatch_path_points(&self, path: &BoundaryPath, precision: i32) -> Result<Vec<Xyz>, SvgError> {
        let mut pts: Vec<Xyz> = Vec::new();
        for edge in &path.edges {
            match edge {
                BoundaryEdge::CircularArc(a) => {
                    pts.extend(Self::hatch_arc(a)?.polygonal_vertexes(precision)?)
                }
                BoundaryEdge::EllipticArc(e) => {
                    pts.extend(Self::hatch_ellipse(e)?.polygonal_vertexes(precision)?)
                }
                BoundaryEdge::Line(l) => {
                    pts.push(v2(l.start).to_xyz());
                    pts.push(v2(l.end).to_xyz());
                }
                BoundaryEdge::Polyline(p) => {
                    let vertices: Vec<VertexPoint> = p
                        .vertices
                        .iter()
                        .map(|v| VertexPoint {
                            location: Xyz::new(v.x, v.y, 0.0),
                            bulge: v.z,
                        })
                        .collect();
                    pts.extend(polyline_points_with_arcs(&vertices, p.is_closed, precision, 3)?);
                }
                BoundaryEdge::Spline(s) => {
                    let geometry = SplineGeometry {
                        control_points: s.control_points.iter().map(|p| Xyz::new(p.x, p.y, 0.0)).collect(),
                        // The DWG reader of ACadSharp reads an absent non-rational weight as zero.
                        weights: s
                            .control_points
                            .iter()
                            .map(|p| if self.view && self.doc.dwg_source_version.is_some() && !s.rational { 0.0 } else { p.z })
                            .collect(),
                        knots: s.knots.clone(),
                        degree: s.degree,
                        is_closed: true,
                        is_periodic: s.periodic,
                    };
                    pts.extend(geometry.polygonal_vertexes(precision)?);
                }
            }
        }
        Ok(pts)
    }

    /// The vertices of an `IPolyline` (`IVertex.Location`, `IVertex.Bulge`).
    pub(crate) fn polyline_vertices(&self, kind: &PolylineKind<'_>) -> Vec<VertexPoint> {
        match kind {
            PolylineKind::Lw(p) => p
                .vertices
                .iter()
                .map(|v| VertexPoint {
                    location: Xyz::new(v.location.x, v.location.y, 0.0),
                    bulge: v.bulge,
                })
                .collect(),
            PolylineKind::Polyline2D(p) => p
                .vertices
                .iter()
                .map(|v| VertexPoint {
                    location: v3(v.location),
                    bulge: v.bulge,
                })
                .collect(),
            PolylineKind::Polyline3D(p) => p
                .vertices
                .iter()
                .map(|v| VertexPoint {
                    location: v3(v.position),
                    bulge: 0.0,
                })
                .collect(),
            PolylineKind::Polyface(p) => {
                let mut vertices: Vec<VertexPoint> = p
                    .vertices
                    .iter()
                    .map(|v| VertexPoint {
                        location: v3(v.location),
                        bulge: v.bulge,
                    })
                    .collect();
                if self.r12_dxf() {
                    // Without a polyface class ACadSharp reads the face records as vertices at the origin.
                    vertices.extend(p.faces.iter().map(|_| VertexPoint {
                        location: Xyz::ZERO,
                        bulge: 0.0,
                    }));
                }
                vertices
            }
            PolylineKind::PolygonMesh(p) => p
                .vertices
                .iter()
                .map(|v| VertexPoint {
                    location: v3(v.location),
                    bulge: 0.0,
                })
                .collect(),
        }
    }

    pub(crate) fn polyline_closed(kind: &PolylineKind<'_>) -> bool {
        match kind {
            PolylineKind::Lw(p) => p.is_closed,
            PolylineKind::Polyline2D(p) => p.flags.bits() & 33 != 0,
            PolylineKind::Polyline3D(p) => p.is_closed(),
            PolylineKind::Polyface(p) => p.flags.bits() & 33 != 0,
            PolylineKind::PolygonMesh(p) => p.flags.bits() & 33 != 0,
        }
    }

    /// `IPolyline.GetPoints<XY>(precision)`.
    pub(crate) fn polyline_xy_points(&self, kind: &PolylineKind<'_>, precision: i32) -> Result<Vec<Xyz>, SvgError> {
        polyline_points_with_arcs(&self.polyline_vertices(kind), Self::polyline_closed(kind), precision, 2)
    }

    
}

fn nan_min(acc: f64, v: f64) -> f64 {
    if acc.is_nan() {
        v
    } else {
        super::csmath::dmin(acc, v)
    }
}

fn nan_max(acc: f64, v: f64) -> f64 {
    if acc.is_nan() {
        v
    } else {
        super::csmath::dmax(acc, v)
    }
}
