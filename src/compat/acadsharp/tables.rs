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
                ("Material", json!(optional_handle(Some(v.material)))),
                ("Name", json!(v.name)),
                ("PlotFlag", json!(v.is_plottable)),
                ("PlotStyleName", json!(v.plotstyle_handle.value())),
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
                ("Alignment", character(v.alignment)),
                ("Description", json!(v.description)),
                ("Segments", line_type_segments(v)),
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
                ("PatternLength", json!(number(v.pattern_length))),
            ],
            out,
        );
    }
    table(doc, &doc.text_styles, "TextStylesTable", out);
    for v in doc.text_styles.iter() {
        entry(
            doc,
            v.handle,
            "TextStyle",
            doc.text_styles.handle(),
            vec![
                ("BigFontFilename", json!(v.big_font_file)),
                ("Filename", json!(v.font_file)),
                ("Height", json!(number(v.height))),
                ("IsShapeFile", json!(v.is_shape_file)),
                ("LastHeight", json!(number(v.last_height))),
                (
                    "MirrorFlag",
                    json!((v.flags.backward as i16 * 2) | (v.flags.upside_down as i16 * 4)),
                ),
                ("Name", json!(v.name)),
                ("ObliqueAngle", json!(number(v.oblique_angle))),
                ("TrueType", json!(v.true_type_font_flags)),
                ("Width", json!(number(v.width_factor))),
            ],
            out,
        );
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
            vec![
                (
                    "Flags",
                    json!(
                        (v.xref_dependent as i16) * 16
                            | (v.xref_resolved as i16) * 32
                            | (v.xref_reference as i16) * 64
                    ),
                ),
                ("AlternateDimensioningSuffix", json!(v.dimapost)),
                ("AlternateUnitDecimalPlaces", json!(v.dimaltd)),
                ("AlternateUnitDimensioning", json!(v.dimalt)),
                ("AlternateUnitFormat", json!(v.dimaltu)),
                ("AlternateUnitRounding", json!(number(v.dimaltrnd))),
                ("AlternateUnitScaleFactor", json!(number(v.dimaltf))),
                ("AlternateUnitToleranceDecimalPlaces", json!(v.dimalttd)),
                ("AlternateUnitToleranceZeroHandling", json!(v.dimalttz)),
                ("AlternateUnitZeroHandling", json!(v.dimaltz)),
                ("AngularUnit", json!(v.dimaunit)),
                ("AngularZeroHandling", json!(v.dimazin)),
                ("ArcLengthSymbolPosition", json!(v.dimarcsym)),
                ("ArrowSize", json!(number(v.dimasz))),
                ("CenterMarkSize", json!(number(v.dimcen))),
                ("CursorUpdate", json!(v.dimupt)),
                ("DecimalPlaces", json!(v.dimdec)),
                ("DecimalSeparator", json!(v.dimdsep)),
                ("DimensionTextArrowFit", json!(v.dimatfit)),
                (
                    "ExtensionLineColor",
                    json!(color(
                        v.dimclre_true_color
                            .unwrap_or_else(|| Color::from_index(v.dimclre))
                    )),
                ),
                ("ExtensionLineExtension", json!(number(v.dimexe))),
                ("ExtensionLineOffset", json!(number(v.dimexo))),
                ("ExtensionLineWeight", json!(v.dimlwe)),
                ("FixedExtensionLineLength", json!(number(v.dimfxl))),
                ("FractionFormat", json!(v.dimfrac)),
                ("GenerateTolerances", json!(v.dimtol)),
                ("IsExtensionLineLengthFixed", json!(v.dimfxlon)),
                (
                    "JoggedRadiusDimensionTransverseSegmentAngle",
                    json!(number(v.dimjogang)),
                ),
                ("LimitsGeneration", json!(v.dimlim)),
                ("LinearScaleFactor", json!(number(v.dimlfac))),
                ("LinearUnitFormat", json!(v.dimlunit)),
                ("MinusTolerance", json!(number(v.dimtm))),
                ("Name", json!(v.name)),
                ("PlusTolerance", json!(number(v.dimtp))),
                ("PostFix", json!(v.dimpost)),
                ("Rounding", json!(number(v.dimrnd))),
                ("ScaleFactor", json!(number(v.dimscale))),
                ("SeparateArrowBlocks", json!(v.dimsah)),
                ("SuppressFirstDimensionLine", json!(v.dimsd1)),
                ("SuppressFirstExtensionLine", json!(v.dimse1)),
                ("SuppressOutsideExtensions", json!(v.dimsoxd)),
                ("SuppressSecondDimensionLine", json!(v.dimsd2)),
                ("SuppressSecondExtensionLine", json!(v.dimse2)),
                ("TextBackgroundFillMode", json!(v.dimtfill)),
                (
                    "TextColor",
                    json!(color(
                        v.dimclrt_true_color
                            .unwrap_or_else(|| Color::from_index(v.dimclrt))
                    )),
                ),
                ("TextDirection", json!(v.dimtxtdirection as i16)),
                ("TextHeight", json!(number(v.dimtxt))),
                ("TextHorizontalAlignment", json!(v.dimjust)),
                ("TextInsideExtensions", json!(v.dimtix)),
                ("TextInsideHorizontal", json!(v.dimtih)),
                ("TextMovement", json!(v.dimtmove)),
                ("TextOutsideExtensions", json!(v.dimtofl)),
                ("TextOutsideHorizontal", json!(v.dimtoh)),
                ("TextVerticalAlignment", json!(v.dimtad)),
                ("TextVerticalPosition", json!(number(v.dimtvp))),
                ("TickSize", json!(number(v.dimtsz))),
                ("ToleranceAlignment", json!(v.dimtolj)),
                ("ToleranceDecimalPlaces", json!(v.dimtdec)),
                ("ToleranceScaleFactor", json!(number(v.dimtfac))),
                ("ToleranceZeroHandling", json!(v.dimtzin)),
                ("ZeroHandling", json!(v.dimzin)),
            ],
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
            vec![("Name", json!(v.name))],
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
            ],
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
            ],
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
        let mut p = metadata("Block");
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
            props("BlockEnd", metadata("BlockEnd")),
        ));
    }
}

fn line_type_segments(v: &crate::tables::LineType) -> Value {
    use crate::tables::LineTypeComplexContent;
    Value::Array(
        v.elements
            .iter()
            .map(|s| {
                let mut p = vec![("Length", number(s.length))];
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
                        ("Style", optional_handle(Some(c.style_handle))),
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
    let xdata = if let Some(data) = doc.object_xdata(h) {
        super::entities::extended_data(doc, data)
    } else if doc.eed_by_handle.get(&h).is_some_and(|v| !v.is_empty()) {
        json!(UNMAPPED)
    } else {
        json!([])
    };
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
