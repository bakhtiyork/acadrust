// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-16 (gate G5-RR / G4): the R14 leader's inline sizes, the R14 fast zoom flag and the DXF table value flags. Synthetic documents
//! only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing (sample_AC1014.dwg to sample_AC1032.dwg
//! and their DXF twins).

use std::io::Cursor;

use acadrust::entities::table::{CellValue, CellValueType, Table};
use acadrust::entities::{Leader, Viewport};
use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::objects::ObjectType;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter, EntityType};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

// ── the arrowhead size and dimension gap of an R14 leader ──────────────────────────────────────────────────────────────

fn style_override(code: i16, value: f64) -> Vec<XDataValue> {
    vec![XDataValue::Integer16(code), XDataValue::Real(value)]
}

/// Three leaders of a drawing of R2018: one that takes its sizes from the dimension style, one with its own values, one with a
/// `DSTYLE` override in its extended data.
fn leader_drawing() -> (CadDocument, [Handle; 3]) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let style = document.dim_styles.get_mut("Standard").expect("the standard dimension style");
    style.dimasz = 2.5;
    style.dimgap = 0.625;
    let mut leaders = [Leader::new(), Leader::new(), Leader::new()];
    for leader in &mut leaders {
        leader.vertices = vec![Vector3::ZERO, Vector3::new(5.0, 5.0, 0.0)];
    }
    leaders[1].arrow_size = 3.0;
    leaders[1].dimension_gap = 0.5;
    let mut record = ExtendedDataRecord::new("ACAD");
    record.add_value(XDataValue::String("DSTYLE".into()));
    record.add_value(XDataValue::ControlString("{".into()));
    for (code, value) in [(41, 1.5), (147, 0.25)] {
        for value in style_override(code, value) {
            record.add_value(value);
        }
    }
    record.add_value(XDataValue::ControlString("}".into()));
    leaders[2].common.extended_data.add_record(record);
    let handles = leaders.map(|leader| document.add_entity(EntityType::Leader(leader)).unwrap());
    (document, handles)
}

fn sizes(document: &CadDocument, handle: Handle) -> (f64, f64) {
    let Some(EntityType::Leader(leader)) = document.get_entity(handle) else { panic!("no leader") };
    (leader.arrow_size, leader.dimension_gap)
}

/// The record of R13 and R14 states the arrowhead size and the dimension gap inline (the leader #512 of sample_AC1014.dwg: 2.5 and
/// 0.625 = DIMASZ and DIMGAP of its style ISO-25); the record of R2000 and later takes them from the style, so a leader read from there
/// holds 0 and the R14 writer stated 0 where AutoCAD's R14 file states the style's values. A value of the leader's own and a `DSTYLE`
/// override win over the style.
#[test]
fn an_r14_leader_states_the_sizes_of_its_dimension_style() {
    let (document, [from_style, own, overridden]) = leader_drawing();
    assert_eq!(sizes(&document, from_style), (0.0, 0.0));
    let r14 = write_and_read(&document, DxfVersion::AC1014);
    assert_eq!(sizes(&r14, from_style), (2.5, 0.625), "the style's DIMASZ and DIMGAP");
    assert_eq!(sizes(&r14, own), (3.0, 0.5), "the leader's own values");
    assert_eq!(sizes(&r14, overridden), (1.5, 0.25), "the DSTYLE override");
    // R2000 and later state none of them
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        assert_eq!(sizes(&write_and_read(&document, version), from_style), (0.0, 0.0), "{version:?}");
    }
    // a leader read from an R14 file states what it holds (its inline copy is its own value, whatever the style says now), and an
    // edit after the read wins
    assert_eq!(r14.dwg_source_version, Some(DxfVersion::AC1014));
    let mut edited = r14.clone();
    if let Some(EntityType::Leader(leader)) = edited.get_entity_mut(from_style) {
        leader.arrow_size = 0.0;
        leader.dimension_gap = 0.0;
    }
    assert_eq!(sizes(&write_and_read(&edited, DxfVersion::AC1014), from_style), (0.0, 0.0), "a leader of an R14 file keeps its 0");
    if let Some(EntityType::Leader(leader)) = edited.get_entity_mut(from_style) {
        leader.arrow_size = 4.0;
    }
    assert_eq!(sizes(&write_and_read(&edited, DxfVersion::AC1014), from_style), (4.0, 0.0), "an edit after the read wins");
}

// ── the fast zoom flag of an R14 viewport ──────────────────────────────────────────────────────────────────────────────

fn viewport_drawing(version: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    document.dwg_source_version = Some(version);
    let mut view = Viewport::new();
    view.id = 2;
    view.width = 160.0;
    view.height = 100.0;
    view.status.fast_zoom = false;
    document.add_entity_to_layout(EntityType::Viewport(Box::new(view)), "Layout1").unwrap();
    document.vports.get_mut("*Active").expect("the active VPORT").fast_zoom = false;
    document
}

fn fast_zoom(document: &CadDocument) -> (bool, bool) {
    let view = document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Viewport(view) if view.width == 160.0 => Some(view),
            _ => None,
        })
        .expect("the viewport");
    (view.status.fast_zoom, document.vports.get("*Active").expect("the active VPORT").fast_zoom)
}

/// AutoCAD's own R14 file states fast zoom 1 for every viewport (the six of sample_AC1014.dwg, #240 to #26B) and the VPORT, where its
/// R2000 file of the drawing holds 0 in the status of the viewports (sample_AC1015.dwg; FASTZOOM is 1 and fixed since R2000). A
/// document that does not come from an R13/R14 file states 1 in an R14 file; one read from an R14 file keeps its flag.
#[test]
fn a_viewport_and_vport_of_an_r14_file_state_fast_zoom_as_autocads_r14_file() {
    for source in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let document = viewport_drawing(source);
        assert_eq!(fast_zoom(&document), (false, false));
        assert_eq!(fast_zoom(&write_and_read(&document, DxfVersion::AC1014)), (true, true), "{source:?} -> R14");
        assert_eq!(fast_zoom(&write_and_read(&document, DxfVersion::AC1018)), (false, false), "{source:?} -> R2004 keeps the flags");
    }
    // a document made in memory (no source file)
    let mut made = viewport_drawing(DxfVersion::AC1032);
    made.dwg_source_version = None;
    assert_eq!(fast_zoom(&write_and_read(&made, DxfVersion::AC1014)), (true, true));
    // a document read from an R14 DXF states its own flag as well (review of R5b-16: the DXF source was not tested)
    let mut from_r14_dxf = viewport_drawing(DxfVersion::AC1032);
    from_r14_dxf.dwg_source_version = None;
    from_r14_dxf.dxf_source_version = Some(DxfVersion::AC1014);
    assert_eq!(fast_zoom(&write_and_read(&from_r14_dxf, DxfVersion::AC1014)), (false, false), "an R14 DXF source keeps its flag");
    // an R14 file keeps what it holds, and an edit after the read wins
    let r14 = write_and_read(&viewport_drawing(DxfVersion::AC1032), DxfVersion::AC1014);
    assert_eq!(r14.dwg_source_version, Some(DxfVersion::AC1014));
    let mut edited = r14.clone();
    for entity in edited.entities_mut() {
        if let EntityType::Viewport(view) = entity {
            if view.width == 160.0 {
                view.status.fast_zoom = false;
            }
        }
    }
    edited.vports.get_mut("*Active").unwrap().fast_zoom = false;
    assert_eq!(fast_zoom(&write_and_read(&edited, DxfVersion::AC1014)), (false, false));
}

// ── the value flags of a table in a DXF ────────────────────────────────────────────────────────────────────────────────

fn table_drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::ZERO, 1, 2);
    table.set_cell_text(0, 0, "0.500000");
    table.set_cell_text(0, 1, "60");
    let item = &mut table.rows[0].cells[0].contents[0];
    item.value = CellValue::new();
    item.value.value_type = CellValueType::Double;
    item.value.raw_type_code = 2;
    item.value.numeric_value = 0.5;
    item.format_value_data_type = 2;
    item.value_format = "%lu2%pr2".to_string();
    item.format_has_data = true;
    let handle = document.add_entity(EntityType::Table(Box::new(table))).unwrap();
    (document, handle)
}

/// The value of the first group 90 of the first ACAD_TABLE entity of a DXF text (its value flags).
fn dxf_value_flags(text: &str) -> i32 {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines.chunks_exact(2).position(|pair| pair == ["0", "ACAD_TABLE"]).expect("an ACAD_TABLE entity");
    lines.chunks_exact(2).skip(start + 1).take_while(|pair| pair[0] != "0").find(|pair| pair[0] == "90").expect("group 90")[1].parse().unwrap()
}

/// A table read from an R2010 DWG is the R2010 record (value flags 0, TABLECONTENT objects). AutoCAD's DXF of every version states
/// the value flags 22 in the ACAD_TABLE entity (sample_AC1015_ascii.dxf to sample_AC1032_ascii.dxf: `90 22` for both tables); the DXF
/// writer stated 0. Read back, the table is the R2010 record again, and a table with value flags of its own keeps them.
#[test]
fn the_dxf_of_a_table_of_an_r2010_dwg_states_the_value_flags_of_autocads_dxf() {
    let (document, handle) = table_drawing();
    let r2010 = write_and_read(&write_and_read(&document, DxfVersion::AC1018), DxfVersion::AC1024);
    assert!(r2010.objects.values().any(|object| matches!(object, ObjectType::TableContent(_))), "the objects stay in the R2010 file");
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1032] {
        let mut source = r2010.clone();
        source.version = version;
        let text = String::from_utf8(DxfWriter::new(&source).write_to_vec().unwrap()).unwrap();
        assert_eq!(dxf_value_flags(&text), 22, "{version:?}");
        let back = DxfReader::from_reader(Cursor::new(text.into_bytes())).unwrap().read().unwrap();
        let Some(EntityType::Table(table)) = back.get_entity(handle) else { panic!("{version:?}: no table") };
        assert_eq!(table.value_flags, 0, "{version:?}: the R2010 record again");
    }
    // a table whose own value flags are not 0 keeps them
    let mut own = r2010.clone();
    own.version = DxfVersion::AC1032;
    if let Some(EntityType::Table(table)) = own.get_entity_mut(handle) {
        table.value_flags = 6;
    }
    let text = String::from_utf8(DxfWriter::new(&own).write_to_vec().unwrap()).unwrap();
    assert_eq!(dxf_value_flags(&text), 6);
}
