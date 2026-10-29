// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::objects::{NamedTableCellStyle, ObjectType, TableStyle, TableStyleFlags};
use acadrust::types::{Color, Handle, LineWeight};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfVersion};
use serde_json::{json, Value};
use std::io::Cursor;

fn props(doc: &CadDocument, reader: &str, h: Handle) -> Value {
    dump_document(doc, "", reader)["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{:X}", h.value()))
        .unwrap()["props"]
        .clone()
}

fn dxf(object: &str) -> CadDocument {
    let s = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n{object}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(s.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn dxf_mline_flags_and_description_presence_are_retained() {
    for (description, expected) in [
        ("", Value::Null),
        ("3\n\n", json!("")),
        ("3\nExplicit\n", json!("Explicit")),
    ] {
        let doc = dxf(&format!(
            "0\nMLINESTYLE\n5\nF001\n2\nNamed\n70\n273\n{description}"
        ));
        let ObjectType::MLineStyle(style) = doc.objects.get(&Handle::new(0xF001)).unwrap() else {
            panic!()
        };
        assert_eq!(style.flags.to_bits(), 273);
        let mut without_source = style.clone();
        without_source.dxf_description_present = None;
        assert_eq!(*style, without_source);
        let p = props(&doc, "dxf", style.handle);
        assert_eq!(p["Description"], expected);
        assert_eq!(p["Flags"], 273);
    }
}

#[test]
fn dxf_table_preserves_source_scalars_but_reference_does_not_install_unnamed_rows() {
    for (description, expected) in [
        ("", Value::Null),
        ("3\n\n", json!("")),
        ("3\nStored\n", json!("Stored")),
    ] {
        let doc = dxf(&format!("0\nTABLESTYLE\n5\nF001\n100\nAcDbTableStyle\n{description}71\n0\n40\n0\n41\n0\n280\n1\n281\n1\n7\nStandard\n140\n4\n62\n3\n170\n9\n"));
        let ObjectType::TableStyle(style) = doc.objects.get(&Handle::new(0xF001)).unwrap() else {
            panic!()
        };
        assert_eq!(style.data_row_style.text_height, 4.0);
        assert_eq!(style.data_row_style.text_color, Color::from_index(3));
        let p = props(&doc, "dxf", style.handle);
        assert_eq!(p["Description"], expected);
        assert_eq!(p["Flags"], 0);
        assert_eq!(p["HorizontalCellMargin"], 0.0);
        assert_eq!(p["VerticalCellMargin"], 0.0);
        assert_eq!(p["SuppressTitle"], true);
        assert_eq!(p["SuppressHeaderRow"], true);
        assert_eq!(p["DataCellStyle"]["TextHeight"], 0.0);
        assert_eq!(p["DataCellStyle"]["HorizontalMargin"], 0.06);
        assert_eq!(p["CellStyles"], json!([]));
    }
}

#[test]
fn legacy_table_view_keeps_color_slots_margins_and_source_flags_separate() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    let mut style = TableStyle::new("Legacy");
    style.legacy_source = Some((0x10, false, true));
    style.flags = TableStyleFlags::from_bits_retain(0x13);
    style.title_suppressed = true;
    style.header_suppressed = true;
    style.horizontal_margin = 9.0;
    style.data_row_style.text_color = Color::from_rgb(0x12, 0x34, 0x56);
    style.data_row_style.data_type = 17;
    style.data_row_style.unit_type = 23;
    style.data_row_style.format_string = "custom".into();
    let h = Handle::new(0xF001);
    doc.objects.insert(h, ObjectType::TableStyle(style));
    let p = props(&doc, "dwg", h);
    assert_eq!(p["Flags"], 0x10);
    assert_eq!(p["SuppressTitle"], false);
    assert_eq!(p["SuppressHeaderRow"], true);
    let row = &p["DataCellStyle"];
    assert_eq!(row["TextColor"]["TrueColor"], 0x563412);
    assert_eq!(row["Color"]["IsByBlock"], true);
    assert_eq!(row["ContentColor"]["IsByBlock"], true);
    assert_eq!(row["HorizontalMargin"], 0.06);
    assert_eq!(row["ValueDataType"], 17);
    assert_eq!(row["ValueUnitType"], 23);
    assert_eq!(row["ValueFormatString"], "custom");
    doc.version = DxfVersion::AC1018;
    assert_eq!(
        props(&doc, "dwg", h)["DataCellStyle"]["ValueFormatString"],
        Value::Null
    );
}

#[test]
fn modern_table_view_uses_present_data_and_distinct_borders() {
    let mut doc = CadDocument::new();
    let mut style = TableStyle::new("Modern");
    style.flags = TableStyleFlags::from_bits_retain(8);
    style.horizontal_margin = 9.0;
    let mut cell = NamedTableCellStyle::default();
    cell.name = "Table".into();
    cell.style_type = 2;
    cell.cell_style.data_flags = 1;
    cell.cell_style.content_format.content_color = Color::from_rgb(1, 2, 3);
    cell.cell_style.content_format.property_flags = 12;
    cell.cell_style.content_format.property_override_flags = 34;
    cell.cell_style.margin_override_flags = 0;
    for (mask, weight, invisible) in [(1, 13, true), (2, 25, false), (32, 50, true)] {
        cell.cell_style
            .borders
            .push(acadrust::objects::TableGridFormat {
                index_mask: mask,
                border: acadrust::objects::TableCellBorder {
                    line_weight: LineWeight::from_value(weight),
                    is_invisible: invisible,
                    ..Default::default()
                },
                ..Default::default()
            });
    }
    style.modern_style = Some(cell);
    let mut built_in = NamedTableCellStyle::default();
    built_in.name = "_data".into(); // builder matches case-insensitively
    style.modern_overrides.push((3, built_in));
    let h = Handle::new(0xF001);
    doc.objects.insert(h, ObjectType::TableStyle(style));
    let p = props(&doc, "dwg", h);
    assert_eq!(p["Flags"], 0);
    assert_eq!(p["HorizontalCellMargin"], 0.06);
    let cell = &p["TableCellStyle"];
    assert_eq!(cell["Color"]["TrueColor"], 0x030201);
    assert_eq!(cell["TextColor"]["IsByBlock"], true);
    assert_eq!(cell["ContentColor"]["IsByBlock"], true);
    assert_eq!(cell["PropertyFlags"], 12);
    assert_eq!(cell["PropertyOverrideFlags"], 34);
    assert_eq!(cell["HorizontalMargin"], 0.06);
    assert_eq!(cell["TopBorder"]["EdgeFlags"], 2); // reference constructor quirk
    assert_eq!(cell["TopBorder"]["LineWeight"], 13);
    assert_eq!(cell["RightBorder"]["LineWeight"], 25);
    assert_eq!(cell["HorizontalInsideBorder"]["LineWeight"], 50);
    assert_eq!(cell["TopBorder"]["ApplyBorder"], true);
    assert_eq!(cell["TopBorder"]["IsInvisible"], false);
    assert_eq!(cell["RightBorder"]["IsInvisible"], true);
    assert_eq!(cell["LeftBorder"]["ApplyBorder"], false);
    assert_eq!(p["DataCellStyle"]["HasData"], false);
    assert_eq!(p["DataCellStyle"]["HorizontalMargin"], 0.06);
    assert_eq!(p["DataCellStyle"]["Scale"], 0.0);
    assert_eq!(p["DataCellStyle"]["ValueFormatString"], Value::Null);
}

#[test]
fn dwg_table_reader_retains_description_and_legacy_source_before_normalization() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        let style = doc
            .objects
            .values_mut()
            .find_map(|o| {
                if let ObjectType::TableStyle(s) = o {
                    Some(s)
                } else {
                    None
                }
            })
            .unwrap();
        // The record's text is the description (ACadSharp's DwgObjectWriter: `Description TV 3`); the name is the dictionary key.
        style.description = "Stored description".into();
        let restored = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        let style = restored
            .objects
            .values()
            .find_map(|o| {
                if let ObjectType::TableStyle(s) = o {
                    Some(s)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(style.description, "Stored description");
        assert_eq!(style.name, "Standard", "the name is the key of the ACAD_TABLESTYLE entry, not the description");
        assert_eq!(style.legacy_source.is_some(), version < DxfVersion::AC1024);
        let mut native = style.clone();
        native.legacy_source = None;
        assert_eq!(*style, native);
    }
}
