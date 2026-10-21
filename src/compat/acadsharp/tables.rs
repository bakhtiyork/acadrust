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
/// Borrowed recipes keep table properties out of the document-wide plan.
pub(super) fn records(doc: &CadDocument) -> Vec<Box<dyn Fn() -> Value + '_>> {
    let mut records: Vec<Box<dyn Fn() -> Value + '_>> = Vec::new();
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.layers, "LayersTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.layers.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
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
                    (
                        "Color",
                        json!(color(
                            // The DWG layer reader turns ByBlock/ByLayer into
                            // Color.Default (index 7).
                            if source_dwg(doc)
                                && matches!(
                                    v.color,
                                    crate::types::Color::ByBlock | crate::types::Color::ByLayer
                                )
                            {
                                crate::types::Color::Index(7)
                            } else {
                                v.color
                            }
                        ))
                    ),
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
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.line_types, "LineTypesTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.line_types.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
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
                        json!(v.elements.iter().any(|s| s.complex.as_ref().is_some_and(
                            |c| matches!(
                                c.content,
                                crate::tables::linetype::LineTypeComplexContent::Shape { .. }
                            )
                        ))),
                    ),
                    ("IsComplex", json!(!v.elements.is_empty())),
                    ("Name", json!(v.name)),
                    (
                        "PatternLength",
                        number(v.elements.iter().map(|s| s.length.abs()).sum()),
                    ),
                ],
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.text_styles, "TextStylesTable", &mut out);
        out.pop().expect("table record")
    }));
    let mut used_names = std::collections::HashSet::new();
    let mut unnamed = 0;
    for v in doc.text_styles.iter() {
        let name = if v.name.is_empty() {
            let name = loop {
                let candidate = format!("unnamed{unnamed}");
                unnamed += 1;
                if !used_names.contains(&crate::tables::normalize_name(&candidate)) {
                    break candidate;
                }
            };
            used_names.insert(crate::tables::normalize_name(&name));
            Some(name)
        } else {
            used_names.insert(crate::tables::normalize_name(&v.name));
            None
        };
        records.push(Box::new(move || {
            let mut out = Vec::new();
            text_style(doc, v, &mut out);
            let mut value = out.pop().expect("text style record");
            if let Some(name) = &name {
                value["props"]["Name"] = json!(name);
            }
            value
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.block_records, "BlockRecordsTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.block_records.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
            entry(
                doc,
                v.handle,
                "BlockRecord",
                doc.block_records.handle(),
                vec![
                    (
                        "AttributeDefinitions",
                        json!(view_entity_handles(doc, v)
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
                    ("CanScale", json!(block_can_scale(doc, v))),
                    ("Flags", json!(block_flags(doc, v))),
                    ("Source", block_source(doc, v)),
                    (
                        "Entities",
                        json!(view_entity_handles(doc, v)
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
                        json!(view_entity_handles(doc, v).iter().any(|h| matches!(
                            doc.get_entity(*h),
                            Some(crate::entities::EntityType::AttributeDefinition(_))
                        ))),
                    ),
                    ("IsAnonymous", json!(block_flags(doc, v) & 1 != 0)),
                    ("IsExplodable", json!(block_explodable(doc, v))),
                    ("IsUnloaded", json!(v.flags.is_xref_unloaded)),
                    ("Layout", json!(optional_handle(Some(v.layout)))),
                    ("Name", json!(block_name(doc, v))),
                    ("Preview", block_preview(doc, v)),
                    (
                        "Units",
                        json!(
                            if (source_dwg(doc) && doc.version < crate::types::DxfVersion::AC1021)
                                || v.dxf_present.is_some_and(|p| p & 32 == 0)
                            {
                                0
                            } else {
                                v.units
                            }
                        ),
                    ),
                    (
                        "Viewports",
                        json!(view_entity_handles(doc, v)
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
                &mut out,
            );

            out.pop().expect("table entry")
        }));
        for (end, h) in [(false, v.block_entity_handle), (true, v.block_end_handle)] {
            if !h.is_null() && doc.get_entity(h).is_none() {
                records.push(Box::new(move || {
                    let mut out = Vec::new();
                    super::entities::block_record_marker(doc, v, end, &mut out);
                    out.pop().expect("block marker")
                }));
            }
        }
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.dim_styles, "DimensionStylesTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.dim_styles.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
            entry(
                doc,
                v.handle,
                "DimensionStyle",
                doc.dim_styles.handle(),
                super::table_dimstyle::map(doc, v),
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.app_ids, "AppIdsTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.app_ids.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
            entry(
                doc,
                v.handle,
                "AppId",
                doc.app_ids.handle(),
                vec![
                    ("Name", json!(v.name)),
                    ("Flags", json!(symbol_flags(doc, v.source_flags))),
                ],
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.views, "ViewsTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.views.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
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
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.vports, "VPortsTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.vports.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
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
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records.push(Box::new(move || {
        let mut out = Vec::new();
        table(doc, &doc.ucss, "UCSTable", &mut out);
        out.pop().expect("table record")
    }));
    for v in doc.ucss.iter() {
        records.push(Box::new(move || {
            let mut out = Vec::new();
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
                &mut out,
            );
            out.pop().expect("table entry")
        }));
    }
    records
}

pub(super) fn collect(doc: &CadDocument, out: &mut Vec<Value>) {
    out.extend(records(doc).into_iter().map(|render| render()));
}

/// The block's entities as ACadSharp registers them from a DWG: only the handles the block record itself lists
/// (R2004+ owned-object list, R13-R2000 first/last chain). An entity that merely names the block as its owner but
/// is absent from that list is never added to the block, so ACadSharp does not register it. The native record
/// keeps owner-derived members too; documents that were not read from a DWG have no source list.
pub(super) fn view_entity_handles<'a>(
    doc: &'a CadDocument,
    b: &'a crate::tables::BlockRecord,
) -> std::borrow::Cow<'a, [crate::types::Handle]> {
    use std::borrow::Cow;
    let Some(listed) = doc.block_entity_handles.get(&b.handle) else {
        return Cow::Borrowed(&b.entity_handles);
    };
    let listed: std::collections::HashSet<_> = listed.iter().copied().collect();
    Cow::Owned(
        b.entity_handles
            .iter()
            .copied()
            .filter(|h| listed.contains(h))
            .collect(),
    )
}

pub(super) fn block_flags(doc: &CadDocument, b: &crate::tables::BlockRecord) -> i16 {
    // DWG's has-attributes bit is consumed but never assigned by ACadSharp.
    if source_dwg(doc) {
        return b.source_flags & !2;
    }
    let attributes = !source_dwg(doc) && b.flags.has_attributes;
    (b.source_flags & !31)
        | i16::from(b.flags.anonymous)
        | (i16::from(attributes) << 1)
        | (i16::from(b.flags.is_xref) << 2)
        | (i16::from(b.flags.is_xref_overlay) << 3)
        | (i16::from(b.flags.is_external) << 4)
}

pub(super) fn block_name<'a>(doc: &'a CadDocument, b: &'a crate::tables::BlockRecord) -> &'a str {
    // ACadSharp attaches the parsed BLOCK marker and adopts its name. Native
    // AutoCAD-style anonymous numbering remains intact in the document.
    if source_dwg(doc) {
        if let Some(crate::entities::EntityType::Block(marker)) = doc.get_entity(b.block_entity_handle) {
            let name = marker.source_name.as_deref().unwrap_or(&marker.name);
            if b.handle == doc.header.model_space_block_handle && !name.eq_ignore_ascii_case("*Model_Space") {
                return "*Model_Space";
            }
            if b.handle == doc.header.paper_space_block_handle && !name.eq_ignore_ascii_case("*Paper_Space") {
                return "*Paper_Space";
            }
            if !name.is_empty() {
                return name;
            }
        }
    }
    &b.name
}

fn block_can_scale(doc: &CadDocument, b: &crate::tables::BlockRecord) -> bool {
    if let Some(present) = b.dxf_present {
        present & 2 == 0 || b.scale_uniformly
    } else if source_dwg(doc) {
        doc.version < crate::types::DxfVersion::AC1021 || b.scale_uniformly
    } else {
        !b.scale_uniformly
    }
}

fn block_explodable(doc: &CadDocument, b: &crate::tables::BlockRecord) -> bool {
    if let Some(present) = b.dxf_present {
        present & 1 != 0 && b.explodable
    } else {
        (!source_dwg(doc) || doc.version >= crate::types::DxfVersion::AC1021) && b.explodable
    }
}

fn block_preview(doc: &CadDocument, b: &crate::tables::BlockRecord) -> Value {
    if let Some(present) = b.dxf_present {
        if present & 4 == 0 && b.preview_data.is_empty() {
            return Value::Null;
        }
        // The reference DXF property setter replaces the byte array for each
        // group 310. Preserve the complete native bitmap and project its last
        // source chunk only while the retained bounds still fit the payload.
        if let Some((start, len)) = b.dxf_preview_last_chunk {
            if len == b.preview_data.len() && start <= len {
                return json!(bytes(&b.preview_data[start..]));
            }
        }
    } else if b.preview_data.is_empty()
        && (!source_dwg(doc) || doc.version < crate::types::DxfVersion::AC1015)
    {
        return Value::Null;
    }
    json!(bytes(&b.preview_data))
}

fn block_source(doc: &CadDocument, b: &crate::tables::BlockRecord) -> Value {
    if block_flags(doc, b) & 1 == 0 {
        return Value::Null;
    }
    let Some(data) = doc.object_xdata(b.handle)
        .and_then(|d| d.records().iter().find(|r| r.application_name.eq_ignore_ascii_case("AcDbBlockRepBTag"))) else {
        return Value::Null;
    };
    let Some(h) = data.values.iter().find_map(|v| match v {
        crate::xdata::XDataValue::Handle(h) => Some(*h),
        _ => None,
    }) else {
        return json!("!throws:NullReferenceException");
    };
    if doc.block_records.iter().any(|b| b.handle == h) {
        handle(h)
    } else {
        Value::Null
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
                    // A plain dash/space segment still carries the stored scale;
                    // constructed segments take the constructor default.
                    ("Scale", number(s.source_scale.unwrap_or(1.0))),
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
