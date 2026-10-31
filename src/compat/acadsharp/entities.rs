//! Canonical ACadSharp-facing entity dump mappings.
//!
//! The mapping deliberately uses only state retained by acadrust.  A field
//! whose state is absent or only held in an opaque payload is left as
//! `!unmapped` by `props`.

use crate::entities::{Dimension, DimensionType, EntityCommon, EntityType, Spline};
use crate::types::{DxfVersion, Handle, Vector2, Vector3};
use crate::CadDocument;
use serde_json::{json, Map, Value};

use super::{color, handle, number, optional_handle, props, record, transparency, vec2, vec3};

mod modeler;
mod multileader;
mod blocks;
pub(super) mod table;
pub(super) use blocks::block_record_marker;

const UNMAPPED: &str = "!unmapped";

fn unmapped() -> Value {
    Value::String(UNMAPPED.to_string())
}

fn integer(value: impl Into<i64>) -> Value {
    Value::Number(value.into().into())
}

fn bool_value(value: bool) -> Value {
    Value::Bool(value)
}

fn handles(values: impl IntoIterator<Item = Handle>) -> Value {
    Value::Array(values.into_iter().map(handle).collect())
}

fn array(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(values.into_iter().collect())
}

fn enum_value(value: i64) -> Value {
    integer(value)
}

fn spline_flags(doc: &CadDocument, value: &Spline) -> i64 {
    // DXF retains unknown bits in the raw group-70 word. DWG does not have
    // that word, so its public Flags value is reconstructed from the typed
    // five-bit flags. ORing the typed bits also keeps a constructed entity
    // coherent when its raw DXF word has not been populated.
    let mut flags = if value.dwg_scenario.is_some() {
        0
    } else {
        i64::from(value.dxf_flags as u16)
    };
    if value.flags.closed {
        flags |= 1;
    }
    // ACadSharp seeds Flags from Flags1.Closed before reading the scenario
    // body; its scenario-1 closed bit only adds the bit and never clears it.
    // The native DWG reader replaces the bit with the body value, so restore
    // the ACadSharp result for R2013+ DWG records here.
    if value.dwg_scenario.is_some()
        && doc.version >= DxfVersion::AC1027
        && value.dwg_flags1 & 4 != 0
    {
        flags |= 1;
    }
    if value.flags.periodic {
        flags |= 2;
    }
    if value.flags.rational {
        flags |= 4;
    }
    if value.flags.planar {
        flags |= 8;
    }
    if value.flags.linear {
        flags |= 16;
    }
    flags
}

fn spline_flags1(doc: &CadDocument, value: &Spline) -> i64 {
    let Some(scenario) = value.dwg_scenario else {
        // ACadSharp's DXF reader only fills Flags, leaving the DWG-only
        // Flags1 property at its default value.
        return 0;
    };
    // ACadSharp's SplineFlags1 enum has an underlying ushort; the native
    // reader stores the bit-long in i32, so project the low 16 bits only.
    // Before R2013 the record has no flags 1: the native model holds the fit-point method bits that AutoCAD's
    // `ACAD_SPLINE_DATA` record implies.
    let mut flags = if doc.version < DxfVersion::AC1027 { 0 } else { i64::from(value.dwg_flags1 as u16) };
    // Before R2013 the scenario itself is the only creation-method marker;
    // ACadSharp adds MethodFitPoints when it reads scenario 2.
    if scenario == 2 && doc.version < DxfVersion::AC1027 {
        flags |= 1;
    }
    flags
}

fn spline_knot_parameterization(doc: &CadDocument, value: &Spline) -> i64 {
    let Some(scenario) = value.dwg_scenario else {
        // KnotParametrization has no DXF assignment in ACadSharp.
        return 0;
    };
    if doc.version < DxfVersion::AC1027 {
        // The pre-R2013 reader assigns Custom only to control-point records;
        // fit-point records retain the enum's Chord default.
        return if scenario == 1 { 15 } else { 0 };
    }
    i64::from(value.knot_parameterization)
}

fn spline_has_valid_knot_count(value: &Spline, closed: bool) -> bool {
    // Match ACadSharp's unchecked Int32 expression, including negative and
    // overflowing public Degree values.
    let multiplier: i32 = if closed { 2 } else { 1 };
    let expected = (value.control_points.len() as i32)
        .wrapping_add(multiplier.wrapping_mul(value.degree))
        .wrapping_add(1);
    expected == value.knots.len() as i32
}

fn spline_control_point_tolerance(value: &Spline) -> f64 {
    if value.dwg_scenario == Some(2) {
        // Fit-point DWG records do not carry knot/control tolerances.
        0.0000001
    } else {
        value.control_tolerance
    }
}

fn spline_fit_tolerance(value: &Spline) -> f64 {
    if value.dwg_scenario == Some(1) {
        // Control-point DWG records do not carry fit tolerance.
        0.0000000001
    } else {
        value.fit_tolerance
    }
}

fn spline_knot_tolerance(value: &Spline) -> f64 {
    if value.dwg_scenario == Some(2) {
        // Fit-point DWG records do not carry knot/control tolerances.
        0.0000001
    } else {
        value.knot_tolerance
    }
}

/// The common data of an MTEXT as ACadSharp reads it from a DWG of a version before R2004: the line spacing and the background
/// fill are blocks of the `ACAD` extended data (the native model holds the values).
fn mtext_common<'a>(doc: &CadDocument, text: &'a crate::entities::MText) -> std::borrow::Cow<'a, EntityCommon> {
    let Some(version) = legacy_round_trip_version(doc).filter(|version| crate::io::dwg::autocad_roundtrip::states_mtext_spacing(*version)) else {
        // R2004 to R2013: the columns are the only block the entity's native fields stand for.
        if column_block_version(doc).is_none() {
            return std::borrow::Cow::Borrowed(&text.common);
        }
        let application = doc.app_ids.get("ACAD").map(|app| app.handle.value());
        return match crate::io::dwg::autocad_roundtrip::with_column_block(&text.common.extended_data, text, application) {
            Some(xdata) => {
                let mut common = text.common.clone();
                common.extended_data = xdata;
                std::borrow::Cow::Owned(common)
            }
            None => std::borrow::Cow::Borrowed(&text.common),
        };
    };
    let blocks = crate::io::dwg::autocad_roundtrip::mtext_blocks(text, version);
    if blocks.is_empty() {
        return std::borrow::Cow::Borrowed(&text.common);
    }
    let application = doc.app_ids.get("ACAD").map(|app| app.handle.value());
    match crate::io::dwg::autocad_roundtrip::with_mtext_blocks(&text.common.extended_data, blocks, application) {
        Some(xdata) => {
            let mut common = text.common.clone();
            common.extended_data = xdata;
            std::borrow::Cow::Owned(common)
        }
        None => std::borrow::Cow::Borrowed(&text.common),
    }
}

/// The version of a source document before R2018 whose MTEXT columns the readers took out of the `ACAD_MTEXT_COLUMN_INFO` block
/// of the entity's extended data (ACadSharp reads the block as extended data and no columns).
fn column_block_version(doc: &CadDocument) -> Option<crate::types::DxfVersion> {
    let version = doc.dwg_source_version.or(doc.dxf_source_version)?;
    (super::registration::source_document(doc) && doc.version == version && crate::io::dwg::autocad_roundtrip::states_mtext_columns(version))
        .then_some(version)
}

/// Whether the document is a source before R2007: ACadSharp does not read the lock of an attribute from the `AcDbAttr`
/// extended data the file states it in (`autocad_conversions`).
fn legacy_lock(doc: &CadDocument) -> bool {
    super::registration::source_document(doc) && doc.version < crate::types::DxfVersion::AC1021
}

/// The version of a DWG source document whose entity records lack some of the values the native model holds.
fn legacy_round_trip_version(doc: &CadDocument) -> Option<crate::types::DxfVersion> {
    let version = doc.dwg_source_version.or(doc.dxf_source_version)?;
    (super::registration::source_document(doc)
        && doc.version == version
        && crate::io::dwg::autocad_roundtrip::applies(version))
    .then_some(version)
}

/// The colour of an entity as ACadSharp reads it (what the SVG writer and `Entity.GetActiveColor` see): a drawing of
/// R14, R2000 or R2004 that AutoCAD saved from a later version holds the nearest index in the entity record.
pub(crate) fn view_entity_color(doc: &CadDocument, common: &EntityCommon) -> crate::types::Color {
    let legacy_color = legacy_round_trip_version(doc)
        .filter(|version| !crate::io::dwg::autocad_roundtrip::stores_true_color(*version))
        .and_then(|_| crate::io::dwg::autocad_roundtrip::aci_index(common));
    if let Some(index) = legacy_color {
        crate::types::Color::Index(index)
    } else if super::registration::source_document(doc) {
        crate::svg::acadsharp_entity_color(common)
    } else {
        common.color
    }
}

/// The line weight of an entity as ACadSharp reads it: those drawings state none in the entity record.
pub(crate) fn view_entity_line_weight(doc: &CadDocument, common: &EntityCommon) -> crate::types::LineWeight {
    if legacy_round_trip_version(doc).is_some_and(|version| !crate::io::dwg::autocad_roundtrip::stores_line_weight(version)) {
        crate::types::LineWeight::ByLayer
    } else {
        common.line_weight
    }
}

pub(super) fn common(
    doc: &CadDocument,
    common: &EntityCommon,
    object_name: &str,
    object_type: i64,
    subclass: &str,
    dynamic: bool,
) -> Vec<(&'static str, Value)> {
    // A drawing of R14, R2000 or R2004 that AutoCAD saved from a later version states the true colour, line weight and
    // material of an entity in round-trip records and the entity record holds the nearest index, no line weight or no
    // material (ACadSharp reads exactly that): the native model holds the values themselves.
    let legacy = legacy_round_trip_version(doc);
    let legacy_data = legacy.and_then(|version| {
        let values = crate::io::dwg::autocad_roundtrip::groups(common, doc, version);
        if values.is_empty() {
            return None;
        }
        let application = doc.app_ids.get("ACAD").map(|app| app.handle.value());
        crate::io::dwg::autocad_roundtrip::with_groups(&common.extended_data, values, application)
    });
    let legacy_color = legacy
        .filter(|version| !crate::io::dwg::autocad_roundtrip::stores_true_color(*version))
        .and_then(|_| crate::io::dwg::autocad_roundtrip::aci_index(common));
    // Before R2004 the entity record has no colour-book reference (the restored one names the object of the record's name).
    let legacy_book = legacy.is_some_and(|version| !crate::io::dwg::autocad_roundtrip::stores_true_color(version));
    vec![
        ("BookColor", optional_handle(if legacy_book { None } else { common.color_book_handle })),
        (
            "Color",
            if let Some(index) = legacy_color {
                color(crate::types::Color::Index(index))
            } else if super::registration::source_document(doc) {
                super::common::entity_color(common)
            } else {
                color(common.color)
            },
        ),
        (
            "ExtendedData",
            super::xdata::extended_data(doc, common.handle, legacy_data.as_ref().unwrap_or(&common.extended_data)),
        ),
        ("HasDynamicSubclass", bool_value(dynamic)),
        ("IsInvisible", bool_value(common.invisible)),
        (
            "Layer",
            optional_handle(
                common
                    .layer_handle
                    .filter(|h| !h.is_null())
                    .or_else(|| doc.layers.get(&common.layer).map(|layer| layer.handle)),
            ),
        ),
        (
            "LineType",
            optional_handle(common.linetype_handle.filter(|h| !h.is_null()).or_else(|| {
                let name = if common.linetype.is_empty() {
                    "ByLayer"
                } else {
                    &common.linetype
                };
                doc.line_types.get(name).map(|line| line.handle)
            })),
        ),
        ("LineTypeScale", number(common.linetype_scale)),
        (
            "LineWeight",
            enum_value(
                if legacy.is_some_and(|version| !crate::io::dwg::autocad_roundtrip::stores_line_weight(version)) {
                    crate::types::LineWeight::ByLayer.value()
                } else {
                    common.line_weight.value()
                } as i64,
            ),
        ),
        (
            "Material",
            optional_handle(
                if legacy.is_some_and(|version| !crate::io::dwg::autocad_roundtrip::stores_material(version)) {
                    None
                } else {
                    material_handle(doc, common)
                },
            ),
        ),
        ("ObjectName", Value::String(object_name.to_string())),
        ("ObjectType", enum_value(object_type)),
        // DwgReaderConfiguration.IgnoreProxyGraphics defaults to true; the
        // DXF builder always ignores them. Retain native graphic_data intact.
        ("ProxyGeometries", array([])),
        ("Reactors", handles(common.reactors.iter().copied())),
        ("SubclassMarker", Value::String(subclass.to_string())),
        (
            "Transparency",
            if super::registration::source_document(doc) {
                super::common::entity_transparency(common)
            } else {
                transparency(common.transparency)
            },
        ),
        ("XDictionary", optional_handle(common.xdictionary_handle)),
    ]
}

fn entity_record(
    doc: &CadDocument,
    entity_common: &EntityCommon,
    type_name: &'static str,
    object_name: &str,
    object_type: i64,
    subclass: &str,
    dynamic: bool,
    mut specific: Vec<(&'static str, Value)>,
) -> Value {
    // Some acadrust entities have no ACadSharp schema counterpart.  Keep
    // those records visible through the common UnknownEntity shape instead
    // of panicking while trying to project a nonexistent type.
    let output_type = if super::schema().contains_key(type_name) {
        type_name
    } else {
        if !specific.iter().any(|(name, _)| *name == "DxfClass") {
            specific.push(("DxfClass", dxf_class(object_name)));
        }
        "UnknownEntity"
    };
    let mut mapped = common(
        doc,
        entity_common,
        object_name,
        object_type,
        subclass,
        dynamic,
    );
    mapped.append(&mut specific);
    record(
        entity_common.handle,
        output_type,
        entity_common.owner_handle,
        props(output_type, mapped),
    )
}

fn inline(type_name: &'static str, mapped: Vec<(&'static str, Value)>) -> Value {
    if super::schema().contains_key(type_name) {
        props(type_name, mapped)
    } else {
        unmapped()
    }
}

fn dxf_class(name: &str) -> Value {
    inline(
        "DxfClass",
        vec![
            ("ApplicationName", unmapped()),
            ("ClassNumber", unmapped()),
            ("CppClassName", unmapped()),
            ("DwgVersion", unmapped()),
            ("DxfName", Value::String(name.to_string())),
            ("InstanceCount", unmapped()),
            ("ProxyFlags", unmapped()),
            ("WasZombie", unmapped()),
        ],
    )
}

fn bytes(value: &[u8]) -> Value {
    Value::String(value.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn field_common(
    doc: &CadDocument,
    common: &EntityCommon,
    type_name: &'static str,
    object_name: &str,
    object_type: i64,
    subclass: &str,
    dynamic: bool,
    specific: Vec<(&'static str, Value)>,
    out: &mut Vec<Value>,
) {
    out.push(entity_record(
        doc,
        common,
        type_name,
        object_name,
        object_type,
        subclass,
        dynamic,
        specific,
    ));
}

fn points3(values: impl IntoIterator<Item = Vector3>) -> Value {
    array(values.into_iter().map(vec3))
}

fn points2(values: impl IntoIterator<Item = Vector2>) -> Value {
    array(values.into_iter().map(vec2))
}

/// Child geometry shares the native common state; only the oracle class varies.
fn vertex_record(
    doc: &CadDocument,
    common: &EntityCommon,
    type_name: &'static str,
    location: Vector3,
    flags: i64,
    bulge: f64,
    tangent: f64,
    start_width: f64,
    end_width: f64,
    id: i32,
) -> Value {
    let (object_type, subclass) = match type_name {
        "Vertex2D" => (0x0A, "AcDb2dVertex"),
        "Vertex3D" => (0x0B, "AcDb3dPolylineVertex"),
        _ => (0x0D, "AcDbPolyFaceMeshVertex"),
    };
    entity_record(
        doc,
        common,
        type_name,
        "VERTEX",
        object_type,
        subclass,
        false,
        vec![
            ("Flags", integer(flags)),
            ("Location", vec3(location)),
            ("Bulge", number(bulge)),
            ("CurveTangent", number(tangent)),
            ("StartWidth", number(start_width)),
            ("EndWidth", number(end_width)),
            ("Id", integer(id)),
        ],
    )
}

fn polyline3d_vertices(
    value: &crate::entities::Polyline3D,
) -> Value {
    if value.vertices.iter().all(|v| !v.handle.is_null()) {
        handles(value.vertices.iter().map(|v| v.handle))
    } else {
        unmapped()
    }
}

fn lw_vertex_inline(vertex: &crate::entities::LwVertex) -> Value {
    inline(
        "LwPolyline.Vertex",
        vec![
            ("Bulge", number(vertex.bulge)),
            // acadrust's lightweight vertex model does not expose the two
            // optional Vertex fields.  ACadSharp initializes both to zero;
            // keep that default in the compatibility view while leaving the
            // native model untouched.
            ("CurveTangent", number(0.0)),
            ("EndWidth", number(vertex.end_width)),
            ("Flags", enum_value(0)),
            ("Id", integer(vertex.vertex_id as i64)),
            ("Location", vec2(vertex.location)),
            ("StartWidth", number(vertex.start_width)),
        ],
    )
}

fn dimension_flags(
    base: &crate::entities::DimensionBase,
    dimension_type: crate::entities::DimensionType,
    ordinate_type_x: bool,
) -> i64 {
    const TYPE_MASK: i64 = 0x0F;
    const ORDINATE_TYPE_X: i64 = 0x40;
    const TEXT_USER_DEFINED: i64 = 0x80;
    const KNOWN_MASK: i64 = TYPE_MASK | ORDINATE_TYPE_X | TEXT_USER_DEFINED;

    let current = (dimension_type as i64 & TYPE_MASK)
        | if ordinate_type_x { ORDINATE_TYPE_X } else { 0 }
        | if base.text_user_positioned {
            TEXT_USER_DEFINED
        } else {
            0
        };

    if let Some(source) = base.dxf_source.as_ref() {
        if let Some(raw) = source.raw_flags.map(i64::from) {
            // Keep unknown DXF bits and the exact source word while the
            // represented type/ordinate/text state is unchanged. If a caller
            // edits one of those modeled bits, update only that bit group.
            let source_known = raw & KNOWN_MASK;
            if source_known == current {
                return raw;
            }
            return (raw & !KNOWN_MASK) | current;
        }
    }

    // ACadSharp constructors add the anonymous dimension-block reference bit
    // to every regular dimension type. DWG's common stream carries the same
    // effective flag through its type-specific object code.
    current | 0x20
}

fn dimension_is_angular(flags: i64) -> bool {
    // ACadSharp uses Enum.HasFlag for both values. This intentionally keeps
    // its bitwise false positives for Diameter and Ordinate.
    flags & 2 != 0 || flags & 5 == 5
}

fn dimension_attachment_point(
    doc: &CadDocument,
    base: &crate::entities::DimensionBase,
) -> i64 {
    // ACadSharp's DWG reader only reads these fields for R2000 and newer
    // files. CLR defaults remain visible for pre-R2000 DWG dimensions.
    if doc
        .dwg_source_version
        .is_some_and(|version| version < crate::types::DxfVersion::AC1015)
    {
        return 0;
    }
    // ACadSharp keeps the word of the record as it is, also when it is no attachment point (0).
    if let Some(raw) = base.source_attachment_point.0 {
        return i64::from(raw);
    }
    let current = base.attachment_point as i16;
    let Some(source) = base.dxf_source.as_ref() else {
        return i64::from(current);
    };

    match source.attachment_point {
        Some(raw)
            if raw == current
                || (current == crate::entities::AttachmentPointType::MiddleCenter as i16
                    && !(1..=9).contains(&raw)) =>
        {
            i64::from(raw)
        }
        Some(_) => i64::from(current),
        None if current == crate::entities::AttachmentPointType::MiddleCenter as i16 => 0,
        None => i64::from(current),
    }
}

fn dimension_line_spacing_factor(
    doc: &CadDocument,
    base: &crate::entities::DimensionBase,
) -> f64 {
    // ACadSharp leaves the optional R2000 dimension fields at CLR defaults
    // when reading pre-R2000 DWG files.
    if doc
        .dwg_source_version
        .is_some_and(|version| version < crate::types::DxfVersion::AC1015)
    {
        return 0.0;
    }
    let Some(source) = base.dxf_source.as_ref() else {
        return base.line_spacing_factor;
    };
    match source.line_spacing_factor {
        Some(value) if value.to_bits() == base.line_spacing_factor.to_bits() => value,
        Some(_) => base.line_spacing_factor,
        None if base.line_spacing_factor.to_bits() == 1.0f64.to_bits() => 0.0,
        None => base.line_spacing_factor,
    }
}

fn dimension_line_spacing_style(
    doc: &CadDocument,
    base: &crate::entities::DimensionBase,
) -> i64 {
    if doc
        .dwg_source_version
        .is_some_and(|version| version < crate::types::DxfVersion::AC1015)
    {
        return 0;
    }
    let Some(source) = base.dxf_source.as_ref() else {
        return i64::from(base.line_spacing_style);
    };
    match source.line_spacing_style {
        Some(value) if value == base.line_spacing_style => i64::from(value),
        Some(_) => i64::from(base.line_spacing_style),
        None if base.line_spacing_style == 1 => 0,
        None => i64::from(base.line_spacing_style),
    }
}

fn dimension_text(base: &crate::entities::DimensionBase) -> Value {
    if let Some(source) = base.dxf_source.as_ref() {
        if !source.text_present && base.text.is_empty() {
            return Value::Null;
        }
    }
    Value::String(base.text.clone())
}

fn dimension_has_style_override(doc: &CadDocument, base: &crate::entities::DimensionBase) -> bool {
    let data = crate::io::dwg::dimension_overrides::view_data(doc, base.common.handle, &base.common.extended_data);
    data.records().iter()
        .find(|record| record.application_name.eq_ignore_ascii_case("ACAD"))
        .and_then(|record| record.values.first())
        .is_some_and(|value| match value {
            crate::xdata::XDataValue::String(value) => {
                // Match the same DWG NUL trimming used by the XData view.
                let value = if doc.dwg_source_version.is_some() {
                    value.trim_end_matches('\0')
                } else { value.as_str() };
                value == "DSTYLE"
            }
            _ => false,
        })
}

fn dimension_common(
    doc: &CadDocument,
    base: &crate::entities::DimensionBase,
    type_name: &'static str,
    object_type: i64,
    subclass: &'static str,
    mut specific: Vec<(&'static str, Value)>,
) -> Value {
    let block = optional_handle(
        (!base.block_name.is_empty())
            .then(|| {
                doc.block_records
                    .get(&base.block_name)
                    .map(|block| block.handle)
            })
            .flatten(),
    );
    let style = optional_handle(
        doc.dim_styles
            .get(&base.style_name)
            .map(|style| style.handle),
    );
    let mut values = common(doc, &base.common, "DIMENSION", object_type, subclass, false);
    values.extend([
        (
            "AttachmentPoint",
            enum_value(dimension_attachment_point(doc, base)),
        ),
        ("Block", block),
        ("DefinitionPoint", vec3(base.definition_point)),
        (
            "Flags",
            enum_value(dimension_flags(base, base.dimension_type, false)),
        ),
        ("FlipArrow1", bool_value(base.flip_arrow1)),
        ("FlipArrow2", bool_value(base.flip_arrow2)),
        (
            "HasStyleOverride",
            bool_value(dimension_has_style_override(doc, base)),
        ),
        ("HorizontalDirection", number(base.horizontal_direction)),
        ("InsertionPoint", vec3(base.insertion_point)),
        (
            "IsAngular",
            bool_value(dimension_is_angular(dimension_flags(
                base,
                base.dimension_type,
                false,
            ))),
        ),
        (
            "IsTextUserDefinedLocation",
            bool_value(base.text_user_positioned),
        ),
        (
            "LineSpacingFactor",
            number(dimension_line_spacing_factor(doc, base)),
        ),
        (
            "LineSpacingStyle",
            enum_value(dimension_line_spacing_style(doc, base)),
        ),
        ("Measurement", number(base.actual_measurement)),
        ("Normal", vec3(base.normal)),
        ("Offset", unmapped()),
        ("Style", style),
        ("Text", dimension_text(base)),
        ("TextMiddlePoint", vec3(base.text_middle_point)),
        ("TextRotation", number(base.text_rotation)),
        ("Version", integer(base.version as i64)),
    ]);
    values.append(&mut specific);
    record(
        base.common.handle,
        type_name,
        base.common.owner_handle,
        props(type_name, values),
    )
}

/// Follow the reference getter, including its exception for zero vectors.
/// CSMath uses the reversed final segment, an unclamped acos, and 1e-12 epsilon.
fn leader_has_hookline(value: &crate::entities::Leader) -> Value {
    if value.vertices.len() <= 1 {
        return bool_value(false);
    }
    let last = value.vertices[value.vertices.len() - 1];
    let previous = value.vertices[value.vertices.len() - 2];
    let segment = previous - last;
    let horizontal = value.horizontal_direction;
    let segment_length = segment.length();
    let horizontal_length = horizontal.length();
    if segment_length <= 1e-12 || horizontal_length <= 1e-12 {
        return Value::String("!throws:InvalidOperationException".into());
    }
    let angle = (segment.dot(&horizontal) / (segment_length * horizontal_length)).acos();
    bool_value(angle.abs() <= 1e-12)
}

/// Resolve the computed `Insert.SpatialFilter` property through the same
/// dictionary path used by ACadSharp: the insert's extension dictionary owns
/// an `ACAD_FILTER` dictionary, whose `SPATIAL` entry points at the filter.
/// Keep this lookup in the compatibility projection; the native model already
/// retains the dictionaries and the typed filter object independently.
fn insert_spatial_filter(doc: &CadDocument, value: &crate::entities::Insert) -> Value {
    let Some(xdictionary) = value
        .common
        .xdictionary_handle
        .filter(|handle| !handle.is_null())
        .or_else(|| doc.extension_dictionary_handle(value.common.handle))
    else {
        return Value::Null;
    };
    let Some(crate::objects::ObjectType::Dictionary(extension)) = doc.objects.get(&xdictionary)
    else {
        return Value::Null;
    };
    let Some(filter_dictionary) = extension
        .entries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("ACAD_FILTER"))
        .map(|(_, handle)| *handle)
        .and_then(|handle| match doc.objects.get(&handle) {
            Some(crate::objects::ObjectType::Dictionary(dictionary)) => Some(dictionary),
            _ => None,
        })
    else {
        return Value::Null;
    };
    let Some(filter_handle) = filter_dictionary
        .entries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("SPATIAL"))
        .map(|(_, handle)| *handle)
        .filter(|handle| {
            matches!(
                doc.objects.get(handle),
                Some(crate::objects::ObjectType::SpatialFilter(_))
            )
        })
    else {
        return Value::Null;
    };
    optional_handle(Some(filter_handle))
}

/// ACadSharp's R12 DXF registration only links an ATTRIB when its source
/// record carries an explicit owner handle. Native INSERT parsing groups the
/// trailing sequence structurally even when the source owner is absent, so
/// project the collection using the reader's retained source-owner map while
/// leaving the attribute records themselves available to the block collection.
fn insert_attributes<'a>(
    doc: &CadDocument,
    value: &'a crate::entities::Insert,
    legacy: bool,
) -> Vec<&'a crate::entities::AttributeEntity> {
    if !legacy {
        return value.attributes.iter().collect();
    }
    let Some(source_owners) = doc.source_entity_owners.as_ref() else {
        return value.attributes.iter().collect();
    };
    value
        .attributes
        .iter()
        .filter(|attribute| {
            source_owners
                .get(&attribute.common.handle)
                .is_some_and(|owner| *owner == value.common.handle)
        })
        .collect()
}

#[derive(Copy, Clone)]
enum RecordPart {
    Parent,
    Seqend,
    InsertAttribute(usize),
    Polyline2DVertex(usize),
    Polyline3DVertex(usize),
    PolyfaceVertex(usize),
    PolyfaceFace(usize),
    PolygonMeshVertex(usize),
}

/// One output record backed by one native entity and, where applicable, one
/// retained child.  The descriptor owns no projected data; rendering it
/// allocates only the single canonical record being requested.
#[derive(Copy, Clone)]
pub(super) struct Record<'a> {
    entity: &'a EntityType,
    part: RecordPart,
}

impl<'a> Record<'a> {
    pub(super) fn render(&self, doc: &CadDocument, reader: &str) -> Value {
        let legacy = reader == "dxf"
            && matches!(
                doc.version,
                crate::types::DxfVersion::AC1009 | crate::types::DxfVersion::Unknown
            );
        let mut out = Vec::with_capacity(1);
        collect_entity_part(doc, self.entity, legacy, self.part, &mut out);
        debug_assert_eq!(out.len(), 1);
        out.pop()
            .expect("entity record descriptor rendered no record")
    }

    /// Render only the identity and registration fields needed before the
    /// final canonical pass. Ordinary entities use the native common state
    /// directly; opaque projections fall back to the full renderer and then
    /// discard every property not needed by registration.
    pub(super) fn metadata(&self, doc: &CadDocument, reader: &str) -> Value {
        let legacy = reader == "dxf"
            && matches!(
                doc.version,
                crate::types::DxfVersion::AC1009 | crate::types::DxfVersion::Unknown
            );
        if let Some(value) = metadata_fast(doc, self.entity, self.part, legacy) {
            return value;
        }
        metadata_from_render(self.render(doc, reader))
    }
}

struct MetadataIdentity {
    handle: Handle,
    owner: Handle,
    type_name: &'static str,
    subclass: &'static str,
    xdictionary: Option<Handle>,
    block_name: Option<String>,
    include_xdictionary: bool,
}

fn metadata_record(
    handle_value: Handle,
    type_name: &'static str,
    owner: Handle,
    subclass: &'static str,
    xdictionary: Option<Handle>,
    block_name: Option<&str>,
    include_xdictionary: bool,
) -> Value {
    let mut mapped = Map::new();
    // Registration only needs presence to distinguish graph entities. The
    // final render retains the canonical empty array.
    mapped.insert("ProxyGeometries".into(), Value::Null);
    mapped.insert("SubclassMarker".into(), Value::String(subclass.into()));
    if include_xdictionary {
        mapped.insert("XDictionary".into(), optional_handle(xdictionary));
    }
    if let Some(name) = block_name {
        mapped.insert("Name".into(), Value::String(name.to_owned()));
    }
    record(handle_value, type_name, owner, Value::Object(mapped))
}

fn metadata_from_render(mut value: Value) -> Value {
    let mut mapped = Map::new();
    for key in ["ProxyGeometries", "SubclassMarker", "XDictionary"] {
        if let Some(property) = value["props"].get(key) {
            mapped.insert(key.to_owned(), property.clone());
        }
    }
    if value["type"] == "Block" {
        if let Some(name) = value["props"].get("Name") {
            mapped.insert("Name".into(), name.clone());
        }
    }
    value["props"] = Value::Object(mapped);
    value
}

fn metadata_identity(
    doc: &CadDocument,
    entity: &EntityType,
    legacy: bool,
) -> Option<MetadataIdentity> {
    let common = entity.common();
    let (type_name, subclass, block_name) = match entity {
        EntityType::Point(_) => ("Point", "AcDbPoint", None),
        EntityType::Line(_) => ("Line", "AcDbLine", None),
        EntityType::Circle(_) => ("Circle", "AcDbCircle", None),
        EntityType::Arc(_) => ("Arc", "AcDbArc", None),
        EntityType::Ellipse(_) => ("Ellipse", "AcDbEllipse", None),
        EntityType::Polyline(_) => ("Polyline3D", "AcDb3dPolyline", None),
        EntityType::Polyline2D(_) => ("Polyline2D", "AcDb2dPolyline", None),
        EntityType::Polyline3D(_) => {
            if legacy {
                ("Polyline2D", "AcDb2dPolyline", None)
            } else {
                ("Polyline3D", "AcDb3dPolyline", None)
            }
        }
        EntityType::LwPolyline(_) => ("LwPolyline", "AcDbPolyline", None),
        EntityType::Text(_) => ("TextEntity", "AcDbText", None),
        EntityType::MText(_) => ("MText", "AcDbMText", None),
        EntityType::Spline(_) | EntityType::Helix(_) => ("Spline", "AcDbSpline", None),
        EntityType::Dimension(value) => match value.as_ref() {
            Dimension::Aligned(_) => ("DimensionAligned", "AcDbAlignedDimension", None),
            Dimension::Linear(_) => ("DimensionLinear", "AcDbRotatedDimension", None),
            Dimension::Radius(_) => ("DimensionRadius", "AcDbRadialDimension", None),
            Dimension::Diameter(_) => ("DimensionDiameter", "AcDbDiametricDimension", None),
            Dimension::Angular2Ln(_) => {
                ("DimensionAngular2Line", "AcDb2LineAngularDimension", None)
            }
            Dimension::Angular3Pt(_) => ("DimensionAngular3Pt", "AcDb3PointAngularDimension", None),
            Dimension::Ordinate(_) => ("DimensionOrdinate", "AcDbOrdinateDimension", None),
            Dimension::Arc(_) => ("DimensionArc", "AcDbArcDimension", None),
            Dimension::LargeRadial(_) => ("DimensionRadius", "AcDbRadialDimensionLarge", None),
        },
        EntityType::Hatch(_) => ("Hatch", "AcDbHatch", None),
        EntityType::Solid(_) => ("Solid", "AcDbTrace", None),
        EntityType::Face3D(_) => ("Face3D", "AcDbFace", None),
        EntityType::Insert(value) => (
            "Insert",
            if value.is_array() {
                "AcDbMInsertBlock"
            } else {
                "AcDbBlockReference"
            },
            None,
        ),
        EntityType::Block(value) => {
            let name = doc
                .block_records
                .get(&value.name)
                .filter(|block| block.handle == common.owner_handle)
                .map(|block| super::tables::block_name(doc, block).to_owned())
                .unwrap_or_else(|| value.name.clone());
            ("Block", "AcDbBlockBegin", Some(name))
        }
        EntityType::BlockEnd(_) => ("BlockEnd", "AcDbBlockEnd", None),
        EntityType::Ray(_) => ("Ray", "AcDbRay", None),
        EntityType::XLine(_) => ("XLine", "AcDbXline", None),
        EntityType::Viewport(_) => ("Viewport", "AcDbViewport", None),
        EntityType::AttributeDefinition(_) => {
            ("AttributeDefinition", "AcDbAttributeDefinition", None)
        }
        EntityType::AttributeEntity(_) => ("AttributeEntity", "AcDbAttribute", None),
        EntityType::Leader(_) => ("Leader", "AcDbLeader", None),
        EntityType::MultiLeader(_) => ("MultiLeader", "AcDbMLeader", None),
        EntityType::MLine(_) => ("MLine", "AcDbMline", None),
        EntityType::Mesh(_) => ("Mesh", "AcDbSubDMesh", None),
        EntityType::RasterImage(_) => ("RasterImage", "AcDbRasterImage", None),
        EntityType::Solid3D(_) => ("Solid3D", "AcDb3dSolid", None),
        EntityType::Region(_) => ("Region", "AcDbModelerGeometry", None),
        EntityType::Body(_) => ("CadBody", "AcDbModelerGeometry", None),
        EntityType::Surface(value) => {
            let (type_name, _, subclass) = surface_identity(value.kind);
            (type_name, subclass, None)
        }
        EntityType::Table(_) => ("TableEntity", "AcDbTable", None),
        EntityType::Tolerance(_) => ("Tolerance", "AcDbFcf", None),
        EntityType::PolyfaceMesh(_) => {
            if legacy {
                ("Polyline2D", "AcDb2dPolyline", None)
            } else {
                ("PolyfaceMesh", "AcDbPolyFaceMesh", None)
            }
        }
        EntityType::PolygonMesh(_) => {
            if legacy {
                ("Polyline2D", "AcDb2dPolyline", None)
            } else {
                ("PolygonMesh", "AcDbPolygonMesh", None)
            }
        }
        EntityType::Wipeout(_) => ("Wipeout", "AcDbWipeout", None),
        EntityType::Shape(_) => ("Shape", "AcDbShape", None),
        EntityType::Underlay(value) => (
            match value.underlay_type {
                crate::entities::UnderlayType::Pdf => "PdfUnderlay",
                crate::entities::UnderlayType::Dwf => "DwfUnderlay",
                crate::entities::UnderlayType::Dgn => "DgnUnderlay",
            },
            "AcDbUnderlayReference",
            None,
        ),
        EntityType::Seqend(_) => ("Seqend", "AcDbEntity", None),
        EntityType::Ole2Frame(_) => ("Ole2Frame", "AcDbOle2Frame", None),
        EntityType::Light(_) => ("Light", "AcDbLight", None),
        EntityType::SectionSymbol(_) => ("SectionSymbol", "AcDbSection", None),
        EntityType::ViewBorder(_) => ("ViewBorder", "AcDbViewBorder", None),
        EntityType::Extended(value) => match &value.data {
            crate::entities::ExtendedEntityData::Proxy(_) => {
                ("ProxyEntity", "AcDbProxyEntity", None)
            }
            crate::entities::ExtendedEntityData::OleFrame(_) => {
                ("Ole2Frame", "AcDbOle2Frame", None)
            }
            _ => {
                let type_name = super::type_names::canonical(value.class_name())
                    .filter(|name| super::schema().contains_key(*name))
                    .unwrap_or("UnknownEntity");
                (type_name, "AcDbEntity", None)
            }
        },
        // A valid AEC wall has an opaque, class-specific projection. Let the
        // fallback preserve its exact type and registration fields.
        EntityType::Unknown(value) if value.dxf_name == "AEC_WALL" => return None,
        EntityType::Unknown(value) => {
            let type_name = super::type_names::canonical(&value.dxf_name)
                .filter(|name| super::schema().contains_key(*name))
                .unwrap_or("UnknownEntity");
            (type_name, "AcDbEntity", None)
        }
    };
    Some(MetadataIdentity {
        handle: common.handle,
        owner: common.owner_handle,
        type_name: if super::schema().contains_key(type_name) {
            type_name
        } else {
            "UnknownEntity"
        },
        subclass,
        xdictionary: common.xdictionary_handle.filter(|h| !h.is_null()),
        block_name,
        include_xdictionary: true,
    })
}

fn metadata_fast(
    doc: &CadDocument,
    entity: &EntityType,
    part: RecordPart,
    legacy: bool,
) -> Option<Value> {
    match part {
        RecordPart::Parent => {
            let identity = metadata_identity(doc, entity, legacy)?;
            Some(metadata_record(
                identity.handle,
                identity.type_name,
                identity.owner,
                identity.subclass,
                identity.xdictionary,
                identity.block_name.as_deref(),
                identity.include_xdictionary,
            ))
        }
        RecordPart::Seqend => {
            let common = match entity {
                EntityType::Insert(value) => &value.seqend.as_ref()?.common,
                EntityType::Polyline2D(value) => &value.seqend.as_ref()?.common,
                EntityType::Polyline3D(value) => &value.seqend.as_ref()?.common,
                EntityType::PolyfaceMesh(value) => &value.seqend.as_ref()?.common,
                EntityType::PolygonMesh(value) => &value.seqend.as_ref()?.common,
                _ => return None,
            };
            let xdictionary = if legacy && !matches!(entity, EntityType::Insert(_)) {
                None
            } else {
                common.xdictionary_handle.filter(|h| !h.is_null())
            };
            Some(metadata_record(
                common.handle,
                "Seqend",
                common.owner_handle,
                "AcDbEntity",
                xdictionary,
                None,
                true,
            ))
        }
        RecordPart::InsertAttribute(index) => {
            let EntityType::Insert(value) = entity else {
                return None;
            };
            let attribute = value.attributes.get(index)?;
            Some(metadata_record(
                attribute.common.handle,
                "AttributeEntity",
                value.common.handle,
                "AcDbAttribute",
                attribute.common.xdictionary_handle.filter(|h| !h.is_null()),
                None,
                true,
            ))
        }
        RecordPart::Polyline2DVertex(index) => {
            let EntityType::Polyline2D(value) = entity else {
                return None;
            };
            let vertex = value.vertices.get(index)?;
            Some(metadata_record(
                vertex.common.handle,
                "Vertex2D",
                vertex.common.owner_handle,
                "AcDb2dVertex",
                vertex.common.xdictionary_handle.filter(|h| !h.is_null()),
                None,
                true,
            ))
        }
        RecordPart::Polyline3DVertex(index) => {
            let EntityType::Polyline3D(value) = entity else {
                return None;
            };
            let vertex = value.vertices.get(index)?;
            if vertex.common.owner_handle.is_null() {
                return Some(metadata_record(
                    vertex.handle,
                    if legacy { "Vertex2D" } else { "Vertex3D" },
                    value.common.handle,
                    if legacy {
                        "AcDb2dVertex"
                    } else {
                        "AcDb3dPolylineVertex"
                    },
                    None,
                    None,
                    false,
                ));
            }
            Some(metadata_record(
                vertex.handle,
                if legacy { "Vertex2D" } else { "Vertex3D" },
                vertex.common.owner_handle,
                if legacy {
                    "AcDb2dVertex"
                } else {
                    "AcDb3dPolylineVertex"
                },
                vertex.common.xdictionary_handle.filter(|h| !h.is_null()),
                None,
                true,
            ))
        }
        RecordPart::PolyfaceVertex(index) => {
            let EntityType::PolyfaceMesh(value) = entity else {
                return None;
            };
            let vertex = value.vertices.get(index)?;
            Some(metadata_record(
                vertex.common.handle,
                if legacy { "Vertex2D" } else { "VertexFaceMesh" },
                vertex.common.owner_handle,
                if legacy {
                    "AcDb2dVertex"
                } else {
                    "AcDbPolyFaceMeshVertex"
                },
                vertex.common.xdictionary_handle.filter(|h| !h.is_null()),
                None,
                true,
            ))
        }
        RecordPart::PolyfaceFace(index) => {
            let EntityType::PolyfaceMesh(value) = entity else {
                return None;
            };
            let face = value.faces.get(index)?;
            Some(metadata_record(
                face.common.handle,
                if legacy {
                    "Vertex2D"
                } else {
                    "VertexFaceRecord"
                },
                face.common.owner_handle,
                if legacy {
                    "AcDb2dVertex"
                } else {
                    "AcDbFaceRecord"
                },
                face.common.xdictionary_handle.filter(|h| !h.is_null()),
                None,
                true,
            ))
        }
        RecordPart::PolygonMeshVertex(index) => {
            let EntityType::PolygonMesh(value) = entity else {
                return None;
            };
            let vertex = value.vertices.get(index)?;
            Some(metadata_record(
                vertex.common.handle,
                if legacy {
                    "Vertex2D"
                } else {
                    "PolygonMeshVertex"
                },
                vertex.common.owner_handle,
                if legacy {
                    "AcDb2dVertex"
                } else {
                    "AcDbPolygonMeshVertex"
                },
                vertex.common.xdictionary_handle.filter(|h| !h.is_null()),
                None,
                true,
            ))
        }
    }
}

/// Enumerate entity records in the same order as the compatibility collector.
/// Structural children are represented by descriptors and rendered only when
/// requested, so a child never requires materializing its siblings.
pub(super) fn records<'a>(doc: &'a CadDocument, _reader: &str) -> Vec<Record<'a>> {
    let mut result = Vec::new();
    for stored in &doc.entities {
        let entity = stored.as_ref();
        let seqend = match entity {
            EntityType::Insert(e) => e.seqend.as_ref(),
            EntityType::Polyline2D(e) => e.seqend.as_ref(),
            EntityType::Polyline3D(e) => e.seqend.as_ref(),
            EntityType::PolyfaceMesh(e) => e.seqend.as_ref(),
            EntityType::PolygonMesh(e) => e.seqend.as_ref(),
            _ => None,
        };
        if seqend.is_some_and(|value| !value.common.handle.is_null()) {
            result.push(Record {
                entity,
                part: RecordPart::Seqend,
            });
        }
        match entity {
            EntityType::Insert(value) => {
                for (index, _) in value.attributes.iter().enumerate() {
                    result.push(Record {
                        entity,
                        part: RecordPart::InsertAttribute(index),
                    });
                }
            }
            EntityType::Polyline2D(value) => {
                for (index, vertex) in value.vertices.iter().enumerate() {
                    if !vertex.common.handle.is_null() {
                        result.push(Record {
                            entity,
                            part: RecordPart::Polyline2DVertex(index),
                        });
                    }
                }
            }
            EntityType::Polyline3D(value) => {
                for (index, vertex) in value.vertices.iter().enumerate() {
                    if !vertex.handle.is_null() {
                        result.push(Record {
                            entity,
                            part: RecordPart::Polyline3DVertex(index),
                        });
                    }
                }
            }
            EntityType::PolyfaceMesh(value) => {
                for (index, vertex) in value.vertices.iter().enumerate() {
                    if !vertex.common.handle.is_null() {
                        result.push(Record {
                            entity,
                            part: RecordPart::PolyfaceVertex(index),
                        });
                    }
                }
                for (index, face) in value.faces.iter().enumerate() {
                    if !face.common.handle.is_null() {
                        result.push(Record {
                            entity,
                            part: RecordPart::PolyfaceFace(index),
                        });
                    }
                }
            }
            EntityType::PolygonMesh(value) => {
                for (index, vertex) in value.vertices.iter().enumerate() {
                    if !vertex.common.handle.is_null() {
                        result.push(Record {
                            entity,
                            part: RecordPart::PolygonMeshVertex(index),
                        });
                    }
                }
            }
            _ => {}
        }
        result.push(Record {
            entity,
            part: RecordPart::Parent,
        });
    }
    result
}

/// Visit the backing collection, including structural BLOCK/ENDBLK records
/// hidden by the public drawing-entity iterator, and retained children.
pub(super) fn collect(doc: &CadDocument, reader: &str, out: &mut Vec<Value>) {
    for record in records(doc, reader) {
        out.push(record.render(doc, reader));
    }
}

fn collect_entity_part(
    doc: &CadDocument,
    entity: &EntityType,
    legacy: bool,
    part: RecordPart,
    out: &mut Vec<Value>,
) {
    let first = out.len();
    if matches!(part, RecordPart::Seqend) {
        let seqend = match entity {
            EntityType::Insert(e) => e.seqend.as_ref(),
            EntityType::Polyline2D(e) => e.seqend.as_ref(),
            EntityType::Polyline3D(e) => e.seqend.as_ref(),
            EntityType::PolyfaceMesh(e) => e.seqend.as_ref(),
            EntityType::PolygonMesh(e) => e.seqend.as_ref(),
            _ => None,
        };
        let seqend = seqend
            .filter(|value| !value.common.handle.is_null())
            .expect("sequence descriptor without sequence entity");
        field_common(
            doc,
            &seqend.common,
            "Seqend",
            "SEQEND",
            6,
            "AcDbEntity",
            false,
            vec![],
            out,
        );
        if legacy && !matches!(entity, EntityType::Insert(_)) {
            // readLegacyPolyline assigns the Seqend directly and never adds
            // its template to the builder. Scalar codes survive; deferred
            // table references, reactors and XData therefore stay defaults.
            let p = &mut out.last_mut().expect("sequence marker")["props"];
            p["Layer"] = optional_handle(doc.layers.get("0").map(|v| v.handle));
            p["LineType"] = optional_handle(doc.line_types.get("ByLayer").map(|v| v.handle));
            p["Material"] = Value::Null;
            p["BookColor"] = Value::Null;
            p["XDictionary"] = Value::Null;
            p["Reactors"] = json!([]);
            p["ExtendedData"] = json!([]);
        }
        return;
    }
    match entity {
        EntityType::Point(value) => field_common(
            doc,
            &value.common,
            "Point",
            "POINT",
            0x1B,
            "AcDbPoint",
            false,
            vec![
                ("Location", vec3(value.location)),
                ("Normal", vec3(value.normal)),
                ("Rotation", number(value.x_axis_angle)),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Line(value) => field_common(
            doc,
            &value.common,
            "Line",
            "LINE",
            0x13,
            "AcDbLine",
            false,
            vec![
                ("StartPoint", vec3(value.start)),
                ("EndPoint", vec3(value.end)),
                ("Normal", vec3(value.normal)),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Circle(value) => field_common(
            doc,
            &value.common,
            "Circle",
            "CIRCLE",
            0x12,
            "AcDbCircle",
            false,
            vec![
                ("Center", vec3(value.center)),
                ("Normal", vec3(value.normal)),
                ("Radius", number(value.radius)),
                ("RadiusRatio", number(1.0)),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Arc(value) => field_common(
            doc,
            &value.common,
            "Arc",
            "ARC",
            0x11,
            "AcDbArc",
            false,
            vec![
                ("Center", vec3(value.center)),
                ("EndAngle", number(value.end_angle)),
                ("Normal", vec3(value.normal)),
                ("Radius", number(value.radius)),
                ("RadiusRatio", number(1.0)),
                ("StartAngle", number(value.start_angle)),
                (
                    "Sweep",
                    number(value.start_angle - if value.end_angle < value.start_angle {
                        value.end_angle + std::f64::consts::TAU
                    } else {
                        value.end_angle
                    }),
                ),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Ellipse(value) => field_common(
            doc,
            &value.common,
            "Ellipse",
            "ELLIPSE",
            0x23,
            "AcDbEllipse",
            false,
            vec![
                ("Center", vec3(value.center)),
                ("EndParameter", number(value.end_parameter)),
                (
                    "IsFullEllipse",
                    bool_value(
                        value.start_parameter == 0.0
                            && value.end_parameter == std::f64::consts::TAU,
                    ),
                ),
                ("MajorAxis", number(value.major_axis_length() * 2.0)),
                ("MajorAxisEndPoint", vec3(value.major_axis)),
                (
                    "MinorAxis",
                    number(value.major_axis_length() * value.minor_axis_ratio * 2.0),
                ),
                (
                    "MinorAxisEndPoint",
                    vec3(
                        value
                            .normal
                            .normalize()
                            .cross(&value.major_axis.normalize())
                            .normalize()
                            * (value.major_axis_length() * value.minor_axis_ratio),
                    ),
                ),
                ("Normal", vec3(value.normal)),
                ("RadiusRatio", number(value.minor_axis_ratio)),
                (
                    "Rotation",
                    number(value.major_axis.y.atan2(value.major_axis.x)),
                ),
                ("StartParameter", number(value.start_parameter)),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Polyline(value) => {
            field_common(
                doc,
                &value.common,
                "Polyline3D",
                "POLYLINE",
                0x10,
                "AcDb3dPolyline",
                false,
                vec![
                    ("Elevation", unmapped()),
                    ("EndWidth", unmapped()),
                    ("Flags", enum_value(value.flags.bits() as i64)),
                    ("IsClosed", bool_value(value.is_closed())),
                    ("MatchVerticesEntityProperties", unmapped()),
                    ("Normal", unmapped()),
                    ("SmoothSurface", unmapped()),
                    ("StartWidth", unmapped()),
                    ("Thickness", unmapped()),
                    // Heavy-polyline vertices are value structs in acadrust;
                    // their Seqend/CadObject identities are not retained.
                    ("Vertices", unmapped()),
                ],
                out,
            );
        }
        EntityType::Polyline2D(value) => {
            if let RecordPart::Polyline2DVertex(index) = part {
                let vertex = value
                    .vertices
                    .get(index)
                    .filter(|vertex| !vertex.common.handle.is_null())
                    .expect("polyline vertex descriptor without vertex");
                let (start, end) = vertex
                    .source_widths
                    .unwrap_or((vertex.start_width, vertex.end_width));
                out.push(vertex_record(
                    doc,
                    &vertex.common,
                    "Vertex2D",
                    vertex.location,
                    vertex.flags.bits() as i64,
                    vertex.bulge,
                    vertex.curve_tangent,
                    start,
                    end,
                    vertex.id,
                ));
            } else {
                let (start, end) = value
                    .source_widths
                    .unwrap_or((value.start_width, value.end_width));
                field_common(
                    doc,
                    &value.common,
                    "Polyline2D",
                    "POLYLINE",
                    0x0F,
                    "AcDb2dPolyline",
                    false,
                    vec![
                        ("Elevation", number(value.elevation)),
                        ("EndWidth", number(end)),
                        ("Flags", enum_value(value.flags.bits() as i64)),
                        ("IsClosed", bool_value(value.is_closed())),
                        ("MatchVerticesEntityProperties", bool_value(true)),
                        ("Normal", vec3(value.normal)),
                        ("SmoothSurface", enum_value(value.smooth_surface as i64)),
                        ("StartWidth", number(start)),
                        ("Thickness", number(value.thickness)),
                        (
                            "Vertices",
                            if value.vertices.iter().all(|v| !v.common.handle.is_null()) {
                                handles(value.vertices.iter().map(|v| v.common.handle))
                            } else {
                                unmapped()
                            },
                        ),
                    ],
                    out,
                );
            }
        }
        EntityType::Polyline3D(value) => {
            if let RecordPart::Polyline3DVertex(index) = part {
                let vertex = value
                    .vertices
                    .get(index)
                    .filter(|vertex| !vertex.handle.is_null())
                    .expect("polyline vertex descriptor without vertex");
                if vertex.common.owner_handle.is_null() {
                    let type_name = if legacy { "Vertex2D" } else { "Vertex3D" };
                    let layer = doc.layers.get(&vertex.layer).map(|layer| layer.handle);
                    out.push(record(
                        vertex.handle,
                        type_name,
                        value.common.handle,
                        props(
                            type_name,
                            vec![
                                ("Flags", integer(vertex.flags)),
                                ("Layer", optional_handle(layer)),
                                ("Location", vec3(vertex.position)),
                                ("ObjectName", Value::String("VERTEX".into())),
                                ("ObjectType", enum_value(if legacy { 0x0A } else { 0x0B })),
                                ("ProxyGeometries", array([])),
                                (
                                    "SubclassMarker",
                                    Value::String(
                                        if legacy {
                                            "AcDb2dVertex"
                                        } else {
                                            "AcDb3dPolylineVertex"
                                        }
                                        .into(),
                                    ),
                                ),
                            ],
                        ),
                    ));
                } else {
                    out.push(vertex_record(
                        doc,
                        &vertex.common,
                        if legacy { "Vertex2D" } else { "Vertex3D" },
                        vertex.position,
                        vertex.flags as i64,
                        0.0,
                        0.0,
                        0.0,
                        0.0,
                        0,
                    ));
                }
            } else {
                let vertices = polyline3d_vertices(value);
                field_common(
                    doc,
                    &value.common,
                    if legacy { "Polyline2D" } else { "Polyline3D" },
                    "POLYLINE",
                    if legacy { 0x0F } else { 0x10 },
                    if legacy {
                        "AcDb2dPolyline"
                    } else {
                        "AcDb3dPolyline"
                    },
                    false,
                    vec![
                        ("Elevation", number(value.elevation)),
                        ("EndWidth", number(value.default_end_width)),
                        ("Flags", enum_value(value.flags.to_bits() as i64)),
                        ("IsClosed", bool_value(value.is_closed())),
                        ("MatchVerticesEntityProperties", bool_value(true)),
                        ("Normal", vec3(value.normal)),
                        (
                            "SmoothSurface",
                            enum_value(value.smooth_type.to_value() as i64),
                        ),
                        ("StartWidth", number(value.default_start_width)),
                        ("Thickness", number(value.thickness)),
                        ("Vertices", vertices),
                    ],
                    out,
                );
            }
        }
        EntityType::LwPolyline(value) => field_common(
            doc,
            &value.common,
            "LwPolyline",
            "LWPOLYLINE",
            0x4D,
            "AcDbPolyline",
            false,
            vec![
                ("ConstantWidth", number(value.constant_width)),
                ("Elevation", number(value.elevation)),
                (
                    "Flags",
                    enum_value(i64::from(value.is_closed) | (i64::from(value.plinegen) << 7)),
                ),
                ("IsClosed", bool_value(value.is_closed)),
                ("Normal", vec3(value.normal)),
                ("Thickness", number(value.thickness)),
                (
                    "Vertices",
                    array(value.vertices.iter().map(lw_vertex_inline)),
                ),
            ],
            out,
        ),
        EntityType::Text(value) => field_common(
            doc,
            &value.common,
            "TextEntity",
            "TEXT",
            1,
            "AcDbText",
            false,
            vec![
                (
                    "AlignmentPoint",
                    vec3(value.alignment_point.unwrap_or(Vector3::ZERO)),
                ),
                ("Height", number(value.height)),
                (
                    "HorizontalAlignment",
                    enum_value(text_horizontal(value.horizontal_alignment)),
                ),
                ("InsertPoint", vec3(value.insertion_point)),
                ("Mirror", enum_value(value.generation_flags as i64)),
                ("Normal", vec3(value.normal)),
                ("ObliqueAngle", number(value.oblique_angle)),
                ("Rotation", number(value.rotation)),
                ("Thickness", number(value.thickness)),
                ("Style", text_style(doc, &value.style)),
                (
                    "Value",
                    dwg_legacy_replacement(
                        doc,
                        text_source_value(&value.value, value.source.as_deref()),
                    ),
                ),
                (
                    "VerticalAlignment",
                    enum_value(text_vertical(value.vertical_alignment)),
                ),
                ("WidthFactor", number(value.width_factor)),
            ],
            out,
        ),
        EntityType::MText(value) => field_common(
            doc,
            &mtext_common(doc, value),
            "MText",
            "MTEXT",
            0x2C,
            "AcDbMText",
            false,
            super::mtext::properties(doc, value),
            out,
        ),
        EntityType::AttributeDefinition(value) => field_common(
            doc,
            &value.common,
            "AttributeDefinition",
            "ATTDEF",
            3,
            "AcDbAttributeDefinition",
            false,
            attribute_definition_props(doc, value),
            out,
        ),
        EntityType::AttributeEntity(value) => field_common(
            doc,
            &value.common,
            "AttributeEntity",
            "ATTRIB",
            2,
            "AcDbAttribute",
            false,
            attribute_entity_props(doc, value),
            out,
        ),
        EntityType::Insert(value) => {
            if let RecordPart::InsertAttribute(index) = part {
                let attribute = value
                    .attributes
                    .get(index)
                    .expect("insert attribute descriptor without attribute");
                collect_attribute_inline(doc, attribute, value.common.handle, out);
            } else {
                let attributes = insert_attributes(doc, value, legacy);
                let attribute_handles = attributes
                    .iter()
                    .map(|attribute| handle(attribute.common.handle))
                    .collect::<Vec<_>>();
                field_common(
                    doc,
                    &value.common,
                    "Insert",
                    "INSERT",
                    if value.is_array() { 8 } else { 7 },
                    if value.is_array() {
                        "AcDbMInsertBlock"
                    } else {
                        "AcDbBlockReference"
                    },
                    true,
                    vec![
                        ("Attributes", array(attribute_handles.into_iter())),
                        (
                            "Block",
                            optional_handle(
                                doc.block_records
                                    .get(&value.block_name)
                                    .map(|block| block.handle),
                            ),
                        ),
                        ("ColumnCount", integer(value.column_count as i64)),
                        ("ColumnSpacing", number(value.column_spacing)),
                        ("HasAttributes", bool_value(!attributes.is_empty())),
                        ("IsMultiple", bool_value(value.is_array())),
                        ("InsertPoint", vec3(value.insert_point)),
                        ("Normal", vec3(value.normal)),
                        ("Rotation", number(value.rotation)),
                        ("RowCount", integer(value.row_count as i64)),
                        ("RowSpacing", number(value.row_spacing)),
                        ("SpatialFilter", insert_spatial_filter(doc, value)),
                        ("XScale", number(value.x_scale())),
                        ("YScale", number(value.y_scale())),
                        ("ZScale", number(value.z_scale())),
                    ],
                    out,
                );
            }
        }
        EntityType::Solid(value) => field_common(
            doc,
            &value.common,
            "Solid",
            "SOLID",
            0x1F,
            "AcDbTrace",
            false,
            vec![
                ("FirstCorner", vec3(value.first_corner)),
                ("FourthCorner", vec3(value.fourth_corner)),
                ("Normal", vec3(value.normal)),
                ("SecondCorner", vec3(value.second_corner)),
                ("ThirdCorner", vec3(value.third_corner)),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Face3D(value) => field_common(
            doc,
            &value.common,
            "Face3D",
            "3DFACE",
            0x1C,
            "AcDbFace",
            false,
            vec![
                ("FirstCorner", vec3(value.first_corner)),
                ("Flags", enum_value(value.invisible_edges.bits() as i64)),
                ("FourthCorner", vec3(value.fourth_corner)),
                ("SecondCorner", vec3(value.second_corner)),
                ("ThirdCorner", vec3(value.third_corner)),
            ],
            out,
        ),
        EntityType::Ray(value) => field_common(
            doc,
            &value.common,
            "Ray",
            "RAY",
            0x28,
            "AcDbRay",
            false,
            vec![
                ("Direction", vec3(value.direction)),
                ("StartPoint", vec3(value.base_point)),
            ],
            out,
        ),
        EntityType::XLine(value) => field_common(
            doc,
            &value.common,
            "XLine",
            "XLINE",
            0x29,
            "AcDbXline",
            false,
            vec![
                ("Direction", vec3(value.direction)),
                ("FirstPoint", vec3(value.base_point)),
            ],
            out,
        ),
        EntityType::Spline(value) => {
            let flags = spline_flags(doc, value);
            let flags1 = spline_flags1(doc, value);
            let is_closed = flags & 1 != 0;
            let is_periodic = if flags1 & 1 != 0 {
                flags1 & 4 != 0
            } else {
                flags & 2 != 0
            };
            field_common(
                doc,
                &value.common,
                "Spline",
                "SPLINE",
                0x24,
                "AcDbSpline",
                false,
                vec![
                    (
                        "ControlPointTolerance",
                        number(spline_control_point_tolerance(value)),
                    ),
                    (
                        "ControlPoints",
                        points3(value.control_points.iter().copied()),
                    ),
                    ("Degree", integer(value.degree as i64)),
                    ("EndTangent", vec3(value.end_tangent)),
                    ("FitPoints", points3(value.fit_points.iter().copied())),
                    ("FitTolerance", number(spline_fit_tolerance(value))),
                    ("Flags", enum_value(flags)),
                    ("Flags1", enum_value(flags1)),
                    (
                        "HasValidKnotCount",
                        bool_value(spline_has_valid_knot_count(value, is_closed)),
                    ),
                    ("IsClosed", bool_value(is_closed)),
                    ("IsPeriodic", bool_value(is_periodic)),
                    (
                        "KnotParametrization",
                        enum_value(spline_knot_parameterization(doc, value)),
                    ),
                    ("KnotTolerance", number(spline_knot_tolerance(value))),
                    ("Knots", array(value.knots.iter().copied().map(number))),
                    ("Normal", vec3(value.normal)),
                    ("StartTangent", vec3(value.begin_tangent)),
                    ("Weights", array(value.weights.iter().copied().map(number))),
                ],
                out,
            )
        }
        EntityType::Hatch(value) => collect_hatch(doc, value, out),
        EntityType::Dimension(value) => collect_dimension(doc, value, out),
        EntityType::Leader(value) => {
            let dxf_source = &value.dxf_source;
            let arrow_enabled = if dxf_source.is_dxf() && !dxf_source.contains(71) {
                false
            } else {
                value.arrow_enabled
            };
            let modern_dwg = doc
                .dwg_source_version
                .is_some_and(|version| version >= crate::types::DxfVersion::AC1024);
            let text_height = if modern_dwg || (dxf_source.is_dxf() && !dxf_source.contains(40)) {
                0.0
            } else {
                value.text_height
            };
            let text_width = if modern_dwg || (dxf_source.is_dxf() && !dxf_source.contains(41)) {
                0.0
            } else {
                value.text_width
            };
            let creation_type = if dxf_source.is_dxf() && !dxf_source.contains(73) {
                0
            } else {
                value.creation_type.to_value() as i64
            };
            let style = optional_handle(
                doc.dim_styles
                    .get(&value.dimension_style)
                    .or_else(|| doc.dim_styles.get("Standard"))
                    .map(|style| style.handle),
            );
            field_common(
                doc,
                &value.common,
                "Leader",
                "LEADER",
                0x2D,
                "AcDbLeader",
                false,
                vec![
                    ("AnnotationOffset", vec3(value.annotation_offset)),
                    ("ArrowHeadEnabled", bool_value(arrow_enabled)),
                    (
                        "AssociatedAnnotation",
                        optional_handle(if value.annotation_handle.is_null() {
                            None
                        } else {
                            Some(value.annotation_handle)
                        }),
                    ),
                    ("BlockOffset", vec3(value.block_offset)),
                    ("CreationType", enum_value(creation_type)),
                    ("HasHookline", leader_has_hookline(value)),
                    (
                        "HookLineDirection",
                        enum_value(value.hookline_direction.to_value() as i64),
                    ),
                    ("HorizontalDirection", vec3(value.horizontal_direction)),
                    ("Normal", vec3(value.normal)),
                    ("PathType", enum_value(value.path_type.to_value() as i64)),
                    ("Style", style),
                    ("TextHeight", number(text_height)),
                    ("TextWidth", number(text_width)),
                    ("Vertices", points3(value.vertices.iter().copied())),
                ],
                out,
            )
        }
        EntityType::Viewport(value) => collect_viewport(doc, value, out),
        EntityType::Tolerance(value) => field_common(
            doc,
            &value.common,
            "Tolerance",
            "TOLERANCE",
            0x2E,
            "AcDbFcf",
            false,
            vec![
                (
                    "Direction",
                    vec3(
                        if value
                            .dxf_source
                            .as_ref()
                            .is_some_and(|source| !source.has_direction)
                        {
                            Vector3::ZERO
                        } else {
                            value.direction
                        },
                    ),
                ),
                ("InsertionPoint", vec3(value.insertion_point)),
                ("Normal", vec3(value.normal)),
                (
                    "Style",
                    optional_handle(
                        // CadToleranceTemplate resolves the DWG handle first,
                        // then the DXF name. An unresolved reference keeps the
                        // Standard style attached by Tolerance.AssignDocument.
                        value
                            .dimension_style_handle
                            .and_then(|handle| {
                                doc.dim_styles.iter().find(|style| style.handle == handle)
                            })
                            .or_else(|| doc.dim_styles.get(&value.dimension_style_name))
                            .or_else(|| doc.dim_styles.get("Standard"))
                            .map(|style| style.handle),
                    ),
                ),
                (
                    "Text",
                    Value::String(
                        value
                            .dxf_source
                            .as_ref()
                            .and_then(|source| source.raw_text.as_ref())
                            .filter(|raw| {
                                raw.replace("^J", "\n")
                                    .replace("^M", "\r")
                                    .replace("^I", "\t")
                                    .replace("^ ", "^") == value.text
                            })
                            .cloned()
                            .unwrap_or_else(|| value.text.clone()),
                    ),
                ),
            ],
            out,
        ),
        EntityType::Shape(value) => field_common(
            doc,
            &value.common,
            "Shape",
            "SHAPE",
            0x21,
            "AcDbShape",
            false,
            vec![
                ("InsertionPoint", vec3(value.insertion_point)),
                ("Normal", vec3(value.normal)),
                ("ObliqueAngle", number(value.oblique_angle)),
                ("RelativeXScale", number(value.relative_x_scale)),
                ("Rotation", number(value.rotation)),
                ("ShapeStyle", optional_handle(value.style_handle)),
                ("Size", number(value.size)),
                ("Thickness", number(value.thickness)),
            ],
            out,
        ),
        EntityType::Wipeout(value) => collect_wipeout(doc, value, out),
        EntityType::RasterImage(value) => collect_raster_image(doc, value, out),
        EntityType::Mesh(value) => field_common(
            doc,
            &value.common,
            "Mesh",
            "MESH",
            -999,
            "AcDbSubDMesh",
            false,
            vec![
                ("BlendCrease", bool_value(value.blend_crease)),
                ("Edges", array(value.edges.iter().map(mesh_edge))),
                (
                    "Faces",
                    array(
                        value
                            .faces
                            .iter()
                            .map(|face| array(face.vertices.iter().map(|v| integer(*v as i64)))),
                    ),
                ),
                ("OverrideOption", integer(value.override_option as i64)),
                ("SubdivisionLevel", integer(value.subdivision_level as i64)),
                (
                    "TextureCoordinates",
                    points3(doc.mesh_texture_coordinates(value.common.handle)),
                ),
                ("Version", integer(value.version as i64)),
                ("Vertices", points3(value.vertices.iter().copied())),
            ],
            out,
        ),
        EntityType::PolyfaceMesh(value) => collect_polyface(doc, value, legacy, part, out),
        EntityType::PolygonMesh(value) => collect_polygon_mesh(doc, value, legacy, part, out),
        EntityType::Solid3D(value) => collect_modeler_geometry(
            doc,
            &value.common,
            "Solid3D",
            "3DSOLID",
            0x26,
            "AcDb3dSolid",
            &value.acis_data,
            value.point_of_reference,
            &value.wires,
            &value.silhouettes,
            out,
        ),
        EntityType::Region(value) => collect_modeler_geometry(
            doc,
            &value.common,
            "Region",
            "REGION",
            0x25,
            "AcDbModelerGeometry",
            &value.acis_data,
            value.point_of_reference,
            &value.wires,
            &value.silhouettes,
            out,
        ),
        EntityType::Body(value) => collect_modeler_geometry(
            doc,
            &value.common,
            "CadBody",
            "BODY",
            0x27,
            "AcDbModelerGeometry",
            &value.acis_data,
            value.point_of_reference,
            &value.wires,
            &value.silhouettes,
            out,
        ),
        EntityType::Surface(value) => collect_surface(doc, value, out),
        EntityType::Underlay(value) => collect_underlay(doc, value, out),
        EntityType::Seqend(value) => field_common(
            doc,
            &value.common,
            "Seqend",
            "SEQEND",
            6,
            "AcDbEntity",
            false,
            Vec::new(),
            out,
        ),
        EntityType::Ole2Frame(value) => field_common(
            doc,
            &value.common,
            "Ole2Frame",
            "OLE2FRAME",
            0x4A,
            "AcDbOle2Frame",
            false,
            vec![
                (
                    "BinaryData",
                    match &value.source_payload {
                        Some(source) => bytes(&source.0),
                        None => bytes(&value.encoded_payload()),
                    },
                ),
                // The DWG reader reads the mode and source application
                // without assigning either property.
                (
                    "IsPaperSpace",
                    bool_value(value.is_paper_space && doc.dwg_source_version.is_none()),
                ),
                ("LowerRightCorner", vec3(value.lower_right_corner)),
                ("OleObjectType", enum_value(value.ole_object_type as i64)),
                (
                    "SourceApplication",
                    if doc.dwg_source_version.is_some() {
                        Value::Null
                    } else {
                        Value::String(value.source_application.clone())
                    },
                ),
                ("UpperLeftCorner", vec3(value.upper_left_corner)),
                ("Version", integer(value.version as i64)),
            ],
            out,
        ),
        EntityType::Light(value) => field_common(
            doc,
            &value.common,
            "Light",
            "LIGHT",
            -999,
            "AcDbLight",
            false,
            vec![
                ("Name", Value::String(value.name.clone())),
                ("ClassVersion", integer(value.class_version as i64)),
                ("LightType", integer(value.light_type as i64)),
                ("Position", vec3(value.position)),
                ("Target", vec3(value.target)),
                ("Status", bool_value(value.status)),
                ("LightColor", color(value.light_color)),
                ("Intensity", number(value.intensity)),
            ],
            out,
        ),
        EntityType::SectionSymbol(value) => field_common(
            doc,
            &value.common,
            "SectionSymbol",
            "SECTION",
            -999,
            "AcDbSection",
            false,
            vec![
                (
                    "Style",
                    optional_handle(if value.style_handle.is_null() {
                        None
                    } else {
                        Some(value.style_handle)
                    }),
                ),
                ("SymbolScale", number(value.symbol_scale)),
                (
                    "ViewRep",
                    optional_handle(if value.view_rep_handle.is_null() {
                        None
                    } else {
                        Some(value.view_rep_handle)
                    }),
                ),
                (
                    "Points",
                    array(value.points.iter().map(|point| {
                        inline(
                            "SectionSymbolPoint",
                            vec![
                                ("Point", vec3(point.point)),
                                ("Bulge", number(point.bulge)),
                                ("Label", Value::String(point.label.clone())),
                                ("LabelOffset", vec3(point.label_offset)),
                                ("RawFlag280", integer(point.raw_flag_280 as i64)),
                            ],
                        )
                    })),
                ),
                ("Label", Value::String(value.label.clone())),
            ],
            out,
        ),
        EntityType::ViewBorder(value) => field_common(
            doc,
            &value.common,
            "ViewBorder",
            "VIEWBORDER",
            -999,
            "AcDbViewBorder",
            false,
            vec![
                ("Version", integer(value.version as i64)),
                ("Min", array(value.min.into_iter().map(number))),
                ("Max", array(value.max.into_iter().map(number))),
                ("Center", array(value.center.into_iter().map(number))),
                ("Scale", number(value.scale)),
                ("RotationAngle", number(value.rotation_angle)),
                ("ActiveViewport", handle(value.active_viewport)),
                ("ScaleHandle", handle(value.scale_handle)),
            ],
            out,
        ),
        EntityType::Extended(value) => collect_extended(doc, value, out),
        EntityType::Unknown(value) => {
            if let Some(wall) = super::aec::wall(doc, value) {
                out.push(wall);
            } else {
                field_common(
                    doc,
                    &value.common,
                    super::type_names::canonical(&value.dxf_name).unwrap_or("UnknownEntity"),
                    &value.dxf_name,
                    value.dwg_type_code as i64,
                    "AcDbEntity",
                    false,
                    vec![("DxfClass", dxf_class(&value.dxf_name))],
                    out,
                );
            }
        }
        EntityType::Block(value) => blocks::block(doc, value, out),
        EntityType::BlockEnd(value) => field_common(
            doc,
            &value.common,
            "BlockEnd",
            "ENDBLK",
            5,
            "AcDbBlockEnd",
            false,
            Vec::new(),
            out,
        ),
        EntityType::MultiLeader(value) => collect_multileader(doc, value, out),
        EntityType::MLine(value) => collect_mline(doc, value, out),
        EntityType::Table(value) => table::collect(doc, value, out),
        EntityType::Helix(value) => field_common(
            doc,
            &value.common,
            "Spline",
            "SPLINE",
            0x24,
            "AcDbSpline",
            false,
            Vec::new(),
            out,
        ),
    }
    // ACadSharp's Polyline.onAddVertices calls Entity.MatchProperties for
    // vertices and face records. Identity, XData and reactors remain the child's.
    if matches!(
        entity,
        EntityType::Polyline2D(_) | EntityType::Polyline3D(_) | EntityType::PolyfaceMesh(_) | EntityType::PolygonMesh(_)
    ) {
        let appearance: Vec<_> = common(doc, entity.common(), "", 0, "", false)
            .into_iter()
            .filter(|(key, _)| {
                matches!(
                    *key,
                    "Layer"
                        | "LineType"
                        | "Material"
                        | "Color"
                        | "LineWeight"
                        | "LineTypeScale"
                        | "IsInvisible"
                        | "Transparency"
                )
            })
            .collect();
        for child in &mut out[first..] {
            if child["type"]
                .as_str()
                .is_some_and(|name| name.starts_with("Vertex") || name == "PolygonMeshVertex")
                && child["props"]["Color"].as_str() != Some(UNMAPPED)
            {
                for (key, value) in &appearance {
                    child["props"][*key] = value.clone();
                }
            }
        }
    }
}

fn text_horizontal(value: crate::entities::TextHorizontalAlignment) -> i64 {
    match value {
        crate::entities::TextHorizontalAlignment::Left => 0,
        crate::entities::TextHorizontalAlignment::Center => 1,
        crate::entities::TextHorizontalAlignment::Right => 2,
        crate::entities::TextHorizontalAlignment::Aligned => 3,
        crate::entities::TextHorizontalAlignment::Middle => 4,
        crate::entities::TextHorizontalAlignment::Fit => 5,
    }
}

fn text_vertical(value: crate::entities::TextVerticalAlignment) -> i64 {
    match value {
        crate::entities::TextVerticalAlignment::Baseline => 0,
        crate::entities::TextVerticalAlignment::Bottom => 1,
        crate::entities::TextVerticalAlignment::Middle => 2,
        crate::entities::TextVerticalAlignment::Top => 3,
    }
}

fn text_style(doc: &CadDocument, name: &str) -> Value {
    doc.text_styles
        .get(name)
        .or_else(|| doc.text_styles.get("Standard"))
        .map(|style| handle(style.handle))
        .unwrap_or_else(|| handle(Handle::NULL))
}

fn text_source_tag<'a>(
    tag: &'a str,
    source: Option<&'a crate::entities::text::TextSource>,
) -> &'a str {
    source
        .and_then(|s| s.tag.as_ref())
        .filter(|(_, recovered)| recovered == tag)
        .map(|(raw, _)| raw.as_str())
        .unwrap_or(tag)
}

/// Pre-R2007 DWG strings are decoded through the drawing's code page. A byte sequence the code page cannot
/// decode becomes U+FFFD natively (the WHATWG decoders restore an ASCII byte that follows an invalid lead
/// byte), but .NET's DBCS decoders, which ACadSharp uses, substitute one '?' for the lead byte and that
/// following ASCII byte together.
fn dwg_legacy_replacement(doc: &CadDocument, value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(crate::svg::dwg_legacy_replacement(doc, text)),
        other => other,
    }
}

fn text_source_value(value: &str, source: Option<&crate::entities::text::TextSource>) -> Value {
    Value::String(crate::svg::text_source_value(value, source))
}

fn attribute_definition_props(
    doc: &CadDocument,
    value: &crate::entities::AttributeDefinition,
) -> Vec<(&'static str, Value)> {
    vec![
        ("AlignmentPoint", vec3(value.alignment_point)),
        (
            "AttributeType",
            enum_value(
                value
                    .source
                    .as_deref()
                    .and_then(|s| s.attribute_type)
                    .unwrap_or_else(|| {
                        // A drawing before R2018 states the embedded MTEXT of an attribute only in AutoCAD's round-trip
                        // record, which ACadSharp reads as an object: the view shows a single-line attribute.
                        if value.mtext_flag.to_value() == 0 || crate::io::dwg::autocad_records::mlatt_masked(doc) {
                            1
                        } else {
                            value.mtext_flag.to_value()
                        }
                    }) as i64,
            ),
        ),
        ("Flags", enum_value(value.flags.to_bits() as i64)),
        (
            "Height",
            number(
                if value
                    .source
                    .as_deref()
                    .is_some_and(|s| s.dxf && !s.has_height)
                {
                    1.0
                } else {
                    value.height
                },
            ),
        ),
        (
            "HorizontalAlignment",
            enum_value(value.horizontal_alignment.to_value() as i64),
        ),
        ("InsertPoint", vec3(value.insertion_point)),
        (
            "IsLocked",
            bool_value(value.lock_position && !value.source.as_deref().is_some_and(|s| s.dxf) && !legacy_lock(doc)),
        ),
        (
            "MText",
            value
                .embedded_mtext
                .as_ref()
                .filter(|_| !crate::io::dwg::autocad_records::mlatt_masked(doc))
                .map(|m| handle(m.common.handle))
                .unwrap_or(Value::Null),
        ),
        ("Normal", vec3(value.normal)),
        ("Mirror", enum_value(value.text_generation_flags as i64)),
        ("ObliqueAngle", number(value.oblique_angle)),
        ("Prompt", Value::String(value.prompt.clone())),
        ("Rotation", number(value.rotation)),
        ("Style", text_style(doc, &value.text_style)),
        (
            "Tag",
            Value::String(text_source_tag(&value.tag, value.source.as_deref()).into()),
        ),
        ("Thickness", number(value.thickness)),
        (
            "Value",
            text_source_value(&value.default_value, value.source.as_deref()),
        ),
        (
            "Version",
            integer(
                value
                    .source
                    .as_deref()
                    .and_then(|s| s.last_280)
                    .unwrap_or(value.version) as i64,
            ),
        ),
        (
            "VerticalAlignment",
            enum_value(value.vertical_alignment.to_value() as i64),
        ),
        ("WidthFactor", number(value.width_factor)),
    ]
}

fn attribute_entity_props(
    doc: &CadDocument,
    value: &crate::entities::AttributeEntity,
) -> Vec<(&'static str, Value)> {
    vec![
        ("AlignmentPoint", vec3(value.alignment_point)),
        (
            "AttributeType",
            enum_value(
                value
                    .source
                    .as_deref()
                    .and_then(|s| s.attribute_type)
                    .unwrap_or_else(|| {
                        // A drawing before R2018 states the embedded MTEXT of an attribute only in AutoCAD's round-trip
                        // record, which ACadSharp reads as an object: the view shows a single-line attribute.
                        if value.mtext_flag.to_value() == 0 || crate::io::dwg::autocad_records::mlatt_masked(doc) {
                            1
                        } else {
                            value.mtext_flag.to_value()
                        }
                    }) as i64,
            ),
        ),
        ("Flags", enum_value(value.flags.to_bits() as i64)),
        (
            "Height",
            number(
                if value
                    .source
                    .as_deref()
                    .is_some_and(|s| s.dxf && !s.has_height)
                {
                    1.0
                } else {
                    value.height
                },
            ),
        ),
        (
            "HorizontalAlignment",
            enum_value(value.horizontal_alignment.to_value() as i64),
        ),
        ("InsertPoint", vec3(value.insertion_point)),
        (
            "IsLocked",
            bool_value(value.lock_position && !value.source.as_deref().is_some_and(|s| s.dxf) && !legacy_lock(doc)),
        ),
        (
            "MText",
            value
                .embedded_mtext
                .as_ref()
                .filter(|_| !crate::io::dwg::autocad_records::mlatt_masked(doc))
                .map(|m| handle(m.common.handle))
                .unwrap_or(Value::Null),
        ),
        ("Normal", vec3(value.normal)),
        ("Mirror", enum_value(value.text_generation_flags as i64)),
        ("ObliqueAngle", number(value.oblique_angle)),
        ("Rotation", number(value.rotation)),
        ("Style", text_style(doc, &value.text_style)),
        (
            "Tag",
            Value::String(text_source_tag(&value.tag, value.source.as_deref()).into()),
        ),
        ("Thickness", number(value.thickness)),
        (
            "Value",
            text_source_value(&value.value, value.source.as_deref()),
        ),
        (
            "Version",
            integer(
                value
                    .source
                    .as_deref()
                    .and_then(|s| s.last_280)
                    .unwrap_or(value.version) as i64,
            ),
        ),
        (
            "VerticalAlignment",
            enum_value(value.vertical_alignment.to_value() as i64),
        ),
        ("WidthFactor", number(value.width_factor)),
    ]
}

fn collect_attribute_inline(
    doc: &CadDocument,
    value: &crate::entities::AttributeEntity,
    owner: Handle,
    out: &mut Vec<Value>,
) {
    let mut common = value.common.clone();
    common.owner_handle = owner;
    out.push(entity_record(
        doc,
        &common,
        "AttributeEntity",
        "ATTRIB",
        2,
        "AcDbAttribute",
        false,
        attribute_entity_props(doc, value),
    ));
}

fn collect_dimension(doc: &CadDocument, value: &Dimension, out: &mut Vec<Value>) {
    let result = match value {
        Dimension::Aligned(value) => dimension_common(
            doc,
            &value.base,
            "DimensionAligned",
            0x16,
            "AcDbAlignedDimension",
            vec![
                ("FirstPoint", vec3(value.first_point)),
                ("SecondPoint", vec3(value.second_point)),
                ("DefinitionPoint", vec3(value.definition_point)),
                ("ExtLineRotation", number(value.ext_line_rotation)),
                ("Measurement", number(value.measurement())),
                ("Offset", number(value.offset())),
            ],
        ),
        Dimension::Linear(value) => dimension_common(
            doc,
            &value.base,
            "DimensionLinear",
            0x15,
            "AcDbRotatedDimension",
            vec![
                ("FirstPoint", vec3(value.first_point)),
                ("SecondPoint", vec3(value.second_point)),
                ("DefinitionPoint", vec3(value.definition_point)),
                ("ExtLineRotation", number(value.ext_line_rotation)),
                ("Rotation", number(value.rotation)),
                ("Measurement", number(value.measurement())),
                (
                    "Offset",
                    number(value.second_point.distance(&value.definition_point)),
                ),
            ],
        ),
        Dimension::Radius(value) => dimension_common(
            doc,
            &value.base,
            "DimensionRadius",
            0x19,
            "AcDbRadialDimension",
            vec![
                // ACadSharp reports the DXF group-10 centre as
                // DefinitionPoint and group-15 arc point as AngleVertex.
                // The native model names these two geometric roles in the
                // opposite direction, so project the source-compatible names
                // here without changing the round-trip representation.
                ("DefinitionPoint", vec3(value.angle_vertex)),
                ("AngleVertex", vec3(value.definition_point)),
                ("LeaderLength", number(value.leader_length)),
                ("Measurement", number(value.measurement())),
            ],
        ),
        Dimension::Diameter(value) => dimension_common(
            doc,
            &value.base,
            "DimensionDiameter",
            0x1A,
            "AcDbDiametricDimension",
            vec![
                // ACadSharp checks the Angular bit as a flag; the diameter
                // type value (3) therefore reports IsAngular=true.
                ("IsAngular", bool_value(true)),
                ("DefinitionPoint", vec3(value.definition_point)),
                ("AngleVertex", vec3(value.angle_vertex)),
                ("Center", vec3(value.center())),
                ("LeaderLength", number(value.leader_length)),
                ("Measurement", number(value.measurement())),
            ],
        ),
        Dimension::Angular2Ln(value) => dimension_common(
            doc,
            &value.base,
            "DimensionAngular2Line",
            0x18,
            "AcDb2LineAngularDimension",
            vec![
                ("DimensionArc", vec3(value.dimension_arc)),
                ("FirstPoint", vec3(value.first_point)),
                ("SecondPoint", vec3(value.second_point)),
                ("AngleVertex", vec3(value.angle_vertex)),
                ("DefinitionPoint", vec3(value.definition_point)),
                (
                    "Measurement",
                    super::dimension_geometry::angular2_measurement(value),
                ),
                (
                    "Offset",
                    number(value.second_point.distance(&value.definition_point)),
                ),
                ("Center", super::dimension_geometry::angular2_center(value)),
            ],
        ),
        Dimension::Angular3Pt(value) => dimension_common(
            doc,
            &value.base,
            "DimensionAngular3Pt",
            0x17,
            "AcDb3PointAngularDimension",
            vec![
                ("AngleVertex", vec3(value.angle_vertex)),
                ("FirstPoint", vec3(value.first_point)),
                ("SecondPoint", vec3(value.second_point)),
                ("DefinitionPoint", vec3(value.definition_point)),
                ("Measurement", number(value.measurement_radians())),
            ],
        ),
        Dimension::Ordinate(value) => dimension_common(
            doc,
            &value.base,
            "DimensionOrdinate",
            0x14,
            "AcDbOrdinateDimension",
            vec![
                // Ordinate (6) also contains the Angular flag bit in
                // ACadSharp's DimensionType enum.
                ("IsAngular", bool_value(true)),
                (
                    "Flags",
                    enum_value(dimension_flags(
                        &value.base,
                        value.base.dimension_type,
                        value.is_ordinate_type_x,
                    )),
                ),
                ("FeatureLocation", vec3(value.feature_location)),
                ("LeaderEndpoint", vec3(value.leader_endpoint)),
                ("DefinitionPoint", vec3(value.definition_point)),
                ("IsOrdinateTypeX", bool_value(value.is_ordinate_type_x)),
                ("Measurement", number(value.measurement())),
            ],
        ),
        Dimension::Arc(value) => {
            // ACadSharp constructs DimensionArc with the Angular3Point flag
            // even though the native model uses ArcLength for this subtype.
            dimension_common(
                doc,
                &value.base,
                "DimensionArc",
                -999,
                "AcDbArcDimension",
                vec![
                    ("ObjectName", Value::String("ARC_DIMENSION".into())),
                    (
                        "Flags",
                        enum_value(dimension_flags(
                            &value.base,
                            DimensionType::Angular3Point,
                            false,
                        )),
                    ),
                    ("IsAngular", bool_value(true)),
                    ("Center", vec3(value.center_point)),
                    ("DefinitionPoint", vec3(value.definition_point)),
                    ("FirstPoint", vec3(value.first_extension_point)),
                    ("SecondPoint", vec3(value.second_extension_point)),
                    ("HasLeader", bool_value(value.has_leader)),
                    ("IsPartial", bool_value(value.is_partial)),
                    ("LeaderPoint1", vec3(value.first_leader_point)),
                    ("LeaderPoint2", vec3(value.second_leader_point)),
                    ("StartAngle", number(value.arc_start_parameter)),
                    ("EndAngle", number(value.arc_end_parameter)),
                    (
                        "Measurement",
                        super::dimension_geometry::arc_measurement(value),
                    ),
                ],
            )
        }
        Dimension::LargeRadial(value) => dimension_common(
            doc,
            &value.base,
            "DimensionRadius",
            -999,
            "AcDbRadialDimensionLarge",
            vec![
                ("DefinitionPoint", vec3(value.definition_point)),
                ("FirstPoint", vec3(value.chord_point)),
                ("LeaderPoint1", vec3(value.jog_point)),
                ("TextRotation", number(value.jog_angle)),
                ("Measurement", number(value.measurement())),
            ],
        ),
    };
    out.push(result);
}

fn collect_hatch(doc: &CadDocument, value: &crate::entities::Hatch, out: &mut Vec<Value>) {
    let source = value.dxf_source.as_deref();
    let dwg = doc.dwg_source_version;
    let paths = value.paths.iter().map(|p| hatch_path(doc, p));
    let seeds = source
        .and_then(|s| s.elevation_point)
        .into_iter()
        .chain(value.seed_points.iter().copied());
    // The DWG solid layout has no scale slot; DXF may explicitly store one.
    let scale = if source.is_some_and(|s| !s.has_pattern_scale) || (dwg.is_some() && value.is_solid)
    {
        0.0
    } else {
        value.pattern_scale
    };
    let gradient = value
        .gradient_recovery_source
        .as_deref()
        .unwrap_or(&value.gradient_color);
    let gradient_name = if source.is_some_and(|s| !s.has_gradient_name)
        || dwg.is_some_and(|v| v < DxfVersion::AC1018)
        || (source.is_none() && dwg.is_none() && gradient.name.is_empty())
    {
        Value::Null
    } else {
        Value::String(gradient.name.clone())
    };
    let gradient_colors = gradient.colors.iter().enumerate().map(|(i, entry)| {
        // ACadSharp's DXF callback ignores 421, retaining only the last ACI.
        let c = source
            .and_then(|s| s.gradient_indices.get(i))
            .map(|&index| crate::types::Color::from_index(index))
            .unwrap_or(entry.color);
        let mut projected = color(c);
        if let crate::types::Color::Rgb { r, g, b } = c {
            projected["TrueColor"] =
                json!(i32::from(r) | (i32::from(g) << 8) | (i32::from(b) << 16));
        }
        inline(
            "GradientColor",
            vec![("Color", projected), ("Value", number(entry.value))],
        )
    });
    field_common(
        doc,
        &value.common,
        "Hatch",
        "HATCH",
        0x4E,
        "AcDbHatch",
        false,
        vec![
            ("Elevation", number(value.elevation)),
            ("IsAssociative", bool_value(value.is_associative)),
            ("IsDouble", bool_value(value.is_double)),
            (
                "IsSolid",
                bool_value(value.is_solid && !source.is_some_and(|s| !s.has_solid)),
            ),
            ("Normal", vec3(value.normal)),
            ("Paths", array(paths)),
            (
                "Pattern",
                inline(
                    "HatchPattern",
                    vec![
                        ("Name", Value::String(value.pattern.name.clone())),
                        (
                            "Description",
                            if value.pattern.description.is_empty() {
                                Value::Null
                            } else {
                                Value::String(value.pattern.description.clone())
                            },
                        ),
                        (
                            "Lines",
                            array(value.pattern.lines.iter().map(|line| {
                                inline(
                                    "HatchPattern.Line",
                                    vec![
                                        ("Angle", number(line.angle)),
                                        ("BasePoint", vec2(line.base_point)),
                                        ("Offset", vec2(line.offset)),
                                        (
                                            "DashLengths",
                                            array(line.dash_lengths.iter().copied().map(number)),
                                        ),
                                    ],
                                )
                            })),
                        ),
                    ],
                ),
            ),
            (
                "GradientColor",
                inline(
                    "HatchGradientPattern",
                    vec![
                        ("Angle", number(gradient.angle)),
                        ("ColorTint", number(gradient.color_tint)),
                        ("Colors", array(gradient_colors)),
                        ("Enabled", bool_value(gradient.enabled)),
                        (
                            "IsSingleColorGradient",
                            bool_value(gradient.is_single_color),
                        ),
                        ("Name", gradient_name),
                        ("Shift", number(gradient.shift)),
                    ],
                ),
            ),
            ("PatternAngle", number(value.pattern_angle)),
            ("PatternScale", number(scale)),
            (
                "PatternType",
                enum_value(if source.is_some_and(|s| !s.has_pattern_type) {
                    0
                } else {
                    value.pattern_type as i64
                }),
            ),
            // readPattern consumes the first non-pattern code on exit. Keep the
            // native pixel size even when the reference loses this trailer slot.
            (
                "PixelSize",
                number(
                    if source.is_some_and(|s| s.pattern_trailer_code == Some(47)) {
                        0.0
                    } else {
                        value.pixel_size
                    },
                ),
            ),
            ("SeedPoints", points2(seeds)),
            (
                "Style",
                enum_value(if source.is_some() {
                    0
                } else {
                    value.style as i64
                }),
            ),
        ],
        out,
    );
}

fn hatch_path(doc: &CadDocument, path: &crate::entities::BoundaryPath) -> Value {
    let mut seen = std::collections::HashSet::new();
    // CadBoundaryPathTemplate uses an insertion-ordered HashSet and only binds
    // entity references. Native duplicate/dangling handles remain untouched.
    let entities = path
        .boundary_handles
        .iter()
        .copied()
        .filter(|h| seen.insert(*h) && doc.get_entity(*h).is_some());
    let polyline = path
        .edges
        .iter()
        .any(|e| matches!(e, crate::entities::BoundaryEdge::Polyline(_)));
    let flags = (path.flags.bits() & !2) | if polyline { 2 } else { 0 };
    inline(
        "Hatch.BoundaryPath",
        vec![
            (
                "Edges",
                array(
                    path.edges
                        .iter()
                        .map(|edge| hatch_edge(edge, doc.dwg_source_version.is_some())),
                ),
            ),
            ("Entities", handles(entities)),
            ("Flags", enum_value(flags as i64)),
        ],
    )
}

fn hatch_edge(edge: &crate::entities::BoundaryEdge, dwg: bool) -> Value {
    match edge {
        crate::entities::BoundaryEdge::Line(value) => inline(
            "Hatch.BoundaryPath.Line",
            vec![("Start", vec2(value.start)), ("End", vec2(value.end))],
        ),
        crate::entities::BoundaryEdge::CircularArc(value) => inline(
            "Hatch.BoundaryPath.Arc",
            vec![
                ("Center", vec2(value.center)),
                ("CounterClockWise", bool_value(value.counter_clockwise)),
                ("EndAngle", number(value.end_angle)),
                ("Radius", number(value.radius)),
                ("StartAngle", number(value.start_angle)),
            ],
        ),
        crate::entities::BoundaryEdge::EllipticArc(value) => inline(
            "Hatch.BoundaryPath.Ellipse",
            vec![
                ("Center", vec2(value.center)),
                ("CounterClockWise", bool_value(value.counter_clockwise)),
                ("EndAngle", number(value.end_angle)),
                ("MajorAxisEndPoint", vec2(value.major_axis_endpoint)),
                ("RadiusRatio", number(value.minor_axis_ratio)),
                ("StartAngle", number(value.start_angle)),
            ],
        ),
        crate::entities::BoundaryEdge::Polyline(value) => inline(
            "Hatch.BoundaryPath.Polyline",
            vec![
                ("IsClosed", bool_value(value.is_closed)),
                ("Vertices", points3(value.vertices.iter().copied())),
            ],
        ),
        crate::entities::BoundaryEdge::Spline(value) => inline(
            "Hatch.BoundaryPath.Spline",
            vec![
                (
                    "ControlPoints",
                    points3(value.control_points.iter().copied().map(|mut point| {
                        // The DWG reference reader uses zero for an absent
                        // non-rational weight; native splines correctly use one.
                        if dwg && !value.rational {
                            point.z = 0.0;
                        }
                        point
                    })),
                ),
                ("Degree", integer(value.degree as i64)),
                ("EndTangent", vec2(value.end_tangent)),
                ("FitPoints", points2(value.fit_points.iter().copied())),
                ("IsPeriodic", bool_value(value.periodic)),
                ("IsRational", bool_value(value.rational)),
                ("Knots", array(value.knots.iter().copied().map(number))),
                ("StartTangent", vec2(value.start_tangent)),
            ],
        ),
    }
}

fn collect_viewport(doc: &CadDocument, value: &crate::entities::Viewport, out: &mut Vec<Value>) {
    use crate::objects::ObjectType;
    // A drawing before R2007 states the display settings (before R2004: the shade plot mode) only in AutoCAD's round-trip
    // record, which ACadSharp reads as an object: the native model holds them, the view shows what ACadSharp reads.
    let unrestored = crate::io::dwg::autocad_records::unrestored_viewport(doc, value);
    let value = unrestored.as_ref().unwrap_or(value);
    let block = doc
        .block_records
        .iter()
        .find(|b| b.handle == value.common.owner_handle);
    let id = block
        .and_then(|b| {
            super::tables::view_entity_handles(doc, b)
                .iter()
                .filter(|h| matches!(doc.get_entity(**h), Some(EntityType::Viewport(_))))
                .position(|h| *h == value.common.handle)
        })
        .map_or(0, |index| (index + 1) as i16);
    let represents_paper = id == 1
        && block.is_some_and(|b| {
            matches!(doc.objects.get(&b.layout), Some(ObjectType::Layout(layout))
            if !layout.name.eq_ignore_ascii_case("Model"))
        })
        && !(value.source.as_ref().is_some_and(|s| s.dxf_codes.is_some())
            && doc.version <= DxfVersion::AC1009);
    let mut seen = std::collections::HashSet::new();
    let frozen =
        value.frozen_layers.iter().copied().filter(|h| {
            !h.is_null() && seen.insert(*h) && doc.layers.iter().any(|l| l.handle == *h)
        });
    let status = value
        .source
        .as_ref()
        .and_then(|s| s.status_flags)
        .unwrap_or_else(|| value.status.to_bits() | (i32::from(value.grid_flags.to_bits()) << 18));
    let mut fields = vec![
        (
            "Boundary",
            optional_handle(
                (!value.clip_boundary_handle.is_null()
                    && doc.get_entity(value.clip_boundary_handle).is_some())
                .then_some(value.clip_boundary_handle),
            ),
        ),
        ("Brightness", number(value.brightness)),
        ("DefaultLightingType", integer(value.default_lighting_type)),
        ("FrozenLayers", handles(frozen)),
        ("RepresentsPaper", bool_value(represents_paper)),
        ("Scale", optional_handle(viewport_scale(doc, value))),
        (
            "ScaleFactor",
            number(1.0 / (value.view_height / value.height)),
        ),
        ("ShadePlotMode", integer(value.shade_plot_mode)),
        // Both C# reader templates discard this reference.
        ("VisualStyle", Value::Null),
        ("ActiveStatus", integer(1)),
        ("AmbientLightColor", color(value.ambient_color)),
        ("BackClipPlane", number(value.back_clip_z)),
        ("Center", vec3(value.center)),
        ("CircleZoomPercent", integer(value.circle_sides as i64)),
        ("Contrast", number(value.contrast)),
        ("DisplayUcsIcon", bool_value(value.ucs_at_origin)),
        ("Elevation", number(value.elevation)),
        ("FrontClipPlane", number(value.front_clip_z)),
        (
            "GridSpacing",
            vec2(Vector2::new(value.grid_spacing.x, value.grid_spacing.y)),
        ),
        ("Height", number(value.height)),
        ("Id", integer(id)),
        ("LensLength", number(value.lens_length)),
        ("MajorGridLineFrequency", integer(value.grid_major as i64)),
        (
            "RenderMode",
            enum_value(value.render_mode.to_value() as i64),
        ),
        ("SnapAngle", number(value.snap_angle)),
        (
            "SnapBase",
            vec2(Vector2::new(value.snap_base.x, value.snap_base.y)),
        ),
        (
            "SnapSpacing",
            vec2(Vector2::new(value.snap_spacing.x, value.snap_spacing.y)),
        ),
        ("Status", integer(status)),
        ("StyleSheetName", Value::String(value.style_sheet.clone())),
        ("TwistAngle", number(value.twist_angle)),
        ("UcsOrigin", vec3(value.ucs_origin)),
        ("UcsOrthographicType", integer(value.ucs_ortho_type as i64)),
        ("UcsPerViewport", bool_value(value.ucs_per_viewport)),
        ("UcsXAxis", vec3(value.ucs_x_axis)),
        ("UcsYAxis", vec3(value.ucs_y_axis)),
        ("UseDefaultLighting", bool_value(value.default_lighting)),
        (
            "ViewCenter",
            vec2(Vector2::new(value.view_center.x, value.view_center.y)),
        ),
        ("ViewDirection", vec3(value.view_direction)),
        ("ViewHeight", number(value.view_height)),
        ("ViewTarget", vec3(value.view_target)),
        (
            "ViewWidth",
            number(value.view_height / value.height * value.width),
        ),
        ("Width", number(value.width)),
    ];

    // C# does not apply legacy MVIEW XData to viewport geometry. Its absent
    // DXF properties and pre-version DWG fields keep constructor defaults.
    let legacy_dwg = doc
        .dwg_source_version
        .is_some_and(|v| v <= DxfVersion::AC1014);
    let absent = |code| {
        value
            .source
            .as_ref()
            .is_some_and(|s| s.dxf_codes.is_some() && !s.contains(code))
    };
    let mut set = |name, replacement| {
        if let Some((_, field)) = fields.iter_mut().find(|(key, _)| *key == name) {
            *field = replacement;
        }
    };
    for (name, code, default) in [
        ("Width", 40, number(0.0)),
        ("Height", 41, number(0.0)),
        ("CircleZoomPercent", 72, integer(0)),
        ("LensLength", 42, number(0.0)),
        ("ViewHeight", 45, number(0.0)),
        ("ViewCenter", 12, vec2(Vector2::ZERO)),
        ("ViewDirection", 16, vec3(Vector3::UNIT_Z)),
        ("ViewTarget", 17, vec3(Vector3::ZERO)),
        ("TwistAngle", 51, number(0.0)),
        ("SnapAngle", 50, number(0.0)),
        ("SnapBase", 13, vec2(Vector2::ZERO)),
        ("FrontClipPlane", 43, number(0.0)),
        ("BackClipPlane", 44, number(0.0)),
        ("GridSpacing", 15, vec2(Vector2::ZERO)),
        ("SnapSpacing", 14, vec2(Vector2::ZERO)),
        ("Status", 90, integer(0)),
        ("StyleSheetName", 1, Value::Null),
        ("DisplayUcsIcon", 74, bool_value(false)),
        ("UcsPerViewport", 71, bool_value(false)),
        ("UcsOrigin", 110, vec3(Vector3::ZERO)),
        ("UcsXAxis", 111, vec3(Vector3::UNIT_X)),
        ("UcsYAxis", 112, vec3(Vector3::UNIT_Y)),
        ("Elevation", 146, number(0.0)),
        ("UcsOrthographicType", 79, integer(0)),
        ("MajorGridLineFrequency", 61, integer(0)),
        ("UseDefaultLighting", 292, bool_value(false)),
        ("DefaultLightingType", 282, integer(0)),
    ] {
        let legacy_field = legacy_dwg && !matches!(code, 40 | 41);
        let before_2007 = matches!(code, 61 | 292 | 282)
            && doc
                .dwg_source_version
                .is_some_and(|v| v < DxfVersion::AC1021);
        if absent(code) || legacy_field || before_2007 {
            set(name, default);
        }
    }
    if legacy_dwg || absent(331) {
        set("FrozenLayers", json!([]));
    }
    if value.source.is_none() && value.style_sheet.is_empty() {
        set("StyleSheetName", Value::Null);
    }
    // Vector components have independent DXF codes. An omitted X must not
    // erase a supplied Y (or inherit the native spacing default of ten).
    if value.source.as_ref().is_some_and(|s| s.dxf_codes.is_some()) {
        for (name, codes, vector, default) in [
            ("ViewCenter", [12, 22, 0], value.view_center, Vector3::ZERO),
            (
                "ViewDirection",
                [16, 26, 36],
                value.view_direction,
                Vector3::UNIT_Z,
            ),
            ("ViewTarget", [17, 27, 37], value.view_target, Vector3::ZERO),
            ("SnapBase", [13, 23, 0], value.snap_base, Vector3::ZERO),
            (
                "SnapSpacing",
                [14, 24, 0],
                value.snap_spacing,
                Vector3::ZERO,
            ),
            (
                "GridSpacing",
                [15, 25, 0],
                value.grid_spacing,
                Vector3::ZERO,
            ),
            (
                "UcsOrigin",
                [110, 120, 130],
                value.ucs_origin,
                Vector3::ZERO,
            ),
            (
                "UcsXAxis",
                [111, 121, 131],
                value.ucs_x_axis,
                Vector3::UNIT_X,
            ),
            (
                "UcsYAxis",
                [112, 122, 132],
                value.ucs_y_axis,
                Vector3::UNIT_Y,
            ),
        ] {
            let x = if absent(codes[0]) {
                default.x
            } else {
                vector.x
            };
            let y = if absent(codes[1]) {
                default.y
            } else {
                vector.y
            };
            let z = if absent(codes[2]) {
                default.z
            } else {
                vector.z
            };
            set(
                name,
                if codes[2] == 0 {
                    vec2(Vector2::new(x, y))
                } else {
                    vec3(Vector3::new(x, y, z))
                },
            );
        }
    }
    let get_number = |name| {
        fields
            .iter()
            .find(|(key, _)| *key == name)
            .and_then(|(_, v)| v.as_f64())
            .unwrap()
    };
    let height = get_number("Height");
    let view_height = get_number("ViewHeight");
    let width = get_number("Width");
    for (name, field) in &mut fields {
        match *name {
            "ScaleFactor" => *field = number(1.0 / (view_height / height)),
            "ViewWidth" => *field = number(view_height / height * width),
            _ => {}
        }
    }
    field_common(
        doc,
        &value.common,
        "Viewport",
        "VIEWPORT",
        0x22,
        "AcDbViewport",
        false,
        fields,
        out,
    );
}

fn viewport_scale(doc: &CadDocument, value: &crate::entities::Viewport) -> Option<Handle> {
    use crate::objects::ObjectType;
    // Read only the viewport's dictionary and its one scale XRecord; no
    // whole-document search on this reference path.
    let dictionary = value
        .common
        .xdictionary_handle
        .or_else(|| doc.xdic_by_handle.get(&value.common.handle).copied())?;
    let ObjectType::Dictionary(dictionary) = doc.objects.get(&dictionary)? else {
        return None;
    };
    let record = dictionary.get("ASDK_XREC_ANNOTATION_SCALE_INFO")?;
    let ObjectType::XRecord(record) = doc.objects.get(&record)? else {
        return None;
    };
    let scale = record
        .entries
        .iter()
        .find(|entry| entry.code == 340)?
        .value
        .as_handle()?;
    matches!(doc.objects.get(&scale), Some(ObjectType::Scale(_))).then_some(scale)
}

fn collect_wipeout(doc: &CadDocument, value: &crate::entities::Wipeout, out: &mut Vec<Value>) {
    field_common(
        doc,
        &value.common,
        "Wipeout",
        "WIPEOUT",
        -999,
        "AcDbWipeout",
        false,
        vec![
            ("Brightness", integer(value.brightness as i64)),
            ("ClassVersion", integer(value.class_version as i64)),
            (
                "ClipBoundaryVertices",
                points2(value.clip_boundary_vertices.iter().copied()),
            ),
            ("ClipMode", enum_value(value.clip_mode as i64)),
            ("ClipType", enum_value(value.clip_type as i64)),
            ("ClippingState", bool_value(value.clipping_enabled)),
            ("Contrast", integer(value.contrast as i64)),
            ("Definition", optional_handle(value.definition_handle)),
            ("Fade", integer(value.fade as i64)),
            ("Flags", enum_value(value.flags.bits() as i64)),
            ("InsertPoint", vec3(value.insertion_point)),
            (
                "ShowImage",
                bool_value(
                    value
                        .flags
                        .contains(crate::entities::WipeoutDisplayFlags::SHOW_IMAGE),
                ),
            ),
            ("Size", vec2(value.size)),
            ("UVector", vec3(value.u_vector)),
            ("VVector", vec3(value.v_vector)),
        ],
        out,
    );
}

fn collect_raster_image(
    doc: &CadDocument,
    value: &crate::entities::RasterImage,
    out: &mut Vec<Value>,
) {
    field_common(
        doc,
        &value.common,
        "RasterImage",
        "IMAGE",
        -999,
        "AcDbRasterImage",
        false,
        vec![
            ("Brightness", integer(value.brightness as i64)),
            ("ClassVersion", integer(value.class_version as i64)),
            (
                "ClipBoundaryVertices",
                points2(value.clip_boundary.vertices.iter().copied()),
            ),
            ("ClipMode", enum_value(value.clip_boundary.clip_mode as i64)),
            ("ClipType", enum_value(value.clip_boundary.clip_type as i64)),
            ("ClippingState", bool_value(value.clipping_enabled)),
            ("Contrast", integer(value.contrast as i64)),
            ("Definition", optional_handle(value.definition_handle)),
            ("Fade", integer(value.fade as i64)),
            ("Flags", enum_value(value.flags.bits() as i64)),
            ("InsertPoint", vec3(value.insertion_point)),
            ("ShowImage", bool_value(value.is_visible())),
            ("Size", vec2(value.size)),
            ("UVector", vec3(value.u_vector)),
            ("VVector", vec3(value.v_vector)),
        ],
        out,
    );
}

fn mesh_edge(value: &crate::entities::MeshEdge) -> Value {
    inline(
        "Mesh.Edge",
        vec![
            (
                "Crease",
                // ACadSharp's DXF and DWG readers assign every serialized
                // crease slot, including the zero sentinel used for an
                // uncreased edge.  Native acadrust keeps that sentinel as
                // `None`; project the reader result here without changing
                // the native model.
                value.crease.map(number).unwrap_or_else(|| number(0.0)),
            ),
            ("End", integer(value.end as i64)),
            ("Start", integer(value.start as i64)),
        ],
    )
}

fn collect_polyface(
    doc: &CadDocument,
    value: &crate::entities::PolyfaceMesh,
    legacy: bool,
    part: RecordPart,
    out: &mut Vec<Value>,
) {
    if let RecordPart::PolyfaceVertex(index) = part {
        let vertex = value
            .vertices
            .get(index)
            .filter(|vertex| !vertex.common.handle.is_null())
            .expect("polyface vertex descriptor without vertex");
        field_common(
            doc,
            &vertex.common,
            if legacy { "Vertex2D" } else { "VertexFaceMesh" },
            "VERTEX",
            if legacy { 0x0A } else { 0x0D },
            if legacy {
                "AcDb2dVertex"
            } else {
                "AcDbPolyFaceMeshVertex"
            },
            false,
            vec![
                ("Bulge", number(vertex.bulge)),
                ("CurveTangent", number(vertex.curve_tangent)),
                ("EndWidth", number(vertex.end_width)),
                ("Flags", enum_value(vertex.flags.bits() as i64)),
                ("Id", integer(vertex.id as i64)),
                ("Location", vec3(vertex.location)),
                ("StartWidth", number(vertex.start_width)),
            ],
            out,
        );
        return;
    }
    if let RecordPart::PolyfaceFace(index) = part {
        let face = value
            .faces
            .get(index)
            .filter(|face| !face.common.handle.is_null())
            .expect("polyface face descriptor without face");
        if legacy {
            out.push(vertex_record(
                doc,
                &face.common,
                "Vertex2D",
                Vector3::ZERO,
                face.flags.bits() as i64,
                0.0,
                0.0,
                0.0,
                0.0,
                0,
            ));
        } else {
            field_common(
                doc,
                &face.common,
                "VertexFaceRecord",
                "VERTEX",
                0x0E,
                "AcDbFaceRecord",
                false,
                vec![
                    ("Bulge", number(0.0)),
                    ("CurveTangent", number(0.0)),
                    ("StartWidth", number(0.0)),
                    ("EndWidth", number(0.0)),
                    ("Id", integer(0)),
                    ("Location", vec3(Vector3::ZERO)),
                    ("Flags", enum_value((face.flags.bits() | 128) as i64)),
                    ("Index1", integer(face.index1 as i64)),
                    ("Index2", integer(face.index2 as i64)),
                    ("Index3", integer(face.index3 as i64)),
                    ("Index4", integer(face.index4 as i64)),
                ],
                out,
            );
        }
        return;
    }
    let vertices_complete = value
        .vertices
        .iter()
        .all(|vertex| !vertex.common.handle.is_null());
    let vertices = value
        .vertices
        .iter()
        .filter_map(|vertex| {
            if vertex.common.handle.is_null() {
                return None;
            }
            Some(handle(vertex.common.handle))
        })
        .collect::<Vec<_>>();
    let faces_complete = value.faces.iter().all(|face| !face.common.handle.is_null());
    let faces = value
        .faces
        .iter()
        .filter_map(|face| {
            if face.common.handle.is_null() {
                return None;
            }
            Some(handle(face.common.handle))
        })
        .collect::<Vec<_>>();
    field_common(
        doc,
        &value.common,
        if legacy { "Polyline2D" } else { "PolyfaceMesh" },
        "POLYLINE",
        if legacy { 0x0F } else { 0x1D },
        if legacy {
            "AcDb2dPolyline"
        } else {
            "AcDbPolyFaceMesh"
        },
        false,
        vec![
            ("MatchVerticesEntityProperties", bool_value(true)),
            ("Elevation", number(value.elevation)),
            ("EndWidth", number(value.end_width)),
            (
                "Faces",
                if faces_complete {
                    array(faces.iter().cloned())
                } else {
                    unmapped()
                },
            ),
            ("Flags", enum_value(value.flags.bits() as i64)),
            (
                "IsClosed",
                bool_value(
                    value
                        .flags
                        .contains(crate::entities::PolyfaceMeshFlags::CLOSED),
                ),
            ),
            ("Normal", vec3(value.normal)),
            ("SmoothSurface", enum_value(value.smooth_surface as i64)),
            ("StartWidth", number(value.start_width)),
            ("Thickness", number(value.thickness)),
            (
                "Vertices",
                if vertices_complete {
                    if legacy {
                        array(vertices.iter().cloned().chain(faces.iter().cloned()))
                    } else {
                        array(vertices.iter().cloned())
                    }
                } else {
                    unmapped()
                },
            ),
        ],
        out,
    );
}

fn collect_polygon_mesh(
    doc: &CadDocument,
    value: &crate::entities::PolygonMeshEntity,
    legacy: bool,
    part: RecordPart,
    out: &mut Vec<Value>,
) {
    if let RecordPart::PolygonMeshVertex(index) = part {
        let vertex = value
            .vertices
            .get(index)
            .filter(|vertex| !vertex.common.handle.is_null())
            .expect("polygon mesh vertex descriptor without vertex");
        if legacy {
            out.push(vertex_record(
                doc,
                &vertex.common,
                "Vertex2D",
                vertex.location,
                vertex.flags as i64,
                0.0,
                0.0,
                0.0,
                0.0,
                0,
            ));
        } else {
            field_common(
                doc,
                &vertex.common,
                "PolygonMeshVertex",
                "VERTEX",
                0x0C,
                "AcDbPolygonMeshVertex",
                false,
                vec![
                    ("Flags", integer(vertex.flags as i64)),
                    ("Location", vec3(vertex.location)),
                ],
                out,
            );
        }
        return;
    }
    let vertices_complete = value
        .vertices
        .iter()
        .all(|vertex| !vertex.common.handle.is_null());
    let vertices = value
        .vertices
        .iter()
        .filter_map(|vertex| {
            if vertex.common.handle.is_null() {
                return None;
            }
            Some(handle(vertex.common.handle))
        })
        .collect::<Vec<_>>();
    field_common(
        doc,
        &value.common,
        if legacy { "Polyline2D" } else { "PolygonMesh" },
        "POLYLINE",
        if legacy { 0x0F } else { 0x1E },
        if legacy { "AcDb2dPolyline" } else { "AcDbPolygonMesh" },
        false,
        vec![
            ("Elevation", number(value.elevation)),
            ("Flags", enum_value(value.flags.bits() as i64)),
            (
                "IsClosed",
                bool_value(value.is_closed_m() || (!legacy && value.is_closed_n())),
            ),
            ("MVertexCount", integer(value.m_vertex_count as i64)),
            ("NVertexCount", integer(value.n_vertex_count as i64)),
            (
                "MSmoothSurfaceDensity",
                integer(value.m_smooth_density as i64),
            ),
            (
                "NSmoothSurfaceDensity",
                integer(value.n_smooth_density as i64),
            ),
            ("Normal", vec3(value.normal)),
            ("SmoothSurface", enum_value(value.smooth_type as i64)),
            ("Thickness", unmapped()),
            (
                "Vertices",
                if vertices_complete {
                    array(vertices.into_iter())
                } else {
                    unmapped()
                },
            ),
        ],
        out,
    );
}

fn collect_modeler_geometry(
    doc: &CadDocument,
    common: &EntityCommon,
    type_name: &'static str,
    object_name: &'static str,
    object_type: i64,
    subclass: &'static str,
    data: &crate::entities::AcisData,
    point: Vector3,
    wires: &[crate::entities::solid3d::Wire],
    silhouettes: &[crate::entities::solid3d::Silhouette],
    out: &mut Vec<Value>,
) {
    field_common(
        doc,
        common,
        type_name,
        object_name,
        object_type,
        subclass,
        false,
        modeler::properties(data, point, wires, silhouettes),
        out,
    );
}

/// ACadSharp reads each surface subtype as its own class; they share
/// Surface's properties and differ only in identity.
fn surface_identity(kind: crate::entities::SurfaceKind) -> (&'static str, &'static str, &'static str) {
    use crate::entities::SurfaceKind;
    match kind {
        SurfaceKind::Generic => ("Surface", "SURFACE", "AcDbSurface"),
        SurfaceKind::Plane => ("PlaneSurface", "PLANESURFACE", "AcDbPlaneSurface"),
        SurfaceKind::Extruded => ("ExtrudedSurface", "EXTRUDEDSURFACE", "AcDbExtrudedSurface"),
        SurfaceKind::Lofted => ("LoftedSurface", "LOFTEDSURFACE", "AcDbLoftedSurface"),
        SurfaceKind::Revolved => ("RevolvedSurface", "REVOLVEDSURFACE", "AcDbRevolvedSurface"),
        SurfaceKind::Swept => ("SweptSurface", "SWEPTSURFACE", "AcDbSweptSurface"),
        SurfaceKind::Nurb => ("NurbSurface", "NURBSURFACE", "AcDbNurbSurface"),
    }
}

fn collect_surface(doc: &CadDocument, value: &crate::entities::Surface, out: &mut Vec<Value>) {
    let (type_name, object_name, subclass) = surface_identity(value.kind);
    let mut properties = modeler::properties(
        &value.acis_data,
        value.point_of_reference,
        &value.wires,
        &value.silhouettes,
    );
    // The DWG surface reader does not decode the AcDbSurface isoline counts.
    let isolines = |count: i16| {
        integer(if doc.dwg_source_version.is_some() { 0 } else { i64::from(count) })
    };
    properties.push(("UIsolines", isolines(value.u_isolines)));
    properties.push(("VIsolines", isolines(value.v_isolines)));
    field_common(
        doc,
        &value.common,
        type_name,
        object_name,
        -999,
        subclass,
        false,
        properties,
        out,
    );
}

fn collect_underlay(doc: &CadDocument, value: &crate::entities::Underlay, out: &mut Vec<Value>) {
    let (type_name, object_name, subclass) = match value.underlay_type {
        // ACadSharp's concrete underlay entities all inherit the same
        // AcDbUnderlayReference marker; the PDF/DWF/DGN distinction is in
        // ObjectName and the typed record.
        crate::entities::UnderlayType::Pdf => ("PdfUnderlay", "PDFUNDERLAY", "AcDbUnderlayReference"),
        crate::entities::UnderlayType::Dwf => ("DwfUnderlay", "DWFUNDERLAY", "AcDbUnderlayReference"),
        crate::entities::UnderlayType::Dgn => ("DgnUnderlay", "DGNUNDERLAY", "AcDbUnderlayReference"),
    };
    field_common(
        doc,
        &value.common,
        type_name,
        object_name,
        -999,
        subclass,
        false,
        vec![
            ("Definition", handle(value.definition_handle)),
            ("InsertPoint", vec3(value.insertion_point)),
            ("Rotation", number(value.rotation)),
            ("Normal", vec3(value.normal)),
            ("XScale", number(value.x_scale)),
            ("YScale", number(value.y_scale)),
            ("ZScale", number(value.z_scale)),
            ("Contrast", integer(value.contrast as i64)),
            ("Fade", integer(value.fade as i64)),
            (
                "ClipBoundaryVertices",
                points2(value.clip_boundary_vertices.iter().copied()),
            ),
            ("ClipInverted", bool_value(value.clip_inverted)),
            ("Flags", integer(value.flags.bits() as i64)),
        ],
        out,
    );
}

fn collect_extended(
    doc: &CadDocument,
    value: &crate::entities::ExtendedEntity,
    out: &mut Vec<Value>,
) {
    let class_name = value.class_name();
    match &value.data {
        crate::entities::ExtendedEntityData::Proxy(data) => {
            field_common(
                doc,
                &value.common,
                "ProxyEntity",
                class_name,
                0x1f2,
                "AcDbProxyEntity",
                false,
                {
                    let mut members = super::proxy::members(
                        doc,
                        data.class_id,
                        data.dwg_version,
                        data.maintenance_version,
                        data.from_dxf,
                        // The DXF reader has no handler for the entity data.
                        false,
                    );
                    members.push(("ProxyClassId", integer(data.proxy_id)));
                    members
                },
                out,
            );
        }
        crate::entities::ExtendedEntityData::OleFrame(data) => field_common(
            doc,
            &value.common,
            "Ole2Frame",
            class_name,
            0x4A,
            "AcDbOle2Frame",
            false,
            vec![
                ("BinaryData", bytes(&data.storage.encode())),
                ("IsPaperSpace", bool_value(data.mode != 0)),
                ("OleObjectType", enum_value(data.flag as i64)),
            ],
            out,
        ),
        _ => field_common(
            doc,
            &value.common,
            super::type_names::canonical(class_name).unwrap_or("UnknownEntity"),
            class_name,
            -999,
            "AcDbEntity",
            false,
            vec![("DxfClass", dxf_class(class_name))],
            out,
        ),
    }
}

fn collect_multileader(
    doc: &CadDocument,
    value: &crate::entities::MultiLeader,
    out: &mut Vec<Value>,
) {
    multileader::collect(doc, value, out)
}

fn collect_mline(doc: &CadDocument, value: &crate::entities::MLine, out: &mut Vec<Value>) {
    // ACadSharp's DWG reader only consumes the open/closed word and maps any
    // value other than 3 to HasVertices.  Rust retains the raw DWG bits,
    // including cap-suppression flags, so apply that reader-specific view at
    // the compatibility boundary.  DXF stores the full enum directly.
    let flags = if doc.dwg_source_version.is_some() {
        // The DWG word is tested as an exact value by ACadSharp, so a
        // nonstandard value carrying cap bits is still reported as open.
        if value.flags.bits()
            == (crate::entities::MLineFlags::HAS_VERTICES | crate::entities::MLineFlags::CLOSED)
                .bits()
        {
            crate::entities::MLineFlags::CLOSED.bits()
        } else {
            crate::entities::MLineFlags::HAS_VERTICES.bits()
        }
    } else {
        value.flags.bits()
    };
    field_common(
        doc,
        &value.common,
        "MLine",
        "MLINE",
        0x2F,
        "AcDbMline",
        false,
        vec![
            ("Flags", enum_value(flags as i64)),
            ("Justification", enum_value(value.justification as i64)),
            ("Normal", vec3(value.normal)),
            ("ScaleFactor", number(value.scale_factor)),
            ("StartPoint", vec3(value.start_point)),
            ("Style", optional_handle(value.style_handle)),
            (
                "Vertices",
                array(value.vertices.iter().map(|vertex| {
                    inline(
                        "MLine.Vertex",
                        vec![
                            ("Direction", vec3(vertex.direction)),
                            ("Miter", vec3(vertex.miter)),
                            ("Position", vec3(vertex.position)),
                            (
                                "Segments",
                                array(vertex.segments.iter().map(|segment| {
                                    inline(
                                        "MLine.Vertex.Segment",
                                        vec![
                                            (
                                                "AreaFillParameters",
                                                array(
                                                    segment
                                                        .area_fill_parameters
                                                        .iter()
                                                        .copied()
                                                        .map(number),
                                                ),
                                            ),
                                            (
                                                "Parameters",
                                                array(
                                                    segment.parameters.iter().copied().map(number),
                                                ),
                                            ),
                                        ],
                                    )
                                })),
                            ),
                        ],
                    )
                })),
            ),
        ],
        out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{
        AttributeEntity, Insert, Line, Polyline2D, Polyline3D, Seqend, Vertex2D, Vertex3DPolyline,
    };
    use crate::types::Color;
    use std::sync::Arc;

    fn handle_value(value: u64) -> Handle {
        Handle::new(value)
    }

    fn synthetic_document(version: DxfVersion) -> CadDocument {
        let mut doc = CadDocument::new();
        doc.version = version;

        let mut line = Line::from_coords(0.0, 0.0, 0.0, 1.0, 1.0, 0.0);
        line.common.handle = handle_value(0x10);
        line.common.owner_handle = handle_value(0x01);
        line.common.xdictionary_handle = Some(handle_value(0x11));
        doc.entities.push(Arc::new(EntityType::Line(line)));

        let mut polyline = Polyline2D::new();
        polyline.common.handle = handle_value(0x20);
        polyline.common.owner_handle = handle_value(0x01);
        polyline.common.color = Color::from_rgb(10, 20, 30);
        polyline.common.xdictionary_handle = Some(handle_value(0x21));
        let mut vertex = Vertex2D::new(Vector3::new(2.0, 3.0, 0.0));
        vertex.common.handle = handle_value(0x22);
        vertex.common.owner_handle = polyline.common.handle;
        vertex.common.xdictionary_handle = Some(handle_value(0x23));
        polyline.vertices.push(vertex);
        let mut polyline_end = Seqend::new();
        polyline_end.common.handle = handle_value(0x24);
        polyline_end.common.owner_handle = polyline.common.handle;
        polyline.seqend = Some(Box::new(polyline_end));
        doc.entities
            .push(Arc::new(EntityType::Polyline2D(polyline)));

        let mut polyline3d = Polyline3D::new();
        polyline3d.common.handle = handle_value(0x30);
        polyline3d.common.owner_handle = handle_value(0x01);
        let mut unowned_vertex = Vertex3DPolyline::new(Vector3::new(4.0, 5.0, 6.0));
        unowned_vertex.common.handle = handle_value(0x31);
        // A source 3D vertex can retain an identity without an owner. The
        // renderer must preserve it while retaining the parent relationship.
        unowned_vertex.common.owner_handle = Handle::NULL;
        polyline3d.vertices.push(unowned_vertex);
        let mut owned_vertex = Vertex3DPolyline::new(Vector3::new(7.0, 8.0, 9.0));
        owned_vertex.common.handle = handle_value(0x32);
        owned_vertex.common.owner_handle = polyline3d.common.handle;
        polyline3d.vertices.push(owned_vertex);
        doc.entities
            .push(Arc::new(EntityType::Polyline3D(polyline3d)));

        let mut insert = Insert::new("MODEL_SPACE", Vector3::ZERO);
        insert.common.handle = handle_value(0x40);
        insert.common.owner_handle = handle_value(0x01);
        let mut attribute = AttributeEntity::new("TAG".to_owned(), "value".to_owned());
        attribute.common.handle = handle_value(0x41);
        attribute.common.owner_handle = Handle::NULL;
        insert.attributes.push(attribute);
        let mut insert_end = Seqend::new();
        insert_end.common.handle = handle_value(0x42);
        insert_end.common.owner_handle = insert.common.handle;
        insert.seqend = Some(Box::new(insert_end));
        doc.entities.push(Arc::new(EntityType::Insert(insert)));

        doc
    }

    fn default_entity_variants() -> Vec<EntityType> {
        vec![
            EntityType::Point(Default::default()),
            EntityType::Line(Default::default()),
            EntityType::Circle(Default::default()),
            EntityType::Arc(Default::default()),
            EntityType::Ellipse(Default::default()),
            EntityType::Polyline(Default::default()),
            EntityType::Polyline2D(Default::default()),
            EntityType::Polyline3D(Default::default()),
            EntityType::LwPolyline(Default::default()),
            EntityType::Text(Default::default()),
            EntityType::MText(Default::default()),
            EntityType::Spline(Default::default()),
            EntityType::Helix(Default::default()),
            EntityType::Hatch(Default::default()),
            EntityType::BlockEnd(Default::default()),
            EntityType::Ray(Default::default()),
            EntityType::XLine(Default::default()),
            EntityType::Viewport(Default::default()),
            EntityType::AttributeDefinition(Default::default()),
            EntityType::AttributeEntity(Default::default()),
            EntityType::Leader(Default::default()),
            EntityType::MultiLeader(Default::default()),
            EntityType::MLine(Default::default()),
            EntityType::Mesh(Default::default()),
            EntityType::RasterImage(Default::default()),
            EntityType::Solid3D(Default::default()),
            EntityType::Region(Default::default()),
            EntityType::Body(Default::default()),
            EntityType::Table(Default::default()),
            EntityType::Tolerance(Default::default()),
            EntityType::PolyfaceMesh(Default::default()),
            EntityType::Wipeout(Default::default()),
            EntityType::Shape(Default::default()),
            EntityType::Underlay(Default::default()),
            EntityType::Seqend(Default::default()),
            EntityType::Ole2Frame(Default::default()),
            EntityType::PolygonMesh(Default::default()),
            EntityType::Light(Default::default()),
            EntityType::SectionSymbol(Default::default()),
            EntityType::ViewBorder(Default::default()),
            EntityType::Dimension(Box::new(Dimension::Aligned(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Linear(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Radius(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Diameter(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Angular2Ln(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Angular3Pt(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Ordinate(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::Arc(Default::default()))),
            EntityType::Dimension(Box::new(Dimension::LargeRadial(Default::default()))),
        ]
    }

    fn document_with_default_variants(version: DxfVersion) -> (CadDocument, usize) {
        let mut doc = CadDocument::new();
        doc.version = version;
        let mut entities = default_entity_variants();
        let count = entities.len();
        for (index, entity) in entities.iter_mut().enumerate() {
            let common = entity.common_mut();
            common.handle = handle_value(0x100 + index as u64);
            common.owner_handle = handle_value(0x01);
            common.xdictionary_handle = Some(handle_value(0x1000 + index as u64));
        }
        doc.entities.extend(entities.into_iter().map(Arc::new));
        (doc, count)
    }

    fn assert_metadata_matches(full: &Value, metadata: &Value) {
        assert_eq!(metadata["handle"], full["handle"]);
        assert_eq!(metadata["type"], full["type"]);
        assert_eq!(metadata["owner"], full["owner"]);

        let full_props = full["props"].as_object().expect("full properties");
        let metadata_props = metadata["props"].as_object().expect("metadata properties");
        assert_eq!(
            metadata_props.get("SubclassMarker"),
            full_props.get("SubclassMarker")
        );
        assert_eq!(
            metadata_props.contains_key("ProxyGeometries"),
            full_props.contains_key("ProxyGeometries")
        );
        if let Some(expected) = full_props.get("XDictionary") {
            match metadata_props.get("XDictionary") {
                Some(actual) => assert_eq!(actual, expected),
                None => assert_eq!(expected.as_str(), Some(UNMAPPED)),
            }
        }
        assert!(metadata_props.keys().all(|key| {
            matches!(
                key.as_str(),
                "Name" | "SubclassMarker" | "ProxyGeometries" | "XDictionary"
            )
        }));
    }

    #[test]
    fn records_render_one_value_and_metadata_is_reduced() {
        for (version, reader) in [
            (DxfVersion::AC1032, "dwg"),
            (DxfVersion::AC1032, "dxf"),
            (DxfVersion::AC1009, "dxf"),
        ] {
            let doc = synthetic_document(version);
            let descriptors = records(&doc, reader);
            assert!(descriptors.len() >= 9);

            let mut collected = Vec::new();
            collect(&doc, reader, &mut collected);
            assert_eq!(collected.len(), descriptors.len());
            for (descriptor, collected_value) in descriptors.iter().zip(&collected) {
                let rendered = descriptor.render(&doc, reader);
                assert_eq!(&rendered, collected_value);
                assert_metadata_matches(&rendered, &descriptor.metadata(&doc, reader));
            }
        }
    }

    #[test]
    fn legacy_polyline_children_keep_identity_and_partial_3d_vertices() {
        let doc = synthetic_document(DxfVersion::AC1009);
        let descriptors = records(&doc, "dxf");
        let values: Vec<_> = descriptors
            .iter()
            .map(|record| record.render(&doc, "dxf"))
            .collect();

        assert!(values.iter().any(|value| {
            value["handle"] == handle(handle_value(0x31)) && value["type"] == "Vertex2D"
        }));
        assert!(values.iter().any(|value| {
            value["handle"] == handle(handle_value(0x41))
                && value["type"] == "AttributeEntity"
        }));
        assert!(values.iter().any(|value| {
            value["handle"] == handle(handle_value(0x42)) && value["type"] == "Seqend"
        }));
    }

    #[test]
    fn metadata_matches_every_default_entity_variant_and_dimension() {
        let (doc, count) = document_with_default_variants(DxfVersion::AC1032);
        let descriptors = records(&doc, "dxf");
        assert_eq!(descriptors.len(), count);
        for descriptor in descriptors {
            let rendered = descriptor.render(&doc, "dxf");
            let metadata = descriptor.metadata(&doc, "dxf");
            assert_metadata_matches(&rendered, &metadata);
            assert_eq!(metadata["props"]["SubclassMarker"], rendered["props"]["SubclassMarker"]);
        }
    }
}

/// The material handle ACadSharp reports. A DXF names the built-in ByBlock material in group 347 where a DWG has the
/// flag "ByBlock" (which ACadSharp's DWG reader does not read); the native model keeps the flag for both.
fn material_handle(doc: &CadDocument, common: &EntityCommon) -> Option<Handle> {
    if common.material_handle.is_some() || common.material_flags != 1 || doc.dwg_source_version.is_some() {
        return common.material_handle;
    }
    doc.objects.iter().find_map(|(handle, object)| match object {
        crate::objects::ObjectType::Material(material) if material.name.eq_ignore_ascii_case("ByBlock") => Some(*handle),
        _ => None,
    })
}
