//! Canonical ACadSharp-facing entity dump mappings.
//!
//! The mapping deliberately uses only state retained by acadrust.  A field
//! whose state is absent or only held in an opaque payload is left as
//! `!unmapped` by `props`.

use crate::entities::{Dimension, EntityCommon, EntityType};
use crate::types::{Handle, Transparency, Vector2, Vector3};
use crate::xdata::XDataValue;
use crate::CadDocument;
use serde_json::{json, Value};

use super::{color, handle, number, optional_handle, props, record, transparency, vec2, vec3};

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

fn normalized_angle(angle: f64) -> f64 {
    if angle < 0.0 {
        angle + std::f64::consts::TAU
    } else {
        angle
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
    vec![
        ("BookColor", optional_handle(common.color_book_handle)),
        ("Color", color(common.color)),
        ("ExtendedData", extended_data(doc, &common.extended_data)),
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
        ("LineWeight", enum_value(common.line_weight.value() as i64)),
        ("Material", optional_handle(common.material_handle)),
        ("ObjectName", Value::String(object_name.to_string())),
        ("ObjectType", enum_value(object_type)),
        // DwgReaderConfiguration.IgnoreProxyGraphics defaults to true; the
        // DXF builder always ignores them. Retain native graphic_data intact.
        ("ProxyGeometries", array([])),
        ("Reactors", handles(common.reactors.iter().copied())),
        ("SubclassMarker", Value::String(subclass.to_string())),
        ("Transparency", transparency(common.transparency)),
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

fn xdata_record(value: &XDataValue) -> Value {
    let (code, raw, closing) = match value {
        XDataValue::String(value) => (1000, Value::String(value.clone()), None),
        XDataValue::ControlString(value) => {
            let is_closing = value == "}";
            (
                1002,
                integer(if is_closing { '}' as i64 } else { '{' as i64 }),
                Some(is_closing),
            )
        }
        XDataValue::LayerName(value) => (1003, Value::String(value.clone()), None),
        XDataValue::BinaryData(value) => (1004, bytes(value), None),
        XDataValue::Handle(value) => (1005, json!(value.value()), None),
        XDataValue::Point3D(value) => (1010, vec3(*value), None),
        XDataValue::Position3D(value) => (1011, vec3(*value), None),
        XDataValue::Displacement3D(value) => (1012, vec3(*value), None),
        XDataValue::Direction3D(value) => (1013, vec3(*value), None),
        XDataValue::Real(value) => (1040, number(*value), None),
        XDataValue::Distance(value) => (1041, number(*value), None),
        XDataValue::ScaleFactor(value) => (1042, number(*value), None),
        XDataValue::Integer16(value) => (1070, integer(*value as i64), None),
        XDataValue::Integer32(value) => (1071, integer(*value as i64), None),
    };
    let mut object = serde_json::Map::new();
    object.insert("Code".to_string(), enum_value(code));
    if let Some(is_closing) = closing {
        object.insert("IsClosing".to_string(), bool_value(is_closing));
    }
    object.insert("RawValue".to_string(), raw);
    Value::Object(object)
}

fn bytes(value: &[u8]) -> Value {
    Value::String(value.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(super) fn extended_data(doc: &CadDocument, data: &crate::xdata::ExtendedData) -> Value {
    let mut pairs = Vec::new();
    for record_data in data.records() {
        let app_handle = doc
            .app_ids
            .get(&record_data.application_name)
            .map(|app| app.handle)
            .unwrap_or(Handle::NULL);
        let records = record_data
            .values
            .iter()
            .map(xdata_record)
            .collect::<Vec<_>>();
        pairs.push(Value::Array(vec![
            handle(app_handle),
            json!({ "Records": records }),
        ]));
    }
    Value::Array(pairs)
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

fn polyline3d_vertices(
    doc: &CadDocument,
    value: &crate::entities::Polyline3D,
    out: &mut Vec<Value>,
) -> Value {
    let complete = value.vertices.iter().all(|vertex| !vertex.handle.is_null());
    let vertices = value
        .vertices
        .iter()
        .filter_map(|vertex| {
            if vertex.handle.is_null() {
                return None;
            }
            let layer = doc.layers.get(&vertex.layer).map(|layer| layer.handle);
            out.push(record(
                vertex.handle,
                "Vertex3D",
                value.common.handle,
                props(
                    "Vertex3D",
                    vec![
                        ("Flags", enum_value(vertex.flags as i64)),
                        ("Layer", optional_handle(layer)),
                        ("Location", vec3(vertex.position)),
                        ("ObjectName", Value::String("VERTEX".to_string())),
                        ("ObjectType", enum_value(0x0B)),
                        ("ProxyGeometries", array([])),
                        (
                            "SubclassMarker",
                            Value::String("AcDb3dPolylineVertex".to_string()),
                        ),
                    ],
                ),
            ));
            Some(handle(vertex.handle))
        })
        .collect::<Vec<_>>();
    if complete {
        array(vertices)
    } else {
        unmapped()
    }
}

fn lw_vertex_inline(vertex: &crate::entities::LwVertex) -> Value {
    inline(
        "LwPolyline.Vertex",
        vec![
            ("Bulge", number(vertex.bulge)),
            ("CurveTangent", unmapped()),
            ("EndWidth", number(vertex.end_width)),
            ("Flags", unmapped()),
            ("Id", integer(vertex.vertex_id as i64)),
            ("Location", vec2(vertex.location)),
            ("StartWidth", number(vertex.start_width)),
        ],
    )
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
        ("AttachmentPoint", enum_value(base.attachment_point as i64)),
        ("Block", block),
        ("DefinitionPoint", vec3(base.definition_point)),
        ("Flags", enum_value(base.dimension_type as i64)),
        ("FlipArrow1", bool_value(base.flip_arrow1)),
        ("FlipArrow2", bool_value(base.flip_arrow2)),
        ("HasStyleOverride", unmapped()),
        ("HorizontalDirection", number(base.horizontal_direction)),
        ("InsertionPoint", vec3(base.insertion_point)),
        ("IsAngular", bool_value(base.is_angular())),
        (
            "IsTextUserDefinedLocation",
            bool_value(base.text_user_positioned),
        ),
        ("LineSpacingFactor", number(base.line_spacing_factor)),
        (
            "LineSpacingStyle",
            enum_value(base.line_spacing_style as i64),
        ),
        ("Measurement", number(base.actual_measurement)),
        ("Normal", vec3(base.normal)),
        ("Offset", unmapped()),
        ("Style", style),
        ("Text", Value::String(base.text.clone())),
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

/// Visit the backing collection, including structural BLOCK/ENDBLK records
/// hidden by the public drawing-entity iterator, and retained children.
pub(super) fn collect(doc: &CadDocument, out: &mut Vec<Value>) {
    for entity in &doc.entities {
        collect_entity(doc, entity, out);
    }
}

fn collect_entity(doc: &CadDocument, entity: &EntityType, out: &mut Vec<Value>) {
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
                ("Sweep", number(value.sweep_angle())),
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
                    number(normalized_angle(
                        value.major_axis.y.atan2(value.major_axis.x),
                    )),
                ),
                ("StartParameter", number(value.start_parameter)),
                ("Thickness", unmapped()),
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
        EntityType::Polyline2D(value) => field_common(
            doc,
            &value.common,
            "Polyline2D",
            "POLYLINE",
            0x0F,
            "AcDb2dPolyline",
            false,
            vec![
                ("Elevation", number(value.elevation)),
                ("EndWidth", number(value.end_width)),
                ("Flags", enum_value(value.flags.bits() as i64)),
                ("IsClosed", bool_value(value.is_closed())),
                ("MatchVerticesEntityProperties", bool_value(true)),
                ("Normal", vec3(value.normal)),
                ("SmoothSurface", enum_value(value.smooth_surface as i64)),
                ("StartWidth", number(value.start_width)),
                ("Thickness", number(value.thickness)),
                // Heavy-polyline vertices are value structs in acadrust;
                // their Seqend/CadObject identities are not retained.
                ("Vertices", unmapped()),
            ],
            out,
        ),
        EntityType::Polyline3D(value) => {
            let vertices = polyline3d_vertices(doc, value, out);
            field_common(
                doc,
                &value.common,
                "Polyline3D",
                "POLYLINE",
                0x10,
                "AcDb3dPolyline",
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
                    ("Thickness", unmapped()),
                    ("Vertices", vertices),
                ],
                out,
            );
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
                ("Flags", enum_value(if value.is_closed { 1 } else { 0 })),
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
                    value.alignment_point.map(vec3).unwrap_or_else(unmapped),
                ),
                ("Height", number(value.height)),
                (
                    "HorizontalAlignment",
                    enum_value(text_horizontal(value.horizontal_alignment)),
                ),
                ("InsertPoint", vec3(value.insertion_point)),
                ("Mirror", enum_value((value.generation_flags & 6) as i64)),
                ("Normal", vec3(value.normal)),
                ("ObliqueAngle", number(value.oblique_angle)),
                ("Rotation", number(value.rotation)),
                ("Thickness", number(value.thickness)),
                ("Value", Value::String(value.value.clone())),
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
            &value.common,
            "MText",
            "MTEXT",
            0x2C,
            "AcDbMText",
            false,
            vec![
                (
                    "AlignmentPoint",
                    value.dwg_x_direction.map(vec3).unwrap_or_else(unmapped),
                ),
                ("AttachmentPoint", enum_value(value.attachment_point as i64)),
                ("BackgroundColor", color(value.background_color)),
                (
                    "BackgroundFillFlags",
                    enum_value(value.background_fill_flags as i64),
                ),
                ("BackgroundScale", number(value.background_scale)),
                (
                    "BackgroundTransparency",
                    transparency_from_raw(value.background_transparency),
                ),
                ("ColumnData", mtext_columns(&value.column_data)),
                (
                    "DrawingDirection",
                    enum_value(value.drawing_direction as i64),
                ),
                ("Height", number(value.height)),
                ("HorizontalWidth", number(value.extents_width)),
                ("InsertPoint", vec3(value.insertion_point)),
                ("IsAnnotative", bool_value(value.is_annotative)),
                ("LineSpacing", number(value.line_spacing_factor)),
                (
                    "LineSpacingStyle",
                    enum_value(value.line_spacing_style as i64),
                ),
                ("Normal", vec3(value.normal)),
                (
                    "RectangleHeight",
                    value.rectangle_height.map(number).unwrap_or_else(unmapped),
                ),
                ("RectangleWidth", number(value.rectangle_width)),
                ("Rotation", number(value.rotation)),
                (
                    "Style",
                    optional_handle(doc.text_styles.get(&value.style).map(|style| style.handle)),
                ),
                ("Value", Value::String(value.value.clone())),
                ("VerticalHeight", number(value.extents_height)),
            ],
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
            attribute_definition_props(value),
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
            attribute_entity_props(value),
            out,
        ),
        EntityType::Insert(value) => {
            let attributes = value
                .attributes
                .iter()
                .map(|attribute| {
                    collect_attribute_inline(doc, attribute, value.common.handle, out);
                    handle(attribute.common.handle)
                })
                .collect::<Vec<_>>();
            field_common(
                doc,
                &value.common,
                "Insert",
                if value.is_array() { "INSERT" } else { "INSERT" },
                if value.is_array() { 8 } else { 7 },
                if value.is_array() {
                    "AcDbMInsertBlock"
                } else {
                    "AcDbBlockReference"
                },
                true,
                vec![
                    ("Attributes", array(attributes.into_iter())),
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
                    ("HasAttributes", bool_value(value.has_attributes())),
                    ("IsMultiple", bool_value(value.is_array())),
                    ("InsertPoint", vec3(value.insert_point)),
                    ("Normal", vec3(value.normal)),
                    ("Rotation", number(value.rotation)),
                    ("RowCount", integer(value.row_count as i64)),
                    ("RowSpacing", number(value.row_spacing)),
                    ("SpatialFilter", unmapped()),
                    ("XScale", number(value.x_scale())),
                    ("YScale", number(value.y_scale())),
                    ("ZScale", number(value.z_scale())),
                ],
                out,
            );
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
        EntityType::Spline(value) => field_common(
            doc,
            &value.common,
            "Spline",
            "SPLINE",
            0x24,
            "AcDbSpline",
            false,
            vec![
                (
                    "ControlPoints",
                    points3(value.control_points.iter().copied()),
                ),
                ("Degree", integer(value.degree as i64)),
                ("EndTangent", vec3(value.end_tangent)),
                ("FitPoints", points3(value.fit_points.iter().copied())),
                ("Flags", enum_value(value.dxf_flags as i64)),
                ("Knots", array(value.knots.iter().copied().map(number))),
                ("Normal", vec3(value.normal)),
                ("StartTangent", vec3(value.begin_tangent)),
                ("Weights", array(value.weights.iter().copied().map(number))),
            ],
            out,
        ),
        EntityType::Hatch(value) => collect_hatch(doc, value, out),
        EntityType::Dimension(value) => collect_dimension(doc, value, out),
        EntityType::Leader(value) => field_common(
            doc,
            &value.common,
            "Leader",
            "LEADER",
            0x2D,
            "AcDbLeader",
            false,
            vec![
                ("AnnotationOffset", vec3(value.annotation_offset)),
                ("ArrowHeadEnabled", bool_value(value.arrow_enabled)),
                (
                    "AssociatedAnnotation",
                    optional_handle(if value.annotation_handle.is_null() {
                        None
                    } else {
                        Some(value.annotation_handle)
                    }),
                ),
                ("BlockOffset", vec3(value.block_offset)),
                (
                    "CreationType",
                    enum_value(value.creation_type.to_value() as i64),
                ),
                ("HasHookline", bool_value(value.hookline_enabled)),
                (
                    "HookLineDirection",
                    enum_value(value.hookline_direction.to_value() as i64),
                ),
                ("HorizontalDirection", vec3(value.horizontal_direction)),
                ("Normal", vec3(value.normal)),
                ("PathType", enum_value(value.path_type.to_value() as i64)),
                ("Style", unmapped()),
                ("TextHeight", number(value.text_height)),
                ("TextWidth", number(value.text_width)),
                ("Vertices", points3(value.vertices.iter().copied())),
            ],
            out,
        ),
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
                ("Direction", vec3(value.direction)),
                ("InsertionPoint", vec3(value.insertion_point)),
                ("Normal", vec3(value.normal)),
                ("Style", optional_handle(value.dimension_style_handle)),
                ("Text", Value::String(value.text.clone())),
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
                ("Version", integer(value.version as i64)),
                ("Vertices", points3(value.vertices.iter().copied())),
            ],
            out,
        ),
        EntityType::PolyfaceMesh(value) => collect_polyface(doc, value, out),
        EntityType::PolygonMesh(value) => collect_polygon_mesh(doc, value, out),
        EntityType::Solid3D(value) => collect_modeler(
            doc,
            &value.common,
            "Solid3D",
            "3DSOLID",
            0x26,
            "AcDb3dSolid",
            &value.acis_data,
            out,
        ),
        EntityType::Region(value) => collect_modeler(
            doc,
            &value.common,
            "Region",
            "REGION",
            0x25,
            "AcDbRegion",
            &value.acis_data,
            out,
        ),
        EntityType::Body(value) => collect_modeler(
            doc,
            &value.common,
            "CadBody",
            "BODY",
            0x27,
            "AcDbBody",
            &value.acis_data,
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
                ("BinaryData", bytes(&value.encoded_payload())),
                ("IsPaperSpace", bool_value(value.is_paper_space)),
                ("LowerRightCorner", vec3(value.lower_right_corner)),
                ("OleObjectType", enum_value(value.ole_object_type as i64)),
                (
                    "SourceApplication",
                    Value::String(value.source_application.clone()),
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
        EntityType::Unknown(value) => field_common(
            doc,
            &value.common,
            super::type_names::canonical(&value.dxf_name).unwrap_or("UnknownEntity"),
            &value.dxf_name,
            value.dwg_type_code as i64,
            "AcDbEntity",
            false,
            vec![("DxfClass", dxf_class(&value.dxf_name))],
            out,
        ),
        EntityType::Block(value) => field_common(
            doc,
            &value.common,
            "Block",
            "BLOCK",
            4,
            "AcDbBlockBegin",
            false,
            vec![
                ("BasePoint", vec3(value.base_point)),
                (
                    "BlockOwner",
                    optional_handle(Some(value.common.owner_handle)),
                ),
                ("Comments", Value::String(value.description.clone())),
                ("Flags", unmapped()),
                ("IsUnloaded", unmapped()),
                ("Name", Value::String(value.name.clone())),
                ("XRefPath", Value::String(value.xref_path.clone())),
            ],
            out,
        ),
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
        EntityType::Table(value) => field_common(
            doc,
            &value.common,
            "TableEntity",
            "TABLE",
            -999,
            "AcDbTable",
            false,
            Vec::new(),
            out,
        ),
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

fn transparency_from_raw(value: i32) -> Value {
    transparency(Transparency::Explicit(value.clamp(0, 255) as u8))
}

fn mtext_columns(value: &crate::entities::MTextColumnData) -> Value {
    inline(
        "MText.TextColumnData",
        vec![
            ("AutoHeight", bool_value(value.auto_height)),
            ("ColumnCount", integer(value.column_count as i64)),
            ("ColumnType", enum_value(value.column_type as i64)),
            ("FlowReversed", bool_value(value.flow_reversed)),
            ("Gutter", number(value.gutter)),
            ("Heights", array(value.heights.iter().copied().map(number))),
            ("Width", number(value.width)),
        ],
    )
}

fn attribute_definition_props(
    value: &crate::entities::AttributeDefinition,
) -> Vec<(&'static str, Value)> {
    vec![
        ("AlignmentPoint", vec3(value.alignment_point)),
        (
            "AttributeType",
            enum_value(value.mtext_flag.to_value() as i64),
        ),
        ("Flags", enum_value(value.flags.to_bits() as i64)),
        ("Height", number(value.height)),
        (
            "HorizontalAlignment",
            enum_value(value.horizontal_alignment.to_value() as i64),
        ),
        ("InsertPoint", vec3(value.insertion_point)),
        ("IsLocked", bool_value(value.lock_position)),
        (
            "MText",
            optional_handle(value.embedded_mtext.as_ref().map(|m| m.common.handle)),
        ),
        ("Normal", vec3(value.normal)),
        ("ObliqueAngle", number(value.oblique_angle)),
        ("Prompt", Value::String(value.prompt.clone())),
        ("Rotation", number(value.rotation)),
        ("Style", unmapped()),
        ("Tag", Value::String(value.tag.clone())),
        ("Thickness", unmapped()),
        ("Value", Value::String(value.default_value.clone())),
        ("Version", unmapped()),
        (
            "VerticalAlignment",
            enum_value(value.vertical_alignment.to_value() as i64),
        ),
        ("WidthFactor", number(value.width_factor)),
    ]
}

fn attribute_entity_props(value: &crate::entities::AttributeEntity) -> Vec<(&'static str, Value)> {
    vec![
        ("AlignmentPoint", vec3(value.alignment_point)),
        (
            "AttributeType",
            enum_value(value.mtext_flag.to_value() as i64),
        ),
        ("Flags", enum_value(value.flags.to_bits() as i64)),
        ("Height", number(value.height)),
        (
            "HorizontalAlignment",
            enum_value(value.horizontal_alignment.to_value() as i64),
        ),
        ("InsertPoint", vec3(value.insertion_point)),
        ("IsLocked", bool_value(value.lock_position)),
        (
            "MText",
            optional_handle(value.embedded_mtext.as_ref().map(|m| m.common.handle)),
        ),
        ("Normal", vec3(value.normal)),
        ("ObliqueAngle", number(value.oblique_angle)),
        ("Rotation", number(value.rotation)),
        ("Style", unmapped()),
        ("Tag", Value::String(value.tag.clone())),
        ("Thickness", unmapped()),
        ("Value", Value::String(value.value.clone())),
        ("Version", unmapped()),
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
        attribute_entity_props(value),
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
            ],
        ),
        Dimension::Radius(value) => dimension_common(
            doc,
            &value.base,
            "DimensionRadius",
            0x19,
            "AcDbRadialDimension",
            vec![
                ("DefinitionPoint", vec3(value.definition_point)),
                ("AngleVertex", vec3(value.angle_vertex)),
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
                ("Measurement", number(value.measurement_radians())),
                ("Center", vec3(value.angle_vertex)),
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
                ("FeatureLocation", vec3(value.feature_location)),
                ("LeaderEndpoint", vec3(value.leader_endpoint)),
                ("DefinitionPoint", vec3(value.definition_point)),
                ("IsOrdinateTypeX", bool_value(value.is_ordinate_type_x)),
                ("Measurement", number(value.measurement())),
            ],
        ),
        Dimension::Arc(value) => dimension_common(
            doc,
            &value.base,
            "DimensionArc",
            -999,
            "AcDbArcDimension",
            vec![
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
                ("Measurement", number(value.measurement())),
            ],
        ),
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
    let paths = value.paths.iter().map(hatch_path).collect::<Vec<_>>();
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
            ("IsSolid", bool_value(value.is_solid)),
            ("Normal", vec3(value.normal)),
            ("Paths", array(paths.into_iter())),
            ("PatternAngle", number(value.pattern_angle)),
            ("PatternScale", number(value.pattern_scale)),
            ("PatternType", enum_value(value.pattern_type as i64)),
            ("PixelSize", number(value.pixel_size)),
            ("SeedPoints", points2(value.seed_points.iter().copied())),
            ("Style", enum_value(value.style as i64)),
        ],
        out,
    );
}

fn hatch_path(path: &crate::entities::BoundaryPath) -> Value {
    inline(
        "Hatch.BoundaryPath",
        vec![
            ("Edges", array(path.edges.iter().map(hatch_edge))),
            ("Entities", unmapped()),
            ("Flags", enum_value(path.flags.bits() as i64)),
        ],
    )
}

fn hatch_edge(edge: &crate::entities::BoundaryEdge) -> Value {
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
                    points3(value.control_points.iter().copied()),
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
    field_common(
        doc,
        &value.common,
        "Viewport",
        "VIEWPORT",
        0x22,
        "AcDbViewport",
        false,
        vec![
            ("ActiveStatus", integer(value.id as i64)),
            ("AmbientLightColor", color(value.ambient_color)),
            ("BackClipPlane", number(value.back_clip_z)),
            ("Center", vec3(value.center)),
            ("CircleZoomPercent", integer(value.circle_sides as i64)),
            ("Contrast", number(value.contrast)),
            ("DisplayUcsIcon", bool_value(value.ucs_icon_visible)),
            ("Elevation", number(value.elevation)),
            ("FrontClipPlane", number(value.front_clip_z)),
            (
                "GridSpacing",
                vec2(Vector2::new(value.grid_spacing.x, value.grid_spacing.y)),
            ),
            ("Height", number(value.height)),
            ("Id", integer(value.id as i64)),
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
            ("Status", enum_value(value.status.to_bits() as i64)),
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
            ("ViewWidth", number(value.width)),
            ("Width", number(value.width)),
        ],
        out,
    );
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
                value.crease.map(number).unwrap_or_else(|| Value::Null),
            ),
            ("End", integer(value.end as i64)),
            ("Start", integer(value.start as i64)),
        ],
    )
}

fn collect_polyface(
    doc: &CadDocument,
    value: &crate::entities::PolyfaceMesh,
    out: &mut Vec<Value>,
) {
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
            field_common(
                doc,
                &vertex.common,
                "VertexFaceMesh",
                "VERTEX",
                0x0D,
                "AcDbPolyFaceMeshVertex",
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
            field_common(
                doc,
                &face.common,
                "VertexFaceRecord",
                "VERTEX",
                0x0E,
                "AcDbFaceRecord",
                false,
                vec![
                    ("Flags", enum_value(face.flags.bits() as i64)),
                    ("Index1", integer(face.index1 as i64)),
                    ("Index2", integer(face.index2 as i64)),
                    ("Index3", integer(face.index3 as i64)),
                    ("Index4", integer(face.index4 as i64)),
                ],
                out,
            );
            Some(handle(face.common.handle))
        })
        .collect::<Vec<_>>();
    field_common(
        doc,
        &value.common,
        "PolyfaceMesh",
        "POLYLINE",
        0x1D,
        "AcDbPolyFaceMesh",
        false,
        vec![
            ("Elevation", unmapped()),
            ("EndWidth", unmapped()),
            (
                "Faces",
                if faces_complete {
                    array(faces.into_iter())
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
            ("StartWidth", unmapped()),
            ("Thickness", number(value.thickness)),
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

fn collect_polygon_mesh(
    doc: &CadDocument,
    value: &crate::entities::PolygonMeshEntity,
    out: &mut Vec<Value>,
) {
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
            Some(handle(vertex.common.handle))
        })
        .collect::<Vec<_>>();
    field_common(
        doc,
        &value.common,
        "PolygonMesh",
        "POLYLINE",
        0x1E,
        "AcDbPolygonMesh",
        false,
        vec![
            ("Elevation", number(value.elevation)),
            ("Flags", enum_value(value.flags.bits() as i64)),
            (
                "IsClosed",
                bool_value(value.is_closed_m() || value.is_closed_n()),
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

fn collect_modeler(
    doc: &CadDocument,
    common: &EntityCommon,
    type_name: &'static str,
    object_name: &'static str,
    object_type: i64,
    subclass: &'static str,
    acis_data: &crate::entities::AcisData,
    out: &mut Vec<Value>,
) {
    let data = if acis_data.is_binary {
        bytes(&acis_data.sab_data)
    } else {
        bytes(acis_data.sat_data.as_bytes())
    };
    field_common(
        doc,
        common,
        type_name,
        object_name,
        object_type,
        subclass,
        false,
        vec![
            ("AcisData", data),
            ("IsBinaryAcisData", bool_value(acis_data.is_binary)),
            ("ModelerFormatVersion", integer(acis_data.version as i64)),
            ("Point", unmapped()),
        ],
        out,
    );
}

fn collect_surface(doc: &CadDocument, value: &crate::entities::Surface, out: &mut Vec<Value>) {
    collect_modeler(
        doc,
        &value.common,
        "Surface",
        "SURFACE",
        -999,
        "AcDbSurface",
        &value.acis_data,
        out,
    )
}

fn collect_underlay(doc: &CadDocument, value: &crate::entities::Underlay, out: &mut Vec<Value>) {
    let (type_name, object_name, subclass) = match value.underlay_type {
        crate::entities::UnderlayType::Pdf => ("PdfUnderlay", "PDFUNDERLAY", "AcDbPdfReference"),
        crate::entities::UnderlayType::Dwf => ("DwfUnderlay", "DWFUNDERLAY", "AcDbDwfReference"),
        crate::entities::UnderlayType::Dgn => ("DgnUnderlay", "DGNUNDERLAY", "AcDbDgnReference"),
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
            let dxf_name = if data.dxf_subclass.is_empty() {
                class_name
            } else {
                data.dxf_subclass.as_str()
            };
            field_common(
                doc,
                &value.common,
                "ProxyEntity",
                class_name,
                0x1f2,
                "AcDbProxyEntity",
                false,
                vec![
                    ("ClassId", integer(data.class_id)),
                    ("Data", bytes(&data.payload.data())),
                    ("DxfClass", dxf_class(dxf_name)),
                    ("DrawingFormat", integer(data.dwg_version)),
                    ("MaintenanceVersion", integer(data.maintenance_version)),
                    ("OriginalDataFormatDxf", bool_value(data.from_dxf)),
                    ("ProxyClassId", integer(data.proxy_id)),
                    ("Version", integer(data.version)),
                ],
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
    field_common(
        doc,
        &value.common,
        "MultiLeader",
        "MULTILEADER",
        -999,
        "AcDbMLeader",
        false,
        vec![
            ("Arrowhead", optional_handle(value.arrowhead_handle)),
            ("ArrowheadSize", number(value.arrowhead_size)),
            ("BlockContent", optional_handle(value.block_content_handle)),
            ("BlockContentColor", color(value.block_content_color)),
            ("BlockContentRotation", number(value.block_rotation)),
            ("BlockContentScale", vec3(value.block_scale)),
            ("ContentType", enum_value(value.content_type as i64)),
            (
                "EnableAnnotationScale",
                bool_value(value.enable_annotation_scale),
            ),
            ("EnableDogleg", bool_value(value.enable_dogleg)),
            ("EnableLanding", bool_value(value.enable_landing)),
            ("LandingDistance", number(value.dogleg_length)),
            ("LineColor", color(value.line_color)),
            ("ScaleFactor", number(value.scale_factor)),
            ("TextColor", color(value.text_color)),
            ("TextFrame", bool_value(value.text_frame)),
        ],
        out,
    )
}

fn collect_mline(doc: &CadDocument, value: &crate::entities::MLine, out: &mut Vec<Value>) {
    field_common(
        doc,
        &value.common,
        "MLine",
        "MLINE",
        0x2F,
        "AcDbMline",
        false,
        vec![
            ("Flags", enum_value(value.flags.bits() as i64)),
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
                            ("Segments", unmapped()),
                        ],
                    )
                })),
            ),
        ],
        out,
    )
}
