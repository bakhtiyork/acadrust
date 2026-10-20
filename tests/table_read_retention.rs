// SPDX-License-Identifier: MPL-2.0

use acadrust::entities::table::{BreakFlowDirection, CellContent, Table};
use acadrust::entities::EntityType;
use acadrust::types::Vector3;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfVersion};
use std::io::Cursor;

fn read_table(input: &str) -> Table {
    let document = DxfReader::from_reader(Cursor::new(input.as_bytes().to_vec()))
        .expect("valid DXF reader")
        .read()
        .expect("valid DXF document");
    let table = match document.entities().next().expect("table entity") {
        EntityType::Table(table) => (**table).clone(),
        other => panic!("expected table entity, got {other:?}"),
    };
    table
}

#[test]
fn dxf_table_keeps_insert_transform_separate_from_reused_table_codes() {
    let table = read_table(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nACAD_TABLE\n5\nF001\n100\nAcDbEntity\n8\n0\n\
         100\nAcDbBlockReference\n10\n1\n20\n2\n30\n3\n41\n2\n42\n3\n43\n4\n50\n45\n70\n2\n71\n3\n44\n4.5\n45\n6.5\n\
         100\nAcDbTable\n90\n22\n91\n1\n92\n1\n93\n1\n41\n0.125\n50\n12.5\n\
         141\n5\n142\n8\n171\n1\n175\n1\n176\n1\n170\n4\n140\n9\n144\n6\n283\n1\n\
         1\nhello\n0\nENDSEC\n0\nEOF\n",
    );

    // The native fields come from the AcDbBlockReference subclass only.
    assert_eq!(table.insertion_point, Vector3::new(1.0, 2.0, 3.0));
    assert_eq!(table.insert_scale, Vector3::new(2.0, 3.0, 4.0));
    assert!((table.insert_rotation - 45.0_f64.to_radians()).abs() < 1e-12);

    // The raw DXF fields retain the later subclass values, including the
    // overlapping table-level scale and rotation codes.
    assert_eq!(table.dxf_insert_scale, Some(Vector3::new(0.125, 3.0, 4.0)));
    assert_eq!(table.dxf_insert_rotation, Some(12.5_f64.to_radians()));
    assert_eq!(table.dxf_insert_array, Some((2, 3, 4.5, 6.5)));
}

#[test]
fn table_defaults_keep_source_presence_markers_distinct_from_payload() {
    let table = Table::new(Vector3::ZERO, 1, 1);
    assert_eq!(table.insert_scale, Vector3::new(1.0, 1.0, 1.0));
    assert_eq!(table.dxf_insert_scale, None);
    assert_eq!(table.dxf_insert_array, None);
    assert!(!table.has_break_data);
    assert_eq!(table.source_break_flow_direction, None);
    assert_eq!(table.base_style_type, None);
    assert_eq!(table.rows[0].source_style_type, None);
    assert_eq!(table.columns[0].source_style_type, None);
    assert_eq!(table.rows[0].cells[0].source_style_type, None);

    let mut content = CellContent::new();
    assert!(!content.format_has_data);
    content.format_has_data = true;
    assert!(content.format_has_data);
}

#[test]
fn dxf_reader_accepts_acad_table_with_no_insert_transform() {
    let table = read_table(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1018\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nACAD_TABLE\n5\nF002\n100\nAcDbEntity\n8\n0\n\
         100\nAcDbTable\n90\n0\n91\n0\n92\n0\n0\nENDSEC\n0\nEOF\n",
    );
    assert_eq!(table.insert_scale, Vector3::new(1.0, 1.0, 1.0));
    assert_eq!(table.insert_rotation, 0.0);
    assert_eq!(table.dxf_insert_scale, Some(Vector3::new(1.0, 1.0, 1.0)));
    assert_eq!(table.dxf_insert_rotation, Some(0.0));
    assert_eq!(table.rows.len(), 0);
    assert_eq!(table.columns.len(), 0);
}

#[test]
fn dwg_roundtrip_retains_explicit_empty_break_section() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::ZERO, 0, 0);
    table.has_break_data = true;
    table.source_break_flow_direction = Some(0);
    let handle = document
        .add_entity(EntityType::Table(Box::new(table)))
        .expect("table handle");

    let bytes = DwgWriter::write_to_vec(&document).expect("DWG write");
    let mut loaded = DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .expect("DWG read");
    let EntityType::Table(table) = loaded.get_entity(handle).expect("round-tripped table") else {
        panic!("round-tripped entity changed type")
    };
    assert!(table.has_break_data);
    assert_eq!(table.source_break_flow_direction, Some(0));
    assert!(table.break_data.is_empty());

    let EntityType::Table(table) = loaded.get_entity_mut(handle).expect("mutable table") else {
        panic!("round-tripped entity changed type")
    };
    table.break_flow_direction = BreakFlowDirection::Left;
    let bytes = DwgWriter::write_to_vec(&loaded).expect("edited DWG write");
    let edited = DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .expect("edited DWG read");
    let EntityType::Table(table) = edited.get_entity(handle).expect("edited table") else {
        panic!("edited entity changed type")
    };
    assert_eq!(table.break_flow_direction, BreakFlowDirection::Left);
    assert_eq!(table.source_break_flow_direction, Some(4));
}
