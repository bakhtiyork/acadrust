// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, handle, new_document};
use acadrust::entities::EntityType;
use acadrust::objects::{Layout, ObjectType, PlotSettings};
use acadrust::types::{DxfVersion, Handle};
use acadrust::DxfReader;
use serde_json::{json, Value};
use std::io::Cursor;

fn object(dump: &Value, h: Handle) -> &Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["handle"] == handle(h))
        .unwrap()
}

fn layout_handle(doc: &acadrust::CadDocument, name: &str) -> Handle {
    doc.objects
        .iter()
        .find_map(|(handle, value)| match value {
            ObjectType::Layout(layout) if layout.name == name => Some(*handle),
            _ => None,
        })
        .unwrap()
}

fn add_plot_settings(doc: &mut acadrust::CadDocument, mut settings: PlotSettings) -> Handle {
    let handle = Handle::new(0xBEEF);
    settings.handle = handle;
    doc.objects
        .insert(handle, ObjectType::PlotSettings(settings));
    handle
}

#[test]
fn layout_viewports_are_read_from_the_associated_block_only() {
    let mut doc = new_document();
    let model_handle = layout_handle(&doc, "Model");
    let paper_handle = layout_handle(&doc, "Layout1");
    let paper_viewport = doc
        .block_records
        .get("*Paper_Space")
        .unwrap()
        .entity_handles[0];

    // The source Layout viewport list is ignored when the associated block is
    // valid, including when that block contains no viewports.
    {
        let Some(ObjectType::Layout(model)) = doc.objects.get_mut(&model_handle) else {
            panic!("model layout")
        };
        model.viewports = vec![paper_viewport];
    }
    let second_viewport = doc
        .add_entity_to_layout(EntityType::Viewport(Box::default()), "Layout1")
        .unwrap();
    let dump = dump_document(&doc, "", "new");
    assert_eq!(object(&dump, model_handle)["props"]["Viewports"], json!([]));
    assert!(object(&dump, model_handle)["props"]["PaperViewport"].is_null());

    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, paper_handle)["props"]["Viewports"],
        json!([handle(paper_viewport), handle(second_viewport)])
    );
    assert_eq!(
        object(&dump, paper_handle)["props"]["PaperViewport"],
        handle(paper_viewport)
    );

    // A missing associated block produces a null enumerable rather than a
    // fallback to Layout.viewports.
    let missing_handle = Handle::new(0xBEE0);
    let mut missing = Layout::new("Detached");
    missing.handle = missing_handle;
    missing.block_record = Handle::new(0xDEAD);
    missing.viewports = vec![paper_viewport];
    doc.objects
        .insert(missing_handle, ObjectType::Layout(missing));
    let dump = dump_document(&doc, "", "new");
    assert!(object(&dump, missing_handle)["props"]["Viewports"].is_null());
    assert_eq!(
        object(&dump, missing_handle)["props"]["PaperViewport"],
        "!throws:ArgumentNullException"
    );
}

#[test]
fn layout_name_controls_paper_space_and_dwg_resolves_active_viewport_by_type() {
    let mut doc = new_document();
    doc.dwg_source_version = Some(DxfVersion::AC1024);
    let model_handle = layout_handle(&doc, "Model");
    let viewport = doc
        .block_records
        .get("*Paper_Space")
        .unwrap()
        .entity_handles[0];
    {
        let Some(ObjectType::Layout(model)) = doc.objects.get_mut(&model_handle) else {
            panic!("model layout")
        };
        model.name = "model".into();
        model.flags = 0;
        model.viewport = viewport;
    }

    let dump = dump_document(&doc, "", "dwg");
    let props = &object(&dump, model_handle)["props"];
    assert_eq!(props["IsPaperSpace"], false);
    assert_eq!(props["LastActiveViewport"], handle(viewport));

    if let Some(ObjectType::Layout(model)) = doc.objects.get_mut(&model_handle) {
        model.viewport = Handle::new(0xDEAD);
    } else {
        panic!("model layout")
    }
    let dump = dump_document(&doc, "", "dwg");
    let props = &object(&dump, model_handle)["props"];
    assert!(props["LastActiveViewport"].is_null());
}

fn read_objects(objects: &str) -> acadrust::CadDocument {
    let source = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n{objects}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(source.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn dxf_layout_overlapping_plot_codes_keep_native_fields_and_project_csharp_defaults() {
    let doc = read_objects("0\nLAYOUT\n5\nBEE0\n100\nAcDbPlotSettings\n1\nexplicit-page\n70\n11952\n148\n7\n149\n8\n100\nAcDbLayout\n1\nSheet\n70\n0\n331\nDEAD\n");
    let h = Handle::new(0xBEE0);
    let Some(ObjectType::Layout(layout)) = doc.objects.get(&h) else {
        panic!("layout");
    };
    assert_eq!(layout.plot_page_name, "explicit-page");
    assert_eq!(layout.plot_flags.to_bits(), 11952);
    assert_eq!(layout.name, "Sheet");
    assert_eq!(layout.flags, 0);
    assert_eq!(layout.viewport, Handle::new(0xDEAD));
    assert_eq!(layout.paper_image_origin_x, 7.0);
    let dump = dump_document(&doc, "source.dxf", "dxf");
    let props = &object(&dump, h)["props"];
    assert_eq!(props["Flags"], 688);
    assert_eq!(props["PageName"], "none_device");
    assert!(props["LastActiveViewport"].is_null());
    assert_eq!(props["PaperImageOrigin"], json!([0.0, 0.0]));
    assert_eq!(props["PaperImageOriginX"], 7.0);
    assert_eq!(props["PaperImageOriginY"], 8.0);
}

#[test]
fn plot_settings_page_name_distinguishes_omitted_and_explicit_empty_dxf_codes() {
    let doc = read_objects("0\nPLOTSETTINGS\n5\nBEE0\n100\nAcDbPlotSettings\n1\n\n70\n11952\n0\nPLOTSETTINGS\n5\nBEE1\n100\nAcDbPlotSettings\n");
    let dump = dump_document(&doc, "source.dxf", "dxf");
    let explicit = &object(&dump, Handle::new(0xBEE0))["props"];
    let omitted = &object(&dump, Handle::new(0xBEE1))["props"];
    assert_eq!(explicit["PageName"], "");
    assert_eq!(explicit["Flags"], 11952);
    assert_eq!(omitted["PageName"], "none_device");
    assert_eq!(omitted["Flags"], 688);
    let Some(ObjectType::PlotSettings(settings)) = doc.objects.get(&Handle::new(0xBEE0)) else {
        panic!("settings");
    };
    assert!(settings.dxf_code_present(1));
    let mut without_source = settings.clone();
    without_source.dxf_present_codes = None;
    assert_eq!(*settings, without_source);
    without_source.page_name = "changed".into();
    assert_ne!(*settings, without_source);
}

#[test]
fn plot_image_origin_follows_dxf_and_dwg_reader_storage() {
    let mut dxf_doc = new_document();
    let mut dxf_settings = PlotSettings::new("page");
    dxf_settings.paper_image_origin_x = 4.0;
    dxf_settings.paper_image_origin_y = 5.0;
    dxf_settings.dxf_present_codes = Some([0; 6]);
    let dxf_handle = add_plot_settings(&mut dxf_doc, dxf_settings);
    let dxf_dump = dump_document(&dxf_doc, "source.dxf", "dxf");
    let dxf_props = &object(&dxf_dump, dxf_handle)["props"];
    assert_eq!(dxf_props["PaperImageOrigin"], json!([0.0, 0.0]));
    assert_eq!(dxf_props["PaperImageOriginX"], 4.0);
    assert_eq!(dxf_props["PaperImageOriginY"], 5.0);

    let mut dwg_doc = new_document();
    dwg_doc.dwg_source_version = Some(DxfVersion::AC1024);
    let mut dwg_settings = PlotSettings::new("page");
    dwg_settings.paper_image_origin_x = 4.0;
    dwg_settings.paper_image_origin_y = 5.0;
    let dwg_handle = add_plot_settings(&mut dwg_doc, dwg_settings);
    let dwg_dump = dump_document(&dwg_doc, "source.dwg", "dwg");
    let dwg_props = &object(&dwg_dump, dwg_handle)["props"];
    assert_eq!(dwg_props["PaperImageOrigin"], json!([4.0, 5.0]));
    assert_eq!(dwg_props["PaperImageOriginX"], 0.0);
    assert_eq!(dwg_props["PaperImageOriginY"], 0.0);
}

#[test]
fn plot_settings_new_defaults_and_modern_dwg_view_name_match_acadsharp() {
    let mut doc = new_document();
    let mut settings = PlotSettings::new("");
    settings.plot_view_name = "VIEW1".into();
    settings.scale_denominator = 0.0;
    let handle = add_plot_settings(&mut doc, settings);
    let dump = dump_document(&doc, "", "new");
    let props = &object(&dump, handle)["props"];
    assert_eq!(props["Flags"], 688);
    assert_eq!(props["PageName"], "none_device");
    assert_eq!(props["PaperSize"], "ISO_A4_(210.00_x_297.00_MM)");
    assert_eq!(props["PaperUnits"], 1);
    assert_eq!(props["PaperWidth"], 297.0);
    assert_eq!(props["PaperHeight"], 210.0);
    assert_eq!(props["PlotType"], 1);
    assert_eq!(props["ShadePlotResolutionMode"], 0);
    assert_eq!(props["PlotViewName"], "VIEW1");
    assert!(props["StyleSheet"].is_null());
    assert!(props["SystemPrinterName"].is_null());
    assert_eq!(props["PrintScale"], "Infinity");

    doc.dwg_source_version = Some(DxfVersion::AC1024);
    let modern_dump = dump_document(&doc, "source.dwg", "dwg");
    let modern = &object(&modern_dump, handle)["props"];
    assert!(modern["PlotViewName"].is_null());
}

#[test]
fn layout_ucs_references_require_ucs_objects() {
    let mut doc = new_document();
    let h = layout_handle(&doc, "Layout1");
    let mut ucs = acadrust::tables::Ucs::new("Survey");
    ucs.handle = Handle::new(0xBEEA);
    let ucs_handle = ucs.handle;
    doc.ucss.add(ucs).unwrap();
    let wrong_type = doc.layers.get("0").unwrap().handle;
    if let Some(ObjectType::Layout(layout)) = doc.objects.get_mut(&h) {
        layout.base_ucs = ucs_handle;
        layout.named_ucs = wrong_type;
    }
    let dump = dump_document(&doc, "", "new");
    let p = &object(&dump, h)["props"];
    assert_eq!(p["BaseUCS"], handle(ucs_handle));
    assert!(p["UCS"].is_null());
    if let Some(ObjectType::Layout(layout)) = doc.objects.get_mut(&h) {
        layout.base_ucs = Handle::NULL;
        layout.named_ucs = ucs_handle;
    }
    let dump = dump_document(&doc, "", "new");
    let p = &object(&dump, h)["props"];
    assert!(p["BaseUCS"].is_null());
    assert_eq!(p["UCS"], handle(ucs_handle));
}

#[test]
fn new_layout_and_legacy_dwg_plot_defaults_do_not_change_native_state() {
    let mut doc = new_document();
    let h = layout_handle(&doc, "Layout1");
    let dump = dump_document(&doc, "", "new");
    let p = &object(&dump, h)["props"];
    assert_eq!(p["MinLimits"], json!([-20.0, -7.5]));
    assert_eq!(p["MaxLimits"], json!([277.0, 202.5]));
    assert_eq!(p["MinExtents"], json!([25.7, 19.5, 0.0]));
    assert_eq!(p["MaxExtents"], json!([231.3, 175.5, 0.0]));
    assert_eq!(p["LayoutFlags"], 1);
    assert_eq!(p["ShadePlotDPI"], 300);
    assert_eq!(p["ShadePlotIDHandle"], 0);
    if let Some(ObjectType::Layout(layout)) = doc.objects.get_mut(&h) {
        layout.shade_plot_dpi = 0;
        layout.plot_view_name = "Old view".into();
    }
    doc.dwg_source_version = Some(DxfVersion::AC1015);
    let dump = dump_document(&doc, "", "dwg");
    let p = &object(&dump, h)["props"];
    assert_eq!(p["ShadePlotDPI"], 300);
    assert_eq!(p["PlotViewName"], "Old view");
    let Some(ObjectType::Layout(layout)) = doc.objects.get(&h) else {
        panic!("layout");
    };
    assert_eq!(layout.shade_plot_dpi, 0);
}
