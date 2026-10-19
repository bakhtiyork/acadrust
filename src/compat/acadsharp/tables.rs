use super::*;
use crate::tables::{Table, TableEntry};

fn metadata(ty: &str) -> Vec<(&'static str, Value)> {
    match ty {
        "DimensionStylesTable" => vec![
            ("ObjectName", json!("DIMSTYLE")),
            ("ObjectType", json!(68)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "UCS" => vec![
            ("ObjectName", json!("UCS")),
            ("ObjectType", json!(63)),
            ("SubclassMarker", json!("AcDbUCSTableRecord")),
        ],
        "BlockEnd" => vec![
            ("ProxyGeometries", json!([])),
            ("ObjectName", json!("ENDBLK")),
            ("ObjectType", json!(5)),
            ("SubclassMarker", json!("AcDbBlockEnd")),
        ],
        "TextStylesTable" => vec![
            ("ObjectName", json!("STYLE")),
            ("ObjectType", json!(52)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "BlockRecord" => vec![
            ("ObjectName", json!("BLOCK_RECORD")),
            ("ObjectType", json!(49)),
            ("SubclassMarker", json!("AcDbBlockTableRecord")),
        ],
        "View" => vec![
            ("ObjectName", json!("VIEW")),
            ("ObjectType", json!(61)),
            ("SubclassMarker", json!("AcDbViewTableRecord")),
        ],
        "TextStyle" => vec![
            ("ObjectName", json!("STYLE")),
            ("ObjectType", json!(53)),
            ("SubclassMarker", json!("AcDbTextStyleTableRecord")),
        ],
        "AppId" => vec![
            ("ObjectName", json!("APPID")),
            ("ObjectType", json!(67)),
            ("SubclassMarker", json!("AcDbRegAppTableRecord")),
        ],
        "VPortsTable" => vec![
            ("ObjectName", json!("VPORT")),
            ("ObjectType", json!(64)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "BlockRecordsTable" => vec![
            ("ObjectName", json!("BLOCK_RECORD")),
            ("ObjectType", json!(48)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "AppIdsTable" => vec![
            ("ObjectName", json!("APPID")),
            ("ObjectType", json!(66)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "Layer" => vec![
            ("ObjectName", json!("LAYER")),
            ("ObjectType", json!(51)),
            ("SubclassMarker", json!("AcDbLayerTableRecord")),
        ],
        "DimensionStyle" => vec![
            ("ObjectName", json!("DIMSTYLE")),
            ("ObjectType", json!(69)),
            ("SubclassMarker", json!("AcDbDimStyleTableRecord")),
        ],
        "LineTypesTable" => vec![
            ("ObjectName", json!("LTYPE")),
            ("ObjectType", json!(56)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "VPort" => vec![
            ("ObjectName", json!("VPORT")),
            ("ObjectType", json!(65)),
            ("SubclassMarker", json!("AcDbViewportTableRecord")),
        ],
        "ViewsTable" => vec![
            ("ObjectName", json!("VIEW")),
            ("ObjectType", json!(60)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "LayersTable" => vec![
            ("ObjectName", json!("LAYER")),
            ("ObjectType", json!(50)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        "LineType" => vec![
            ("ObjectName", json!("LTYPE")),
            ("ObjectType", json!(57)),
            ("SubclassMarker", json!("AcDbLinetypeTableRecord")),
        ],
        "Block" => vec![
            ("ProxyGeometries", json!([])),
            ("ObjectName", json!("BLOCK")),
            ("ObjectType", json!(4)),
            ("SubclassMarker", json!("AcDbBlockBegin")),
        ],
        "UCSTable" => vec![
            ("ObjectName", json!("UCS")),
            ("ObjectType", json!(62)),
            ("SubclassMarker", json!("AcDbSymbolTable")),
        ],
        _ => Vec::new(),
    }
}
fn table<T: TableEntry>(doc: &CadDocument, table: &Table<T>, ty: &str, out: &mut Vec<Value>) {
    let mut p = metadata(ty);
    p.push(("Count", json!(table.len())));
    p.extend(base(doc, table.handle()));
    if ty == "LineTypesTable" {
        p.push((
            "ByLayer",
            doc.line_types
                .get("ByLayer")
                .map(|v| handle(v.handle))
                .unwrap_or(Value::Null),
        ));
        p.push((
            "Continuous",
            doc.line_types
                .get("Continuous")
                .map(|v| handle(v.handle))
                .unwrap_or(Value::Null),
        ));
    }
    let mut object = record(table.handle(), ty, Handle::NULL, props(ty, p));
    object["owner"] = handle(Handle::NULL); // Document owner, not an absent owner.
    out.push(object);
}
fn entry(
    doc: &CadDocument,
    h: Handle,
    ty: &str,
    owner: Handle,
    mapped: Vec<(&'static str, Value)>,
    out: &mut Vec<Value>,
) {
    let mut p = metadata(ty);
    p.extend(mapped);
    p.extend(base(doc, h));
    out.push(record(h, ty, owner, props(ty, p)));
}
pub(super) fn collect(doc: &CadDocument, out: &mut Vec<Value>) {
    table(doc, &doc.layers, "LayersTable", out);
    for v in doc.layers.iter() {
        entry(
            doc,
            v.handle,
            "Layer",
            doc.layers.handle(),
            vec![
                (
                    "Flags",
                    json!(
                        symbol_flags(doc, v.source_flags) & !23
                            | i16::from(v.flags.frozen)
                            | (i16::from(v.flags.frozen_in_new_viewport) << 1)
                            | (i16::from(v.flags.locked) << 2)
                            | (i16::from(v.flags.xref_dependent) << 4)
                    ),
                ),
                ("Color", json!(color(v.color))),
                ("IsOn", json!(!v.flags.off)),
                (
                    "LineType",
                    json!(doc
                        .line_types
                        .get(&v.line_type)
                        .map(|v| handle(v.handle))
                        .unwrap_or_else(|| json!(UNMAPPED))),
                ),
                ("LineWeight", json!(v.line_weight.value())),
                (
                    "Material",
                    if super::registration::source_document(doc) {
                        Value::Null
                    } else {
                        optional_handle(Some(v.material))
                    },
                ),
                ("Name", json!(v.name)),
                (
                    "PlotFlag",
                    json!(v.is_plottable && !v.name.eq_ignore_ascii_case("defpoints")),
                ),
                (
                    "PlotStyleName",
                    json!(if source_dwg(doc) {
                        0
                    } else {
                        v.plotstyle_handle.value()
                    }),
                ),
            ],
            out,
        );
    }
    table(doc, &doc.line_types, "LineTypesTable", out);
    for v in doc.line_types.iter() {
        entry(
            doc,
            v.handle,
            "LineType",
            doc.line_types.handle(),
            vec![
                (
                    "Flags",
                    json!(
                        symbol_flags(doc, v.source_flags) & !16
                            | (i16::from(v.xref_dependent) << 4)
                    ),
                ),
                ("Alignment", character(v.alignment)),
                (
                    "Description",
                    if !v.description_present
                        && (v.description.is_empty()
                            || (super::registration::source_document(doc)
                                && v.name.eq_ignore_ascii_case("Continuous")
                                && v.description == "Solid line"))
                    {
                        Value::Null
                    } else {
                        json!(v.description)
                    },
                ),
                ("Segments", line_type_segments(doc, v)),
                (
                    "HasShapes",
                    json!(v
                        .elements
                        .iter()
                        .any(|s| s.complex.as_ref().is_some_and(|c| matches!(
                            c.content,
                            crate::tables::linetype::LineTypeComplexContent::Shape { .. }
                        )))),
                ),
                ("IsComplex", json!(!v.elements.is_empty())),
                ("Name", json!(v.name)),
                (
                    "PatternLength",
                    number(v.elements.iter().map(|s| s.length.abs()).sum()),
                ),
            ],
            out,
        );
    }
    table(doc, &doc.text_styles, "TextStylesTable", out);
    let mut used_names = std::collections::HashSet::new();
    let mut unnamed = 0;
    for v in doc.text_styles.iter() {
        text_style(doc, v, out);
        if v.name.is_empty() {
            let name = loop {
                let candidate = format!("unnamed{unnamed}");
                unnamed += 1;
                if !used_names.contains(&crate::tables::normalize_name(&candidate)) {
                    break candidate;
                }
            };
            used_names.insert(crate::tables::normalize_name(&name));
            out.last_mut().unwrap()["props"]["Name"] = json!(name);
        } else {
            used_names.insert(crate::tables::normalize_name(&v.name));
        }
    }
    table(doc, &doc.block_records, "BlockRecordsTable", out);
    for v in doc.block_records.iter() {
        entry(
            doc,
            v.handle,
            "BlockRecord",
            doc.block_records.handle(),
            vec![
                (
                    "AttributeDefinitions",
                    json!(v
                        .entity_handles
                        .iter()
                        .filter(|h| matches!(
                            doc.get_entity(**h),
                            Some(crate::entities::EntityType::AttributeDefinition(_))
                        ))
                        .copied()
                        .map(handle)
                        .collect::<Vec<_>>()),
                ),
                ("BlockEnd", json!(optional_handle(Some(v.block_end_handle)))),
                (
                    "BlockEntity",
                    json!(optional_handle(Some(v.block_entity_handle))),
                ),
                ("CanScale", json!(!v.scale_uniformly)),
                (
                    "Entities",
                    json!(v
                        .entity_handles
                        .iter()
                        .copied()
                        .map(handle)
                        .collect::<Vec<_>>()),
                ),
                (
                    "EvaluationGraph",
                    optional_handle(extension_entry(doc, v.handle, "ACAD_ENHANCEDBLOCK")),
                ),
                (
                    "IsDynamic",
                    json!(extension_entry(doc, v.handle, "ACAD_ENHANCEDBLOCK").is_some()),
                ),
                (
                    "SortEntitiesTable",
                    optional_handle(extension_entry(doc, v.handle, "ACAD_SORTENTS")),
                ),
                (
                    "HasAttributes",
                    json!(v.entity_handles.iter().any(|h| matches!(
                        doc.get_entity(*h),
                        Some(crate::entities::EntityType::AttributeDefinition(_))
                    ))),
                ),
                ("IsAnonymous", json!(v.flags.anonymous)),
                ("IsExplodable", json!(v.explodable)),
                ("IsUnloaded", json!(v.flags.is_xref_unloaded)),
                ("Layout", json!(optional_handle(Some(v.layout)))),
                ("Name", json!(v.name)),
                ("Preview", json!(bytes(&v.preview_data))),
                ("Units", json!(v.units)),
                (
                    "Viewports",
                    json!(v
                        .entity_handles
                        .iter()
                        .filter(|h| matches!(
                            doc.get_entity(**h),
                            Some(crate::entities::EntityType::Viewport(_))
                        ))
                        .copied()
                        .map(handle)
                        .collect::<Vec<_>>()),
                ),
            ],
            out,
        );
        block_entities(doc, v, out);
    }
    table(doc, &doc.dim_styles, "DimensionStylesTable", out);
    for v in doc.dim_styles.iter() {
        entry(
            doc,
            v.handle,
            "DimensionStyle",
            doc.dim_styles.handle(),
            super::table_dimstyle::map(doc, v),
            out,
        );
    }
    table(doc, &doc.app_ids, "AppIdsTable", out);
    for v in doc.app_ids.iter() {
        entry(
            doc,
            v.handle,
            "AppId",
            doc.app_ids.handle(),
            vec![
                ("Name", json!(v.name)),
                ("Flags", json!(symbol_flags(doc, v.source_flags))),
            ],
            out,
        );
    }
    table(doc, &doc.views, "ViewsTable", out);
    for v in doc.views.iter() {
        entry(
            doc,
            v.handle,
            "View",
            doc.views.handle(),
            vec![
                (
                    "Flags",
                    json!(
                        (v.xref_dependent as i16) * 16
                            | (v.xref_resolved as i16) * 32
                            | (v.xref_reference as i16) * 64
                    ),
                ),
                ("Angle", json!(number(v.twist_angle))),
                ("BackClipping", json!(number(v.back_clip))),
                (
                    "Center",
                    json!(json!([number(v.center.x), number(v.center.y)])),
                ),
                ("Direction", json!(vec3(v.direction))),
                ("FrontClipping", json!(number(v.front_clip))),
                ("Height", json!(number(v.height))),
                ("IsPlottable", json!(v.camera_plottable)),
                ("IsUcsAssociated", json!(v.ucs_associated)),
                ("LensLength", json!(number(v.lens_length))),
                ("Name", json!(v.name)),
                ("RenderMode", json!(v.render_mode as i16)),
                ("Target", json!(vec3(v.target))),
                ("UcsElevation", json!(number(v.ucs_elevation))),
                ("UcsOrigin", json!(vec3(v.ucs_origin))),
                ("UcsOrthographicType", json!(v.ucs_ortho_type)),
                (
                    "VisualStyle",
                    json!(optional_handle(Some(v.visual_style_handle))),
                ),
                ("Width", json!(number(v.width))),
            ]
            .into_iter()
            .chain(super::table_viewports::view_overrides(doc, v))
            .collect(),
            out,
        );
    }
    table(doc, &doc.vports, "VPortsTable", out);
    for v in doc.vports.iter() {
        entry(
            doc,
            v.handle,
            "VPort",
            doc.vports.handle(),
            vec![
                (
                    "Flags",
                    json!(
                        (v.xref_dependent as i16) * 16
                            | (v.xref_resolved as i16) * 32
                            | (v.xref_reference as i16) * 64
                    ),
                ),
                ("AmbientColor", json!(color(v.ambient_color))),
                ("AspectRatio", json!(number(v.aspect_ratio))),
                ("BackClippingPlane", json!(number(v.back_clip))),
                ("BaseUcs", json!(optional_handle(Some(v.base_ucs_handle)))),
                ("BottomLeft", json!(vec2(v.lower_left))),
                ("Brightness", json!(number(v.brightness))),
                ("Center", json!(vec2(v.view_center))),
                ("CircleZoomPercent", json!(v.circle_zoom)),
                ("Contrast", json!(number(v.contrast))),
                ("DefaultLighting", json!(v.default_lighting_type)),
                ("Direction", json!(vec3(v.view_direction))),
                ("Elevation", json!(number(v.ucs_elevation))),
                ("FrontClippingPlane", json!(number(v.front_clip))),
                ("GridFlags", json!(v.grid_flags.to_bits())),
                ("GridSpacing", json!(vec2(v.grid_spacing))),
                ("IsometricSnap", json!(v.snap_style)),
                ("LensLength", json!(number(v.lens_length))),
                ("MinorGridLinesPerMajorGridLine", json!(v.grid_major)),
                ("Name", json!(v.name)),
                ("NamedUcs", json!(optional_handle(Some(v.named_ucs_handle)))),
                ("Origin", json!(vec3(v.ucs_origin))),
                ("OrthographicType", json!(v.ucs_ortho_type)),
                ("RenderMode", json!(v.render_mode as i16)),
                ("ShowGrid", json!(v.grid_on)),
                ("SnapBasePoint", json!(vec2(v.snap_base))),
                ("SnapIsoPair", json!(v.snap_isopair)),
                ("SnapOn", json!(v.snap_on)),
                ("SnapRotation", json!(number(v.snap_rotation))),
                ("SnapSpacing", json!(vec2(v.snap_spacing))),
                ("Target", json!(vec3(v.view_target))),
                ("TopRight", json!(vec2(v.upper_right))),
                ("TwistAngle", json!(number(v.view_twist))),
                ("UseDefaultLighting", json!(v.use_default_lights)),
                ("ViewHeight", json!(number(v.view_height))),
                (
                    "VisualStyle",
                    json!(optional_handle(Some(v.visual_style_handle))),
                ),
                ("XAxis", json!(vec3(v.ucs_x_axis))),
                ("YAxis", json!(vec3(v.ucs_y_axis))),
            ]
            .into_iter()
            .chain(super::table_viewports::vport_overrides(doc, v))
            .collect(),
            out,
        );
    }
    table(doc, &doc.ucss, "UCSTable", out);
    for v in doc.ucss.iter() {
        entry(
            doc,
            v.handle,
            "UCS",
            doc.ucss.handle(),
            vec![
                (
                    "Flags",
                    json!(
                        (v.xref_dependent as i16) * 16
                            | (v.xref_resolved as i16) * 32
                            | (v.xref_reference as i16) * 64
                    ),
                ),
                ("Elevation", json!(number(v.elevation))),
                ("Name", json!(v.name)),
                ("Origin", json!(vec3(v.origin))),
                ("OrthographicType", json!(v.ortho_type)),
                ("OrthographicViewType", json!(v.ortho_view_type)),
                ("XAxis", json!(vec3(v.x_axis))),
                ("YAxis", json!(vec3(v.y_axis))),
            ],
            out,
        );
    }
}

fn block_entities(doc: &CadDocument, b: &crate::tables::BlockRecord, out: &mut Vec<Value>) {
    // BLOCK/ENDBLK are collapsed into BlockRecord by some readers. Retain their
    // stored identities, but do not invent discarded EntityCommon properties.
    if !b.block_entity_handle.is_null() && doc.get_entity(b.block_entity_handle).is_none() {
        let mut p = b
            .block_entity_common
            .as_ref()
            .map(|common| super::entities::common(doc, common, "BLOCK", 4, "AcDbBlockBegin", false))
            .unwrap_or_else(|| metadata("Block"));
        p.extend([
            ("BasePoint", vec3(b.base_point)),
            ("BlockOwner", handle(b.handle)),
            ("Name", json!(b.name)),
            ("Comments", json!(b.description)),
            ("XRefPath", json!(b.xref_path)),
            ("IsUnloaded", json!(b.flags.is_xref_unloaded)),
        ]);
        out.push(record(
            b.block_entity_handle,
            "Block",
            b.handle,
            props("Block", p),
        ));
    }
    if !b.block_end_handle.is_null() && doc.get_entity(b.block_end_handle).is_none() {
        out.push(record(
            b.block_end_handle,
            "BlockEnd",
            b.handle,
            props(
                "BlockEnd",
                b.block_end_common
                    .as_ref()
                    .map(|common| {
                        super::entities::common(doc, common, "ENDBLK", 5, "AcDbBlockEnd", false)
                    })
                    .unwrap_or_else(|| metadata("BlockEnd")),
            ),
        ));
    }
}

fn line_type_segments(doc: &CadDocument, v: &crate::tables::LineType) -> Value {
    use crate::tables::LineTypeComplexContent;
    Value::Array(
        v.elements
            .iter()
            .map(|s| {
                let mut p = vec![
                    ("Length", number(s.length)),
                    ("Flags", json!(0)),
                    ("Scale", number(1.0)),
                    ("Rotation", number(0.0)),
                    ("Offset", json!([0.0, 0.0])),
                    ("Style", Value::Null),
                    ("ShapeNumber", json!(0)),
                    ("Text", json!("")),
                ];
                if let Some(c) = &s.complex {
                    p.extend([
                        (
                            "Flags",
                            json!(
                                (c.source_flags & !7)
                                    | u8::from(c.absolute_rotation)
                                    | if c.is_shape() { 4 } else { 2 }
                            ),
                        ),
                        ("Scale", number(c.scale)),
                        ("Rotation", number(c.rotation)),
                        ("Offset", json!([number(c.offset[0]), number(c.offset[1])])),
                        (
                            "Style",
                            // ACadSharp's DXF segment reader consumes code 340 without
                            // assigning its template reference; DWG resolves it.
                            if doc.header.dxf_source_variables.is_none()
                                && doc.text_styles.iter().any(|s| s.handle == c.style_handle)
                            {
                                optional_handle(Some(c.style_handle))
                            } else {
                                Value::Null
                            },
                        ),
                    ]);
                    match &c.content {
                        LineTypeComplexContent::Shape { shape_number } => {
                            p.push(("ShapeNumber", json!(shape_number)))
                        }
                        LineTypeComplexContent::Text { text } => p.push(("Text", json!(text))),
                    }
                }
                props("LineType.Segment", p)
            })
            .collect(),
    )
}

fn base(doc: &CadDocument, h: Handle) -> Vec<(&'static str, Value)> {
    let xdata = super::xdata::object_extended_data(doc, h);
    vec![
        ("ExtendedData", xdata),
        ("HasDynamicSubclass", json!(false)),
        (
            "Reactors",
            json!(doc
                .reactors_by_handle
                .get(&h)
                .into_iter()
                .flatten()
                .copied()
                .map(handle)
                .collect::<Vec<_>>()),
        ),
        (
            "XDictionary",
            optional_handle(doc.extension_dictionary_handle(h)),
        ),
    ]
}
fn extension_entry(doc: &CadDocument, owner: Handle, key: &str) -> Option<Handle> {
    let dict = doc.extension_dictionary_handle(owner)?;
    match doc.objects.get(&dict)? {
        crate::objects::ObjectType::Dictionary(d) => d.get(key),
        _ => None,
    }
}

/// Project a retained source STYLE, including unnamed entries overwritten in the native table.
pub(super) fn text_style(doc: &CadDocument, v: &crate::tables::TextStyle, out: &mut Vec<Value>) {
    entry(
        doc,
        v.handle,
        "TextStyle",
        doc.text_styles.handle(),
        vec![
            (
                "Flags",
                json!(
                    symbol_flags(doc, v.source_flags) & !21
                        | i16::from(v.is_shape_file)
                        | (i16::from(v.is_vertical) << 2)
                        | (if source_dwg(doc) {
                            v.source_flags & 16
                        } else {
                            i16::from(v.xref_dependent) << 4
                        })
                ),
            ),
            (
                "BigFontFilename",
                if !v.big_font_file_present && v.big_font_file.is_empty() {
                    Value::Null
                } else {
                    json!(v.big_font_file)
                },
            ),
            (
                "Filename",
                if v.dxf_present.is_some_and(|p| p & 1 == 0) && v.font_file == "txt" {
                    json!("")
                } else {
                    json!(v.font_file)
                },
            ),
            ("Height", json!(number(v.height))),
            ("IsShapeFile", json!(v.is_shape_file)),
            (
                "LastHeight",
                number(
                    if v.dxf_present.is_some_and(|p| p & 2 == 0) && v.last_height == 2.5 {
                        0.0
                    } else {
                        v.last_height
                    },
                ),
            ),
            (
                "MirrorFlag",
                json!((v.flags.backward as i16 * 2) | (v.flags.upside_down as i16 * 4)),
            ),
            ("Name", json!(v.name)),
            ("ObliqueAngle", json!(number(v.oblique_angle))),
            ("TrueType", json!(v.font_flags)),
            ("Width", json!(number(v.width_factor))),
        ],
        out,
    );
    out.last_mut().expect("text style record")["props"]["ExtendedData"] =
        super::xdata::text_style_extended_data(doc, v);
}

// DWG does not assign the DXF-only resolved flag in ACadSharp.
fn source_dwg(doc: &CadDocument) -> bool {
    doc.dwg_source_version.is_some()
}
fn symbol_flags(doc: &CadDocument, flags: i16) -> i16 {
    if source_dwg(doc) {
        flags & !32
    } else {
        flags
    }
}
