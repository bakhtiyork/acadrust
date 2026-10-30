// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-18 (gate G5-RC sample_V and G5-RR): what the fork writes into a DWG compared with AutoCAD's own save of the same drawing.
//! Synthetic documents only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing
//! (sample_AC1014.dwg to sample_AC1032.dwg).

use std::io::Cursor;

use acadrust::entities::Viewport;
use acadrust::types::DxfVersion;
use acadrust::{CadDocument, DwgReader, DwgWriter, EntityType};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

// ── the view parameters of a viewport read from an R14 file ────────────────────────────────────────────────────────────

fn viewport_drawing(source: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(source);
    document.dwg_source_version = Some(source);
    let mut view = Viewport::new();
    view.id = 2;
    view.width = 160.0;
    view.height = 100.0;
    view.status.fast_zoom = true;
    document.add_entity_to_layout(EntityType::Viewport(Box::new(view)), "Layout1").unwrap();
    document
}

fn viewport(document: &CadDocument) -> &Viewport {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Viewport(view) if view.width == 160.0 => Some(view.as_ref()),
            _ => None,
        })
        .expect("the viewport")
}

fn has_mview(view: &Viewport) -> bool {
    view.common.extended_data.records().iter().any(|record| {
        record.application_name.eq_ignore_ascii_case("ACAD")
            && record.values.iter().any(|value| matches!(value, acadrust::xdata::XDataValue::String(name) if name == "MVIEW"))
    })
}

/// AutoCAD's R14 file states the view of a viewport in the ACAD / MVIEW extended data and fast zoom 1 (the six viewports #240 to #26B of
/// sample_AC1014.dwg); its R2000 and later files of the drawing hold the view in the fields of the viewport, no extended data and fast
/// zoom 0 in the status word (sample_AC1015.dwg to sample_AC1032.dwg). A viewport read from an R14 file does the same in a DWG of R2000
/// or later; an R14 file keeps both, and a viewport that did not come from R13/R14 keeps what it holds.
#[test]
fn a_viewport_read_from_r14_drops_the_mview_record_and_fast_zoom_in_a_dwg_of_r2000_or_later() {
    let r14 = write_and_read(&viewport_drawing(DxfVersion::AC1032), DxfVersion::AC1014);
    assert_eq!(r14.dwg_source_version, Some(DxfVersion::AC1014));
    assert!(has_mview(viewport(&r14)) && viewport(&r14).status.fast_zoom, "the R14 file holds the MVIEW record and fast zoom 1");
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let later = write_and_read(&r14, version);
        assert!(!has_mview(viewport(&later)), "{version:?}: no MVIEW record");
        assert!(viewport(&later).common.extended_data.records().is_empty(), "{version:?}: no extended data at all");
        assert!(!viewport(&later).status.fast_zoom, "{version:?}: fast zoom 0");
        assert_eq!((viewport(&later).width, viewport(&later).height), (160.0, 100.0), "{version:?}: the fields stay");
    }
    // R14 again: both come back
    let again = write_and_read(&write_and_read(&r14, DxfVersion::AC1018), DxfVersion::AC1014);
    assert!(has_mview(viewport(&again)), "R14 -> R2004 -> R14 states the MVIEW record again");
    // another source keeps its flag, and has no MVIEW record to drop
    let r2004 = write_and_read(&viewport_drawing(DxfVersion::AC1018), DxfVersion::AC1018);
    assert!(viewport(&r2004).status.fast_zoom, "an R2004 file keeps fast zoom 1");
    let mut other = viewport_drawing(DxfVersion::AC1032);
    other.dwg_source_version = None;
    assert!(viewport(&write_and_read(&other, DxfVersion::AC1032)).status.fast_zoom, "a document made in memory keeps its flag");
}

/// Other data of the ACAD application stays: only the MVIEW group is dropped.
#[test]
fn only_the_mview_group_is_dropped_from_the_acad_record_of_a_viewport() {
    let mut r14 = write_and_read(&viewport_drawing(DxfVersion::AC1032), DxfVersion::AC1014);
    for entity in r14.entities_mut() {
        if let EntityType::Viewport(view) = entity {
            let mut record = view.common.extended_data.get_record("ACAD").cloned().expect("the ACAD record");
            record.values.push(acadrust::xdata::XDataValue::String("OTHER".into()));
            view.common.extended_data.upsert_record(record);
        }
    }
    let later = write_and_read(&r14, DxfVersion::AC1015);
    let view = viewport(&later);
    assert!(!has_mview(view));
    let kept: Vec<_> = view.common.extended_data.records().iter().flat_map(|record| record.values.iter()).collect();
    assert!(kept.iter().any(|value| matches!(value, acadrust::xdata::XDataValue::String(text) if text == "OTHER")), "{kept:?}");
}

// ── the "referenced" bit of the symbol table records ───────────────────────────────────────────────────────────────────

fn referenced(document: &CadDocument) -> Vec<(&'static str, bool)> {
    let layer = document.layers.iter().next().expect("a layer");
    let style = document.text_styles.iter().next().expect("a text style");
    let ltype = document.line_types.iter().next().expect("a line type");
    let app = document.app_ids.iter().next().expect("an application");
    let dim = document.dim_styles.iter().next().expect("a dimension style");
    let vport = document.vports.iter().next().expect("a VPORT");
    vec![
        ("layer", layer.source_flags & 64 != 0),
        ("text style", style.source_flags & 64 != 0),
        ("line type", ltype.source_flags & 64 != 0),
        ("application", app.source_flags & 64 != 0),
        ("dimension style", dim.xref_reference),
        ("vport", vport.xref_reference),
    ]
}

/// AutoCAD's files before R2007 state bit 64 (referenced) on every record of these tables (sample_AC1014.dwg, sample_AC1015.dwg and
/// sample_AC1018.dwg) and its R2010 and later files on none (sample_AC1024.dwg, sample_AC1027.dwg, sample_AC1032.dwg), whatever the
/// drawing was saved from: a save from R2010 states the bit at R14, R2000 and R2004.
#[test]
fn the_records_of_a_dwg_before_r2007_state_the_referenced_bit() {
    let source = CadDocument::with_version(DxfVersion::AC1032);
    assert!(referenced(&source).iter().all(|(_, bit)| !bit), "{:?}", referenced(&source));
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let read = write_and_read(&source, version);
        assert!(referenced(&read).iter().all(|(_, bit)| *bit), "{version:?}: {:?}", referenced(&read));
    }
    for version in [DxfVersion::AC1024, DxfVersion::AC1032] {
        let read = write_and_read(&source, version);
        assert!(referenced(&read).iter().all(|(_, bit)| !bit), "{version:?}: {:?}", referenced(&read));
    }
    // from R2004 to R2010 the bit is gone (the format has no place for it)
    let r2004 = write_and_read(&source, DxfVersion::AC1018);
    assert!(write_and_read(&r2004, DxfVersion::AC1024).layers.iter().all(|layer| layer.source_flags & 64 == 0));
}

// ── the order of the extended data of an R14 block record ──────────────────────────────────────────────────────────────

/// AutoCAD's R14 file states the records of a block record in the order its other applications, the `ACAD` record with the insertion units
/// (`DesignCenter Data`), the description and the preview icon (sample_AC1014.dwg: `ACDBDYNAMICBLOCKGUID`, ACAD for #6F2; ... ACAD,
/// CONTENTBLOCKDESCRIPTION, CONTENTBLOCKICON for #A7E; ACAD, CONTENTBLOCKICON for #77F); the writer added the units last. Judged in the
/// ACadSharp view of the file the fork wrote.
#[test]
fn the_units_record_of_an_r14_block_record_comes_before_its_preview_icon_and_after_its_other_records() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut app = acadrust::tables::AppId::new("OTHER_APP");
    app.handle = document.allocate_handle();
    document.app_ids.add(app).ok();
    let mut block = acadrust::tables::BlockRecord::new("with_icon");
    block.handle = document.allocate_handle();
    block.units = 6;
    block.preview_data = (0..300u32).map(|i| (i % 251) as u8).collect();
    let handle = block.handle;
    document.block_records.add(block).ok();
    let mut other = acadrust::xdata::ExtendedData::new();
    let mut record = acadrust::xdata::ExtendedDataRecord::new("OTHER_APP");
    record.add_value(acadrust::xdata::XDataValue::String("other".into()));
    other.add_record(record);
    document.object_xdata.insert(handle, other);
    let r14 = write_and_read(&document, DxfVersion::AC1014);
    let dump = acadrust::compat::acadsharp::dump_document(&r14, "", "new");
    let object = dump["objects"].as_array().unwrap().iter().find(|o| o["handle"] == format!("#{:X}", handle.value())).expect("the block record");
    let firsts: Vec<_> = object["props"]["ExtendedData"].as_array().unwrap().iter().map(|entry| entry[1]["Records"][0]["RawValue"].clone()).collect();
    assert_eq!(firsts.len(), 3, "{firsts:?}");
    assert_eq!((&firsts[0], &firsts[1]), (&serde_json::json!("other"), &serde_json::json!("DesignCenter Data")), "{firsts:?}");
}
