// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, handle, new_document};
use acadrust::entities::{GridFlags, Line, Viewport, ViewportStatusFlags};
use acadrust::objects::{Dictionary, ObjectType, Scale, XRecord, XRecordEntry};
use acadrust::types::{Color, DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, EntityType};
use serde_json::{json, Value};
use std::io::Cursor;

fn viewport(doc: &CadDocument) -> &Viewport {
    doc.entities()
        .find_map(|e| match e {
            EntityType::Viewport(v) => Some(v.as_ref()),
            _ => None,
        })
        .unwrap()
}

fn properties(doc: &CadDocument, h: Handle) -> Value {
    let dump = dump_document(doc, "synthetic", "dxf");
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == handle(h) && o["type"] == "Viewport")
        .unwrap()["props"]
        .clone()
}

fn dxf(codes: &str) -> CadDocument {
    let text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nVIEWPORT\n5\n700\n100\nAcDbEntity\n8\n0\n100\nAcDbViewport\n{codes}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn default_paper_viewport_uses_csharp_constructor_and_ieee_getters() {
    let doc = new_document();
    let v = viewport(&doc);
    let p = properties(&doc, v.common.handle);
    assert_eq!(p["Id"], 1);
    assert_eq!(p["RepresentsPaper"], true);
    assert_eq!(p["ViewHeight"], 0.0);
    assert_eq!(p["ViewWidth"], 0.0);
    assert_eq!(p["ScaleFactor"], "Infinity");
    assert_eq!(p["Status"], 0);
    assert_eq!(p["StyleSheetName"], Value::Null);
    assert_eq!(p["DisplayUcsIcon"], false);
    assert_eq!(p["UseDefaultLighting"], false);
    for name in ["Boundary", "Scale", "VisualStyle"] {
        assert_eq!(p[name], Value::Null);
    }
    assert_eq!(p["FrozenLayers"], json!([]));
    for name in [
        "CircleZoomPercent",
        "MajorGridLineFrequency",
        "DefaultLightingType",
        "LensLength",
    ] {
        assert_eq!(p[name].as_f64(), Some(0.0), "{name}");
    }
    // Native constructors retain their useful drafting defaults.
    assert_eq!(Viewport::new().view_height, 210.0);
    assert!(Viewport::new().default_lighting);
}

#[test]
fn viewport_getters_use_block_order_and_model_to_paper_ratio() {
    let mut doc = CadDocument::new();
    let mut v = Viewport::with_size(Vector3::ZERO, 20.0, 10.0);
    v.id = 77;
    v.dxf_status = Some(-1);
    v.view_height = 40.0;
    v.status = ViewportStatusFlags::from_bits(0x20);
    v.grid_flags = GridFlags::from_bits(11);
    v.ucs_at_origin = false;
    v.ucs_icon_visible = true;
    let h = doc
        .add_entity_to_layout(EntityType::Viewport(Box::new(v)), "Layout1")
        .unwrap();
    let p = properties(&doc, h);
    assert_eq!(p["Id"], 1);
    assert_eq!(p["ActiveStatus"], 1);
    assert_eq!(p["RepresentsPaper"], true);
    assert_eq!(p["ScaleFactor"], 0.25);
    assert_eq!(p["ViewWidth"], 80.0);
    assert_eq!(p["Width"], 20.0);
    assert_eq!(p["Status"], 0x20 | (11 << 18));
    assert_eq!(p["DisplayUcsIcon"], false);
    let second = doc
        .add_entity_to_layout(EntityType::Viewport(Box::default()), "Layout1")
        .unwrap();
    let p = properties(&doc, second);
    assert_eq!(p["Id"], 2);
    assert_eq!(p["RepresentsPaper"], false);
    if let Some(EntityType::Viewport(v)) = doc.get_entity_mut(h) {
        v.height = 0.0;
        v.view_height = 0.0;
    }
    let p = properties(&doc, h);
    assert_eq!(p["ViewWidth"], "NaN");
    assert_eq!(p["ScaleFactor"], "NaN");
}

#[test]
fn dxf_omissions_project_defaults_without_overwriting_native_values() {
    let doc = dxf("40\n20\n41\n10\n");
    let v = viewport(&doc);
    assert_eq!(v.lens_length, 50.0);
    assert_eq!(v.view_height, 210.0);
    assert!(v.default_lighting);
    let p = properties(&doc, v.common.handle);
    assert_eq!(p["LensLength"], 0.0);
    assert_eq!(p["ViewHeight"], 0.0);
    assert_eq!(p["GridSpacing"], json!([0.0, 0.0]));
    assert_eq!(p["SnapSpacing"], json!([0.0, 0.0]));
    assert_eq!(p["UseDefaultLighting"], false);
    assert_eq!(p["DefaultLightingType"], 0);
    assert_eq!(p["MajorGridLineFrequency"], 0);
    assert_eq!(p["Status"], 0);
    assert_eq!(p["StyleSheetName"], Value::Null);
    let doc = dxf("40\n20\n41\n10\n42\n50\n45\n210\n15\n10\n25\n10\n14\n10\n24\n10\n292\n1\n282\n1\n61\n5\n90\n32768\n1\n\n");
    let p = properties(&doc, viewport(&doc).common.handle);
    assert_eq!(p["LensLength"], 50.0);
    assert_eq!(p["ViewHeight"], 210.0);
    assert_eq!(p["GridSpacing"], json!([10.0, 10.0]));
    assert_eq!(p["UseDefaultLighting"], true);
    assert_eq!(p["DefaultLightingType"], 1);
    assert_eq!(p["MajorGridLineFrequency"], 5);
    assert_eq!(p["Status"], 32768);
    assert_eq!(p["StyleSheetName"], "");
}

#[test]
fn dxf_retains_viewport_specific_fields_and_raw_status() {
    let doc = dxf("40\n20\n41\n10\n90\n4194336\n68\n2\n69\n99\n74\n1\n79\n4\n1\ncustom.ctb\n421\n1193046\n63\n7\n345\n801\n346\n802\n348\n803\n332\n804\n333\n805\n361\n806\n141\n0.6\n170\n3\n");
    let v = viewport(&doc);
    assert!(v.ucs_at_origin);
    assert_eq!(v.ucs_ortho_type, 4);
    assert_eq!(v.style_sheet, "custom.ctb");
    assert_eq!(v.ambient_color, Color::from_rgb(0x12, 0x34, 0x56));
    assert_eq!(
        [
            v.ucs_handle,
            v.base_ucs_handle,
            v.visual_style_handle,
            v.background_handle,
            v.shade_plot_handle,
            v.sun_handle
        ],
        [0x801, 0x802, 0x803, 0x804, 0x805, 0x806].map(Handle::new)
    );
    assert!(v.status.is_on); // Native visibility uses group 68.
    let p = properties(&doc, v.common.handle);
    assert_eq!(p["Status"], 4194336); // C# exposes unnormalized group 90, including high bits.
    assert_eq!(p["ActiveStatus"], 1); // C# ignores group 68.
    assert_eq!(p["DisplayUcsIcon"], true);
    assert_eq!(p["UcsOrthographicType"], 4);
    assert_eq!(p["StyleSheetName"], "custom.ctb");
    assert_eq!(p["AmbientLightColor"]["TrueColor"], 0x123456);
    assert_eq!(p["Brightness"], 0.6);
    assert_eq!(p["ShadePlotMode"], 3);
    assert_eq!(p["VisualStyle"], Value::Null);
    let doc = dxf("63\n7\n");
    assert_eq!(viewport(&doc).ambient_color, Color::from_index(7));
}

#[test]
fn viewport_references_resolve_types_preserve_order_and_stop_at_first_scale_entry() {
    let mut doc = CadDocument::new();
    let boundary = doc
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        )))
        .unwrap();
    let layer = doc.layers.get("0").unwrap().handle;
    let dictionary_handle = Handle::new(0x900);
    let record_handle = Handle::new(0x901);
    let scale_handle = Handle::new(0x902);
    let mut scale = Scale::new("Custom", 1.0, 2.0);
    scale.handle = scale_handle;
    doc.objects.insert(scale_handle, ObjectType::Scale(scale));
    let mut record = XRecord::new();
    record.handle = record_handle;
    record.entries.push(XRecordEntry::handle(340, scale_handle));
    doc.objects
        .insert(record_handle, ObjectType::XRecord(record));
    let mut dictionary = Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary
        .entries
        .push(("ASDK_XREC_ANNOTATION_SCALE_INFO".into(), record_handle));
    doc.objects
        .insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    let mut v = Viewport::new();
    v.common.xdictionary_handle = Some(dictionary_handle);
    v.clip_boundary_handle = boundary;
    v.frozen_layers = vec![layer, layer, boundary, Handle::new(0xdead)];
    let h = doc
        .add_entity_to_layout(EntityType::Viewport(Box::new(v)), "Layout1")
        .unwrap();
    let p = properties(&doc, h);
    assert_eq!(p["Boundary"], handle(boundary));
    assert_eq!(p["FrozenLayers"], json!([handle(layer)]));
    assert_eq!(p["Scale"], handle(scale_handle));
    if let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&record_handle) {
        record
            .entries
            .insert(0, XRecordEntry::handle(340, boundary));
    }
    if let Some(EntityType::Viewport(v)) = doc.get_entity_mut(h) {
        v.clip_boundary_handle = scale_handle;
    }
    let p = properties(&doc, h);
    assert_eq!(p["Boundary"], Value::Null);
    assert_eq!(p["Scale"], Value::Null);
}

#[test]
fn legacy_dwg_projects_unread_fields_without_discarding_mview_geometry() {
    let mut doc = CadDocument::new();
    doc.version = DxfVersion::AC1014;
    let mut v = Viewport::with_size(Vector3::ZERO, 20.0, 10.0);
    v.view_height = 40.0;
    v.view_center = Vector3::new(3.0, 4.0, 0.0);
    doc.add_entity_to_layout(EntityType::Viewport(Box::new(v)), "Layout1")
        .unwrap();
    let bytes = DwgWriter::write_to_vec(&doc).unwrap();
    let restored = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    let v = viewport(&restored);
    assert_eq!(v.view_height, 40.0);
    assert_eq!(v.view_center, Vector3::new(3.0, 4.0, 0.0));
    let p = properties(&restored, v.common.handle);
    assert_eq!(p["ViewHeight"], 0.0);
    assert_eq!(p["ViewCenter"], json!([0.0, 0.0]));
    assert_eq!(p["UcsXAxis"], json!([1.0, 0.0, 0.0]));
    assert_eq!(p["UcsYAxis"], json!([0.0, 1.0, 0.0]));
    assert_eq!(p["Status"], 0);
    assert_eq!(p["UseDefaultLighting"], false);
    assert_eq!(p["MajorGridLineFrequency"], 0);
    assert_eq!(p["ScaleFactor"], "Infinity");
}

#[test]
fn viewport_source_presence_does_not_change_native_equality() {
    let doc = dxf("40\n20\n41\n10\n90\n32\n");
    let v = viewport(&doc);
    let mut without_source = v.clone();
    without_source.source = None;
    assert_eq!(*v, without_source);
    without_source.width += 1.0;
    assert_ne!(*v, without_source);
}

#[test]
fn partial_dxf_vectors_keep_independent_component_defaults() {
    let doc = dxf("25\n2\n14\n3\n22\n4\n16\n5\n121\n6\n");
    let p = properties(&doc, viewport(&doc).common.handle);
    assert_eq!(p["GridSpacing"], json!([0.0, 2.0]));
    assert_eq!(p["SnapSpacing"], json!([3.0, 0.0]));
    assert_eq!(p["ViewCenter"], json!([0.0, 4.0]));
    assert_eq!(p["ViewDirection"], json!([5.0, 0.0, 1.0]));
    assert_eq!(viewport(&doc).view_direction, Vector3::new(5.0, 0.0, 1.0));
    assert_eq!(viewport(&doc).view_center.y, 4.0);
    assert_eq!(p["UcsXAxis"], json!([1.0, 6.0, 0.0]));
}

#[test]
fn dwg_version_controls_lighting_grid_and_full_status_projection() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021] {
        let mut doc = CadDocument::new();
        doc.version = version;
        let mut v = Viewport::new();
        v.grid_major = 7;
        v.grid_flags = GridFlags::from_bits(13);
        v.status = ViewportStatusFlags::from_bits(0x8020);
        v.default_lighting = true;
        v.default_lighting_type = 1;
        v.brightness = 0.75;
        doc.add_entity_to_layout(EntityType::Viewport(Box::new(v)), "Layout1")
            .unwrap();
        let restored = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        let v = viewport(&restored);
        assert!(v.default_lighting);
        let modern = version >= DxfVersion::AC1021;
        // The writer encodes grid behavior only in R2007+ status words.
        let stored_grid = if modern { 13 } else { 0 };
        assert_eq!(v.grid_flags.to_bits(), stored_grid);
        let p = properties(&restored, v.common.handle);
        assert_eq!(p["UseDefaultLighting"], modern);
        assert_eq!(p["DefaultLightingType"], if modern { 1 } else { 0 });
        assert_eq!(p["MajorGridLineFrequency"], if modern { 7 } else { 0 });
        assert_eq!(p["Brightness"], if modern { 0.75 } else { 0.0 });
        assert_eq!(p["Status"], 0x8020 | (i32::from(stored_grid) << 18));
    }
}
