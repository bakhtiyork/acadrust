// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-12 (gate G5-RR and the sample_V cells): the round-trip records of AutoCAD's R14 down-save that the fork did not write: the
//! dimension style's `ACADR14ROUNDTRIP`, the UCS and render mode groups of a viewport, a VPORT and a view, the R14 spelling of the
//! arrowhead name and the VX records of the active paper space. Synthetic documents only; each test fails with its fix reverted.
//! Evidence: AutoCAD's own files of one drawing (sample_AC1014.dwg to sample_AC1032.dwg; the C# dumps under
//! Tests/CadCoreTests/Fixtures/dumps).

use std::io::Cursor;

use acadrust::entities::{EntityType, Viewport, ViewportRenderMode};
use acadrust::tables::{BlockRecord, DimStyle, View, VPort};
use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

/// The groups of the round-trip records the ACadSharp view shows (it reads the records as objects): `(owner handle, entries)` as
/// `code=value` text, for the records whose entries contain `needle`.
fn records_with(document: &CadDocument, needle: &str) -> Vec<String> {
    let dump = acadrust::compat::acadsharp::dump_document(document, "", "new");
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|object| object["type"] == "XRecord")
        .map(|object| {
            object["props"]["Entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| format!("{}={}", entry["Code"], entry["Value"].as_str().map_or_else(|| entry["Value"].to_string(), str::to_string)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|text| text.contains(needle))
        .collect()
}

fn style<'a>(document: &'a CadDocument, name: &str) -> &'a DimStyle {
    document.dim_styles.iter().find(|style| style.name == name).unwrap_or_else(|| panic!("no dimension style {name}"))
}

/// A drawing of R2018 (handles assigned) with the dimension styles of the sample: `Comma` (DIMDSEP 44), `Arrow` (DIMBLK the block
/// `_BoxBlank`, DIMLWD 15), `Plain` (the defaults).
fn dim_style_document() -> CadDocument {
    let mut document = CadDocument::new();
    document.block_records.add(BlockRecord::new("_BoxBlank")).unwrap();
    for name in ["Comma", "Arrow", "Plain"] {
        document.dim_styles.add(DimStyle::new(name)).unwrap();
    }
    let mut document = write_and_read(&document, DxfVersion::AC1032);
    let block = document.block_records.get("_BoxBlank").unwrap().handle;
    for style in document.dim_styles.iter_mut() {
        match style.name.as_str() {
            "Comma" => style.dimdsep = 44,
            "Arrow" => {
                style.dimblk = block;
                style.dimlwd = 15;
            }
            _ => {}
        }
    }
    document
}

/// R14's dimension style record has no DIMDSEP, DIMBLK handle and DIMLWD; AutoCAD's R14 file keeps them in
/// `ACAD_XREC_ROUNDTRIP {ACADR14ROUNDTRIP DSTYLE {278 / 342 / 371}}` of the style (sample_AC1014.dwg: ISO-25 and Beschriftung 278 = 44,
/// custom_style_ltypes 342 = #C80 and 371 = 15; a style with the defaults has none). The reader restores the values and takes the group
/// out of the model; the view shows R14's defaults and the record; an R2000 file holds the values itself.
#[test]
fn an_r14_dimension_style_keeps_dimdsep_dimblk_and_dimlwd_in_the_record_autocad_states() {
    let source = dim_style_document();
    let block = source.block_records.get("_BoxBlank").unwrap().handle;
    let r14 = write_and_read(&source, DxfVersion::AC1014);
    assert_eq!(style(&r14, "Comma").dimdsep, 44);
    let arrow = style(&r14, "Arrow");
    assert_eq!((arrow.dimblk, arrow.dimlwd, arrow.dimdsep), (block, 15, 46));
    assert_eq!(arrow.dimblk_name, "BOXBLANK", "R14 names the arrowhead block BOXBLANK");
    let plain = style(&r14, "Plain");
    assert_eq!((plain.dimdsep, plain.dimlwd, plain.dimblk), (46, -2, Handle::NULL));
    // the view: ACadSharp reads the defaults and the record as an object, one record for each of the two styles
    let comma = style(&r14, "Comma").handle;
    assert!(r14.extension_dictionary_handle(comma).is_some());
    // consumed on read: the record is out of the native model (the view puts it back)
    for name in ["Comma", "Arrow"] {
        assert!(r14.xrecord(style(&r14, name).handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{name}: the record is out of the model");
    }
    let records = records_with(&r14, "ACADR14ROUNDTRIP");
    let hex = format!("{:X}", block.value());
    let mut expected = vec![
        "102=ACADR14ROUNDTRIP 1000=DSTYLE 1002={ 1070=278 70=44 1002=}".to_string(),
        format!("102=ACADR14ROUNDTRIP 1000=DSTYLE 1002={{ 1070=342 340=#{hex} 1070=371 70=15 1002=}}"),
    ];
    expected.sort();
    let mut found = records.iter().map(|text| text[text.find("102=ACADR14ROUNDTRIP").unwrap()..].to_string()).collect::<Vec<_>>();
    found.sort();
    assert_eq!(found, expected);
    let viewed = acadrust::compat::acadsharp::dump_document(&r14, "", "new");
    let shown = viewed["objects"].as_array().unwrap().iter().find(|object| object["type"] == "DimensionStyle" && object["props"]["Name"] == "COMMA").unwrap();
    assert_eq!(shown["props"]["DecimalSeparator"], 46, "ACadSharp reads R14's default");
    // an R2000 file holds the values itself and no record
    let r15 = write_and_read(&r14, DxfVersion::AC1015);
    assert_eq!(style(&r15, "Comma").dimdsep, 44);
    let arrow = style(&r15, "Arrow");
    assert_eq!((arrow.dimblk, arrow.dimlwd), (block, 15));
    assert!(records_with(&r15, "ACADR14ROUNDTRIP").is_empty());
}

/// An edit after the read wins: a value set back to the default drops its variable, and a group no variable is left in goes.
#[test]
fn an_r14_dimension_style_record_follows_an_edit() {
    let r14 = write_and_read(&dim_style_document(), DxfVersion::AC1014);
    let mut edited = r14.clone();
    for style in edited.dim_styles.iter_mut() {
        match style.name.as_str() {
            "Comma" => style.dimdsep = 46,
            "Arrow" => style.dimlwd = -2,
            _ => {}
        }
    }
    let again = write_and_read(&edited, DxfVersion::AC1014);
    assert_eq!(style(&again, "Comma").dimdsep, 46);
    assert_eq!((style(&again, "Arrow").dimlwd, style(&again, "Arrow").dimblk), (-2, edited.block_records.get("_BoxBlank").unwrap().handle));
    let records = records_with(&again, "ACADR14ROUNDTRIP");
    assert_eq!(records.len(), 1, "only the arrowhead is left: {records:?}");
    assert!(records[0].contains("1070=342") && !records[0].contains("1070=371") && !records[0].contains("1070=278"), "{}", records[0]);
}

/// R14 spells the arrowhead block of the header as a name: `BOXBLANK` for `_BoxBlank` (sample_AC1014.dwg, G5-RC-S012).
#[test]
fn the_header_arrowhead_name_is_spelled_as_r14_does() {
    let mut source = dim_style_document();
    source.header.dim_arrow_block = "_BoxBlank".to_string();
    assert_eq!(write_and_read(&source, DxfVersion::AC1014).header.dim_arrow_block, "BOXBLANK");
    // a name no sample shows is stated as it was
    source.header.dim_arrow_block = "_Dot".to_string();
    assert_eq!(write_and_read(&source, DxfVersion::AC1014).header.dim_arrow_block, "_Dot");
}

fn the_viewport(document: &CadDocument) -> Viewport {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Viewport(viewport) if viewport.width == 160.0 => Some((**viewport).clone()),
            _ => None,
        })
        .expect("the viewport")
}

fn ucs_document() -> CadDocument {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut viewport = Viewport::new();
    viewport.id = 2;
    viewport.width = 160.0;
    viewport.height = 100.0;
    viewport.ucs_per_viewport = true;
    viewport.ucs_at_origin = true;
    viewport.ucs_origin = Vector3::new(1.0, 2.0, 3.0);
    viewport.ucs_x_axis = Vector3::new(0.0, 1.0, 0.0);
    viewport.ucs_y_axis = Vector3::new(-1.0, 0.0, 0.0);
    viewport.elevation = 1.5;
    viewport.ucs_ortho_type = 2;
    viewport.render_mode = ViewportRenderMode::HiddenLine;
    document.add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "Layout1").unwrap();
    let mut view = View::new("UcsView");
    view.ucs_associated = true;
    view.ucs_origin = Vector3::new(4.0, 5.0, 6.0);
    view.ucs_x_axis = Vector3::new(0.0, 0.0, 1.0);
    view.ucs_y_axis = Vector3::new(1.0, 0.0, 0.0);
    view.ucs_elevation = 2.5;
    view.ucs_ortho_type = 3;
    view.render_mode = ViewportRenderMode::FlatShaded;
    document.views.add(view).unwrap();
    let mut vport = VPort::new("Second");
    vport.ucs_per_viewport = false;
    vport.ucs_at_origin = true;
    vport.ucs_origin = Vector3::new(7.0, 8.0, 9.0);
    vport.ucs_ortho_type = 4;
    vport.ucs_elevation = 0.25;
    vport.render_mode = ViewportRenderMode::GouraudShaded;
    document.vports.add(vport).unwrap();
    write_and_read(&document, DxfVersion::AC1032)
}

/// A viewport, a VPORT and a view of R14 have no UCS and no render mode; AutoCAD's R14 file keeps them in `{UCS 74 71 / 72, 10 10 10,
/// 146, 79}` and `{VIEW 281}` of the extension dictionary (sample_AC1014.dwg: all 6 viewports, the VPORT *ACTIVE and the view
/// view_custom, with the render mode 0). The reader restores the values; an R2000 file holds them itself; an edit after the read wins.
#[test]
fn an_r14_viewport_vport_and_view_keep_their_ucs_and_render_mode_in_the_records_autocad_states() {
    let source = ucs_document();
    let r14 = write_and_read(&source, DxfVersion::AC1014);
    let viewport = the_viewport(&r14);
    assert!(viewport.ucs_at_origin && viewport.ucs_per_viewport);
    assert_eq!((viewport.ucs_origin, viewport.ucs_x_axis, viewport.ucs_y_axis), (Vector3::new(1.0, 2.0, 3.0), Vector3::new(0.0, 1.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)));
    assert_eq!((viewport.elevation, viewport.ucs_ortho_type, viewport.render_mode), (1.5, 2, ViewportRenderMode::HiddenLine));
    let view = r14.views.iter().find(|view| view.name == "UcsView").unwrap();
    assert!(view.ucs_associated);
    assert_eq!((view.ucs_origin, view.ucs_x_axis, view.ucs_y_axis), (Vector3::new(4.0, 5.0, 6.0), Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)));
    assert_eq!((view.ucs_elevation, view.ucs_ortho_type, view.render_mode), (2.5, 3, ViewportRenderMode::FlatShaded));
    let vport = r14.vports.iter().find(|vport| vport.name.eq_ignore_ascii_case("Second")).unwrap();
    assert!(vport.ucs_at_origin && !vport.ucs_per_viewport);
    assert_eq!((vport.ucs_origin, vport.ucs_elevation, vport.ucs_ortho_type, vport.render_mode), (Vector3::new(7.0, 8.0, 9.0), 0.25, 4, ViewportRenderMode::GouraudShaded));
    // the records the view shows are in AutoCAD's layout
    let records = records_with(&r14, "102=UCS");
    assert!(records.iter().any(|text| text.contains("102=UCS 74=1 71=1 10=[1.0,2.0,3.0] 10=[0.0,1.0,0.0] 10=[-1.0,0.0,0.0] 146=1.5 79=2 102=VIEW 281=2")), "{records:?}");
    assert!(records.iter().any(|text| text.contains("102=UCS 72=1 10=[4.0,5.0,6.0] 10=[0.0,0.0,1.0] 10=[1.0,0.0,0.0] 146=2.5 79=3 102=VIEW 281=3")), "{records:?}");
    assert!(records.iter().any(|text| text.contains("102=UCS 74=1 71=0 10=[7.0,8.0,9.0]")), "{records:?}");
    // consumed on read: the native model's records hold no UCS or VIEW group (the view puts them back)
    let ucs_kept = |owner: Handle| {
        r14.xrecord(owner, "ACAD_XREC_ROUNDTRIP")
            .is_some_and(|record| record.entries.iter().any(|entry| entry.code == 102 && matches!(entry.value.as_string(), Some("UCS" | "VIEW"))))
    };
    assert!(!ucs_kept(viewport.common.handle) && !ucs_kept(view.handle) && !ucs_kept(vport.handle), "the groups are out of the model");
    // the view shows what ACadSharp reads of R14: the defaults
    let viewed = acadrust::compat::acadsharp::dump_document(&r14, "", "new");
    let shown: Vec<_> = viewed["objects"].as_array().unwrap().iter().filter(|object| matches!(object["type"].as_str(), Some("Viewport" | "View" | "VPort"))).collect();
    assert_eq!(shown.iter().filter(|object| object["type"] != "VPort").count(), 2, "the viewport and the view");
    assert!(shown.iter().any(|object| object["type"] == "VPort" && object["props"]["Name"] == "SECOND"));
    for object in shown {
        let key = if object["type"] == "VPort" { "Origin" } else { "UcsOrigin" };
        let origin: Vec<f64> = object["props"][key].as_array().unwrap().iter().map(|value| value.as_f64().unwrap()).collect();
        assert_eq!((object["props"]["RenderMode"].as_i64(), origin), (Some(0), vec![0.0, 0.0, 0.0]), "{}", object["handle"]);
        if object["type"] == "Viewport" {
            assert_eq!(object["props"]["UcsPerViewport"].as_bool(), Some(false));
        }
    }
    // R2000 holds the values itself
    let r15 = write_and_read(&r14, DxfVersion::AC1015);
    let viewport = the_viewport(&r15);
    assert_eq!((viewport.ucs_origin, viewport.elevation, viewport.ucs_ortho_type), (Vector3::new(1.0, 2.0, 3.0), 1.5, 2));
    assert!(records_with(&r15, "102=UCS").is_empty());
    // an edit after the read wins
    let mut edited = r14.clone();
    for entity in edited.entities_mut() {
        if let EntityType::Viewport(viewport) = entity {
            if viewport.width == 160.0 {
                viewport.ucs_origin = Vector3::new(10.0, 11.0, 12.0);
                viewport.render_mode = ViewportRenderMode::Wireframe3D;
            }
        }
    }
    let again = the_viewport(&write_and_read(&edited, DxfVersion::AC1014));
    assert_eq!((again.ucs_origin, again.render_mode), (Vector3::new(10.0, 11.0, 12.0), ViewportRenderMode::Wireframe3D));
}

/// A viewport, a VPORT and a view made in memory with the defaults states no UCS record (the reader of a file without it gives the
/// same values); AutoCAD's down-save of a drawing states them for every one (converted drawings: the test above).
#[test]
fn a_viewport_with_the_default_ucs_made_in_memory_has_no_ucs_record() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut viewport = Viewport::new();
    viewport.id = 2;
    viewport.width = 160.0;
    viewport.height = 100.0;
    document.add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "Layout1").unwrap();
    let r14 = write_and_read(&document, DxfVersion::AC1014);
    assert!(records_with(&r14, "102=UCS").is_empty(), "{:?}", records_with(&r14, "102=UCS"));
}

/// AutoCAD's R14 file holds VX records for the viewports of the active paper space only (sample_AC1014.dwg from sample_AC1032.dwg: 3
/// records, the reserved one and the two viewports of the block *PAPER_SPACE; the other layouts' 4 viewports have none). The reserved
/// record is on.
#[test]
fn a_down_save_to_r14_states_vx_records_for_the_active_paper_space_only() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.add_layout("Second").expect("a second layout");
    for layout in ["Layout1", "Layout1", "Second", "Second"] {
        document.add_entity_to_layout(EntityType::Viewport(Box::default()), layout).expect("a viewport");
    }
    let r14 = write_and_read(&document, DxfVersion::AC1014);
    let paper_space = r14.header.paper_space_block_handle;
    let owner = |viewport: Handle| match r14.get_entity(viewport) {
        Some(EntityType::Viewport(viewport)) => viewport.common.owner_handle,
        _ => panic!("no viewport"),
    };
    let viewports: Vec<_> = r14.vx_table.iter().filter(|record| !record.viewport.is_null()).collect();
    assert!(!viewports.is_empty(), "the active paper space has viewports");
    assert!(viewports.iter().all(|record| owner(record.viewport) == paper_space), "a record for a viewport of another layout");
    // exactly one record for each viewport of the active paper space
    let mut recorded: Vec<Handle> = viewports.iter().map(|record| record.viewport).collect();
    recorded.sort();
    let mut active: Vec<Handle> = r14
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Viewport(viewport) if viewport.common.owner_handle == paper_space => Some(viewport.common.handle),
            _ => None,
        })
        .collect();
    active.sort();
    assert_eq!(recorded, active, "the records are the viewports of the active paper space");
    let total = r14.entities().filter(|entity| matches!(entity, EntityType::Viewport(_))).count();
    assert!(viewports.len() < total, "{} records for {total} viewports", viewports.len());
    let reserved: Vec<_> = r14.vx_table.iter().filter(|record| record.viewport.is_null()).collect();
    assert_eq!(reserved.len(), 1);
    assert!(reserved[0].is_on, "AutoCAD's reserved record is on");
}

/// R14's header has no INSUNITS and no DIMBLK handle, DIMLWD and DIMDSEP; AutoCAD's R14 file keeps them in the dictionary
/// `ACDBHEADERROUNDTRIPXREC` of the root dictionary: `INSUNITS {90 1}` and `DIMSTYLEDATA {DSTYLE {342 #C80, 371 15}}` (sample_AC1014.dwg).
/// The reader restores them; an R2000 header holds them itself; an edit after the read wins.
#[test]
fn an_r14_header_keeps_insunits_and_the_dimension_variables_in_the_chain_autocad_states() {
    let mut source = dim_style_document();
    let block = source.block_records.get("_BoxBlank").unwrap().handle;
    source.header.insertion_units = 4;
    source.header.dim_line_weight = 25;
    source.header.dim_arrow_block_handle = block;
    let r14 = write_and_read(&source, DxfVersion::AC1014);
    assert_eq!((r14.header.insertion_units, r14.header.dim_line_weight, r14.header.dim_arrow_block_handle), (4, 25, block));
    // consumed on read: the chain held only the two records the reader applied, so it is out of the native model
    let root = match r14.objects.get(&r14.header.named_objects_dict_handle) {
        Some(acadrust::objects::ObjectType::Dictionary(root)) => root,
        _ => panic!("no root dictionary"),
    };
    assert!(root.get("ACDBHEADERROUNDTRIPXREC").is_none(), "the chain is out of the model");
    // the view shows R14's header (units 0, weight -2) and the records as objects
    let viewed = acadrust::compat::acadsharp::dump_document(&r14, "", "new");
    assert_eq!((viewed["header"]["InsUnits"].as_i64(), viewed["header"]["DimensionLineWeight"].as_i64()), (Some(0), Some(-2)));
    let hex = format!("{:X}", block.value());
    assert_eq!(records_with(&r14, "1000=DSTYLE 1002={ 1070=342 340=#").len(), 2, "the Arrow style and the header: {:?}", records_with(&r14, "DSTYLE"));
    assert!(records_with(&r14, &format!("1000=DSTYLE 1002={{ 1070=342 340=#{hex} 1070=371 70=25 1002=}}")).len() == 1);
    assert!(records_with(&r14, "90=4").iter().any(|text| text == "90=4"), "{:?}", records_with(&r14, "90=4"));
    // an R2000 header holds the values itself
    let r15 = write_and_read(&r14, DxfVersion::AC1015);
    assert_eq!((r15.header.insertion_units, r15.header.dim_line_weight), (4, 25));
    assert!(!records_with(&r15, "1070=371 70=25").iter().any(|text| text.starts_with("1000=DSTYLE")), "no header record at R2000");
    // an edit after the read wins, and the default drops the variable
    let mut edited = r14.clone();
    edited.header.insertion_units = 6;
    edited.header.dim_line_weight = -2;
    let again = write_and_read(&edited, DxfVersion::AC1014);
    assert_eq!((again.header.insertion_units, again.header.dim_line_weight), (6, -2));
    assert!(!records_with(&again, "1070=371 70=25").iter().any(|text| text.starts_with("1000=DSTYLE")));
}
