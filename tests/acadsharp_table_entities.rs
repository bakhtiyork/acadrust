// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::table::{
    CellContent, CellContentGeometry, CellEdgeFlags, CellStyle, CellValue, CellValueType, Table,
    TableBreakData, TableCustomData,
};
use acadrust::entities::EntityType;
use acadrust::types::{Color, Handle, Vector3};
use acadrust::{CadDocument, DxfReader, DxfVersion};
use serde_json::{json, Value};
use std::io::Cursor;

fn table_props(doc: &CadDocument) -> Value {
    dump_document(
        doc,
        "",
        if doc.dwg_source_version.is_some() {
            "dwg"
        } else {
            "dxf"
        },
    )["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["type"] == "TableEntity")
        .unwrap()["props"]
        .clone()
}

fn document(table: Table, version: DxfVersion) -> CadDocument {
    let mut doc = CadDocument::with_version(version);
    doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    doc
}

#[test]
fn table_inherits_insert_counts_transforms_and_keeps_break_presence() {
    let mut t = Table::new(Vector3::new(4.0, 5.0, 6.0), 2, 3);
    t.insert_scale = Vector3::new(2.0, 3.0, 4.0);
    t.insert_rotation = 0.5;
    t.has_break_data = true;
    t.source_break_flow_direction = Some(0);
    t.break_spacing = 2.25;
    t.break_data.push(TableBreakData {
        position: Vector3::new(7.0, 8.0, 9.0),
        height: 11.0,
        flags: 17,
    });
    let doc = document(t, DxfVersion::AC1032);
    let p = table_props(&doc);
    assert_eq!(p["ObjectName"], "ACAD_TABLE");
    assert_eq!(p["HasDynamicSubclass"], true);
    assert_eq!(p["ColumnCount"], 1);
    assert_eq!(p["RowCount"], 1);
    assert_eq!(p["Columns"].as_array().unwrap().len(), 3);
    assert_eq!(p["Rows"].as_array().unwrap().len(), 2);
    assert_eq!(p["InsertPoint"], json!([4.0, 5.0, 6.0]));
    assert_eq!(p["Rotation"], 0.5);
    assert_eq!(p["XScale"], 2.0);
    assert_eq!(p["YScale"], 3.0);
    assert_eq!(p["ZScale"], 4.0);
    assert_eq!(p["HasBreadData"], true);
    assert_eq!(
        p["BreakData"],
        json!({
            "BreakSpacing": 2.25, "Flags": 0, "FlowDirection": 0,
            "Heights": [{"Height": 11.0, "Position": [7.0, 8.0, 9.0]}],
        })
    );
    assert!(!p.to_string().contains("!unmapped"));
}

#[test]
fn modern_table_preserves_no_data_type_and_separates_cached_merge_geometry() {
    let mut t = Table::new(Vector3::ZERO, 1, 1);
    t.columns[0].source_style_type = Some(37);
    let c = &mut t.rows[0].cells[0];
    c.source_style_type = Some(1);
    c.merge_width = 8;
    c.merge_height = 4;
    c.merged = 1;
    c.contents.push(CellContent::text("stored text"));
    c.geometry = Some(CellContentGeometry {
        width: 12.0,
        height: 13.0,
        outer_width: 22.0,
        outer_height: 23.0,
        distance_to_top_left: Vector3::ZERO,
        distance_to_center: Vector3::ZERO,
        flags: 5,
    });
    let doc = document(t, DxfVersion::AC1032);
    let p = table_props(&doc);
    let s = &p["Columns"][0]["CellStyleOverride"];
    assert_eq!(s["Type"], 37);
    assert_eq!(s["HasData"], false);
    assert_eq!(s["Name"], Value::Null);
    assert_eq!(s["TextStyle"], "#0");
    assert_eq!(s["TopBorder"]["EdgeFlags"], 2);
    let c = &p["Rows"][0]["Cells"][0];
    assert_eq!(c["Type"], 0);
    assert_eq!(c["BorderWidth"], 0);
    assert_eq!(c["MergedValue"], 0);
    assert_eq!(c["Contents"][0]["Format"]["HasData"], false);
    assert_eq!(c["Contents"][0]["Format"]["TextHeight"], 0.0);
    assert_eq!(c["Geometry"]["ContentWidth"], 12.0);
    assert_eq!(c["Geometry"]["Width"], 22.0);
    assert_eq!(c["Geometry"]["ContentHeight"], 13.0);
    assert_eq!(c["Geometry"]["Height"], 23.0);
}

#[test]
fn modern_table_style_slots_and_content_templates_follow_reference_build() {
    let mut t = Table::new(Vector3::ZERO, 1, 1);
    let mut style = CellStyle::new();
    style.alignment = 5;
    style.content_color = Color::from_index(3);
    style.fill_enabled = true;
    style.text_style_handle = Some(Handle::new(0x4321));
    style.applied_border_edges = CellEdgeFlags::TOP;
    style.top_border.invisible = true;
    style.top_border.double_spacing = 1.25;
    t.rows[0].cells[0].style = Some(style);
    let mut content = CellContent::text("formatted");
    content.format_has_data = true;
    content.text_style_handle = Some(Handle::new(0x1234));
    content.text_height = 4.0;
    t.rows[0].cells[0].contents.push(content);
    let doc = document(t, DxfVersion::AC1032);
    let p = table_props(&doc);
    let c = &p["Rows"][0]["Cells"][0];
    let s = &c["StyleOverride"];
    assert_eq!(s["Alignment"], 5);
    assert_eq!(s["CellAlignment"], 0);
    assert_eq!(s["Color"]["Index"], 3);
    assert_eq!(s["ContentColor"]["Index"], 0);
    assert_eq!(s["TextColor"]["Index"], 0);
    assert_eq!(s["IsFillColorOn"], false);
    assert_eq!(s["TextStyle"], "#0");
    assert_eq!(s["TopBorder"]["ApplyBorder"], true);
    assert_eq!(s["TopBorder"]["IsInvisible"], true);
    assert_eq!(s["TopBorder"]["DoubleLineSpacing"], 1.25);
    assert_eq!(c["Contents"][0]["Format"]["TextStyle"], "#0");
    assert_eq!(c["Contents"][0]["Format"]["TextHeight"], 4.0);
}

#[test]
fn table_custom_values_keep_metadata_empty_flags_and_calendar_date() {
    let mut t = Table::new(Vector3::ZERO, 1, 1);
    let mut date = CellValue::new();
    date.value_type = CellValueType::Date;
    date.raw_type_code = 8;
    date.binary_value = [2024i16, 11, 5, 15, 0, 0, 0, 0]
        .into_iter()
        .flat_map(i16::to_le_bytes)
        .collect();
    t.rows[0].custom_data_items.push(TableCustomData {
        name: "date".into(),
        value: date,
    });
    let mut empty = CellValue::number(91.0);
    empty.flags = 5;
    t.columns[0].custom_data_items.push(TableCustomData {
        name: "empty".into(),
        value: empty,
    });
    let doc = document(t, DxfVersion::AC1032);
    let p = table_props(&doc);
    let d = &p["Rows"][0]["CustomDataCollection"][0]["Value"];
    assert_eq!(d["Value"], 2460630.0);
    assert_eq!(d["ValueType"], 8);
    assert_eq!(d["Format"], "");
    let e = &p["Columns"][0]["CustomDataCollection"][0]["Value"];
    assert_eq!(e["Value"], Value::Null);
    assert_eq!(e["Flags"], 5);
    assert_eq!(e["ValueType"], 2);
}

#[test]
fn legacy_table_overrides_preserve_native_fill_and_ignore_unbuilt_style_reference() {
    let mut t = Table::new(Vector3::ZERO, 1, 1);
    let mut style = CellStyle::new();
    style.legacy_override_bits = true;
    style.override_flags = 0x3F;
    style.alignment = 4;
    style.fill_enabled = false;
    style.text_height = 7.0;
    style.text_style_handle = Some(Handle::new(0x123));
    t.rows[0].cells[0].style = Some(style);
    t.rows[0].cells[0]
        .contents
        .push(CellContent::text("legacy"));
    let doc = document(t, DxfVersion::AC1018);
    let p = table_props(&doc);
    let c = &p["Rows"][0]["Cells"][0];
    assert_eq!(c["StyleOverride"]["HasData"], false);
    assert_eq!(c["StyleOverride"]["CellAlignment"], 4);
    assert_eq!(c["StyleOverride"]["IsFillColorOn"], true);
    assert_eq!(c["StyleOverride"]["TextHeight"], 7.0);
    assert_eq!(c["StyleOverride"]["TextStyle"], "#0");
    assert_eq!(c["Contents"][0]["ContentType"], 0);
    assert_eq!(c["Contents"][0]["CadValue"]["Format"], Value::Null);
    assert_eq!(c["Contents"][0]["CadValue"]["FormattedValue"], Value::Null);
    assert_eq!(c["Contents"][0]["CadValue"]["Value"], "legacy");
    let EntityType::Table(native) = doc.entities().next().unwrap() else {
        panic!()
    };
    assert!(!native.rows[0].cells[0].style.as_ref().unwrap().fill_enabled);
}

#[test]
fn dxf_table_margin_overlap_and_ignored_cell_formats_are_compat_only() {
    let input = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1018\n0\nENDSEC\n\
        0\nSECTION\n2\nENTITIES\n0\nACAD_TABLE\n5\nF001\n100\nAcDbEntity\n8\n0\n\
        100\nAcDbBlockReference\n10\n1\n20\n2\n30\n3\n41\n2\n42\n3\n43\n4\n\
        70\n3\n71\n2\n44\n5\n45\n6\n\
        100\nAcDbTable\n343\nBEEF\n342\nCAFE\n90\n22\n91\n1\n92\n1\n93\n1\n41\n0.125\n141\n5\n142\n8\n\
        171\n1\n175\n1\n176\n1\n170\n4\n140\n9\n144\n6\n283\n1\n1\nhello\n\
        0\nENDSEC\n0\nEOF\n";
    let doc = DxfReader::from_reader(Cursor::new(input.as_bytes()))
        .unwrap()
        .read()
        .unwrap();
    let p = table_props(&doc);
    assert_eq!(p["XScale"], 0.125);
    assert_eq!(p["YScale"], 3.0);
    assert_eq!(p["ZScale"], 4.0);
    assert_eq!(p["ColumnCount"], 3);
    assert_eq!(p["RowCount"], 2);
    assert_eq!(p["ColumnSpacing"], 5.0);
    assert_eq!(p["RowSpacing"], 6.0);
    assert_eq!(p["IsMultiple"], true);
    assert_eq!(p["Block"], Value::Null);
    assert_eq!(p["Style"], "#0");
    assert_eq!(p["Columns"].as_array().unwrap().len(), 1);
    assert_eq!(p["Rows"][0]["Cells"][0]["StyleOverride"]["TextHeight"], 0.0);
    assert_eq!(
        p["Rows"][0]["Cells"][0]["StyleOverride"]["CellAlignment"],
        0
    );
    assert_eq!(
        p["Rows"][0]["Cells"][0]["StyleOverride"]["IsFillColorOn"],
        true
    );
    assert_eq!(p["Rows"][0]["Cells"][0]["BlockScale"], 0.0);
    let EntityType::Table(t) = doc.entities().next().unwrap() else {
        panic!()
    };
    assert_eq!(t.insert_scale, Vector3::new(2.0, 3.0, 4.0));
    assert_eq!(t.block_record_handle, Some(Handle::new(0xBEEF)));
    assert_eq!(t.table_style_handle, Some(Handle::new(0xCAFE)));
    assert_eq!(t.rows[0].cells[0].style.as_ref().unwrap().text_height, 9.0);
    assert_eq!(t.rows[0].cells[0].block_scale, 6.0);
}
