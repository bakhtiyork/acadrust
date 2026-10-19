// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, dump_reader_document, handle, new_document};
use acadrust::objects::{ObjectType, VisualStyle};
use acadrust::tables::{VPort, View};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, DxfReader};
use serde_json::Value;
use std::io::Cursor;

fn dxf(tables: &str) -> CadDocument {
    let input = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n0\nEOF\n"
    );
    DxfReader::from_reader(Cursor::new(input.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn table_object<'a>(dump: &'a Value, ty: &str, h: Handle) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == ty && object["handle"] == handle(h))
        .unwrap_or_else(|| panic!("missing {ty} {h:?}"))
}

#[test]
fn new_document_vport_uses_acadsharp_defaults() {
    let doc = new_document();
    let h = doc.vports.get("*Active").unwrap().handle;
    let dump = dump_document(&doc, "", "new");
    let props = &table_object(&dump, "VPort", h)["props"];

    assert_eq!(props["CircleZoomPercent"], 1000);
    assert_eq!(props["GridFlags"], 3);
    assert_eq!(props["ShowGrid"], true);
    assert_eq!(props["UcsIconDisplay"], 3);
    assert_eq!(props["ViewMode"], 16);
}

#[test]
fn constructed_view_and_vport_pack_modes_axes_and_registered_styles() {
    let mut doc = CadDocument::new();

    let style_handle = Handle::new(0x700);
    let mut style = VisualStyle::new();
    style.handle = style_handle;
    doc.objects
        .insert(style_handle, ObjectType::VisualStyle(style));

    let view_handle = Handle::new(0x701);
    let mut view = View::new("SyntheticView");
    view.handle = view_handle;
    view.perspective = true;
    view.front_clipping = true;
    view.back_clipping = true;
    view.front_clip_at_eye = true;
    view.ucs_x_axis = Vector3::new(2.0, 3.0, 4.0);
    view.ucs_y_axis = Vector3::new(5.0, 6.0, 7.0);
    view.xref_dependent = true;
    view.xref_resolved = true;
    view.xref_reference = true;
    view.visual_style_handle = style_handle;
    doc.views.add(view).unwrap();

    let vport_handle = Handle::new(0x702);
    let mut vport = VPort::new("SyntheticVPort");
    vport.handle = vport_handle;
    vport.perspective = true;
    vport.front_clipping = true;
    vport.back_clipping = true;
    vport.ucsfollow = true;
    vport.front_clip_at_eye = true;
    vport.ucsicon_lower = true;
    vport.ucsicon_origin = false;
    vport.circle_zoom = 432;
    vport.grid_on = true;
    vport.grid_flags = acadrust::entities::GridFlags::from_bits(5);
    vport.xref_dependent = true;
    vport.xref_resolved = true;
    vport.xref_reference = true;
    vport.visual_style_handle = style_handle;
    doc.vports.add_allow_duplicate(vport);

    let dump = dump_document(&doc, "synthetic", "dxf");
    let view = &table_object(&dump, "View", view_handle)["props"];
    assert_eq!(view["Flags"], 112);
    assert_eq!(view["UcsXAxis"], serde_json::json!([2.0, 3.0, 4.0]));
    assert_eq!(view["UcsYAxis"], serde_json::json!([5.0, 6.0, 7.0]));
    assert_eq!(view["ViewMode"], 23);
    assert_eq!(view["VisualStyle"], "#700");

    let vport = &table_object(&dump, "VPort", vport_handle)["props"];
    assert_eq!(vport["Flags"], 112);
    assert_eq!(vport["CircleZoomPercent"], 432);
    assert_eq!(vport["GridFlags"], 5);
    assert_eq!(vport["ShowGrid"], true);
    assert_eq!(vport["UcsIconDisplay"], 1);
    assert_eq!(vport["ViewMode"], 31);
    assert_eq!(vport["VisualStyle"], "#700");
}

#[test]
fn omitted_dxf_vport_fields_use_reference_defaults_and_explicit_zeroes_survive() {
    let omitted = dxf("0\nTABLE\n2\nVPORT\n0\nVPORT\n5\nF001\n2\nOmitted\n0\nENDTAB\n");
    let omitted_handle = omitted.vports.get("omitted").unwrap().handle;
    let dump = dump_reader_document(&omitted, "omitted.dxf", "dxf");
    let omitted_props = &table_object(&dump, "VPort", omitted_handle)["props"];
    assert_eq!(omitted_props["CircleZoomPercent"], 1000);
    assert_eq!(omitted_props["GridFlags"], 3);
    assert_eq!(omitted_props["ShowGrid"], true);
    assert_eq!(omitted_props["UcsIconDisplay"], 3);
    assert_eq!(omitted_props["ViewMode"], 16);

    let explicit = dxf(
        "0\nTABLE\n2\nVPORT\n0\nVPORT\n5\nF002\n2\nExplicit\n60\n0\n71\n0\n72\n0\n74\n0\n76\n0\n0\nENDTAB\n",
    );
    let explicit_handle = explicit.vports.get("explicit").unwrap().handle;
    let dump = dump_reader_document(&explicit, "explicit.dxf", "dxf");
    let explicit_props = &table_object(&dump, "VPort", explicit_handle)["props"];
    assert_eq!(explicit_props["CircleZoomPercent"], 0);
    assert_eq!(explicit_props["GridFlags"], 0);
    assert_eq!(explicit_props["ShowGrid"], false);
    assert_eq!(explicit_props["UcsIconDisplay"], 0);
    assert_eq!(explicit_props["ViewMode"], 0);
}

#[test]
fn partial_dxf_view_axis_keeps_present_component_and_zeroes_omitted_components() {
    let doc = dxf("0\nTABLE\n2\nVIEW\n0\nVIEW\n5\nF003\n2\nPartial\n121\n9.0\n0\nENDTAB\n");
    let handle = doc.views.get("partial").unwrap().handle;
    let dump = dump_reader_document(&doc, "partial.dxf", "dxf");
    let props = &table_object(&dump, "View", handle)["props"];

    assert_eq!(props["UcsXAxis"], serde_json::json!([0.0, 9.0, 0.0]));
    assert_eq!(props["UcsYAxis"], serde_json::json!([0.0, 0.0, 0.0]));
}

#[test]
fn source_provenance_does_not_change_table_entry_equality() {
    let mut view = View::new("view");
    let same_view = view.clone();
    view.source_field_mask = u16::MAX;
    assert_eq!(view, same_view);

    let mut vport = VPort::new("vport");
    let same_vport = vport.clone();
    vport.source_field_mask = u16::MAX;
    assert_eq!(vport, same_vport);
}
