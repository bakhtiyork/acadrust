// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::{EntityType, MText};
use acadrust::types::{Color, DxfVersion, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};
use serde_json::{json, Value};
use std::io::Cursor;

fn dxf(codes: &str) -> CadDocument {
    dxf_version(codes, "AC1032")
}

fn dxf_version(codes: &str, version: &str) -> CadDocument {
    let bytes = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{version}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nMTEXT\n5\n100\n100\nAcDbEntity\n100\nAcDbMText\n{codes}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(bytes.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn native(doc: &CadDocument) -> &MText {
    doc.entities()
        .find_map(|e| match e {
            EntityType::MText(t) => Some(t.as_ref()),
            _ => None,
        })
        .unwrap()
}

fn projected(doc: &CadDocument) -> Value {
    dump_document(doc, "", "new")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["type"] == "MText")
        .unwrap()["props"]
        .clone()
}

#[test]
fn omitted_dxf_fields_and_explicit_defaults_are_distinct() {
    let omitted = dxf("");
    let explicit = dxf("63\n256\n73\n2\n46\n4.5\n441\n0\n");
    let p = projected(&omitted);
    assert_eq!(native(&omitted).background_color, Color::ByLayer);
    assert_eq!(p["AlignmentPoint"], json!([1.0, 0.0, 0.0]));
    assert_eq!(p["BackgroundColor"]["Index"], 0);
    assert_eq!(p["BackgroundTransparency"]["Value"], 0);
    assert_eq!(p["LineSpacingStyle"], 0);
    assert_eq!(p["RectangleHeight"], 0.0);
    let p = projected(&explicit);
    assert_eq!(p["BackgroundColor"]["Index"], 256);
    assert_eq!(p["BackgroundTransparency"]["Value"], 100);
    assert_eq!(p["LineSpacingStyle"], 2);
    assert_eq!(p["RectangleHeight"], 4.5);
}

#[test]
fn extents_remain_native_while_the_reader_view_uses_oracle_defaults() {
    let doc = dxf("40\n3\n41\n12\n42\n10.25\n43\n7.5\n");
    assert_eq!(native(&doc).extents_width, 10.25);
    assert_eq!(native(&doc).extents_height, 7.5);
    let mut without_source = native(&doc).clone();
    without_source.dxf_source = None;
    assert_eq!(native(&doc), &without_source);
    without_source.extents_width = 11.0;
    assert_ne!(native(&doc), &without_source);
    let p = projected(&doc);
    assert_eq!(p["Height"], 3.0);
    assert_eq!(p["RectangleWidth"], 12.0);
    assert_eq!(p["HorizontalWidth"], 0.9);
    assert_eq!(p["VerticalHeight"], 0.2);
}

#[test]
fn alignment_follows_source_order_without_changing_native_ocs_geometry() {
    let doc = dxf("11\n2\n21\n0\n31\n7\n50\n90\n210\n0\n220\n1\n230\n0\n");
    assert_eq!(
        native(&doc).dwg_x_direction,
        Some(Vector3::new(2.0, 0.0, 7.0))
    );
    let p = projected(&doc);
    assert!((p["Rotation"].as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert_eq!(p["AlignmentPoint"][2], 0.0);
    let doc = dxf("50\n90\n11\n4\n21\n-2\n31\n3\n");
    assert_eq!(projected(&doc)["AlignmentPoint"], json!([4.0, -2.0, 3.0]));
}

#[test]
fn dxf_columns_use_the_embedded_layout_and_do_not_overwrite_rectangle() {
    let doc = dxf("41\n12\n46\n8\n101\nEmbedded Object\n71\n2\n72\n2\n73\n0\n74\n1\n40\n100\n41\n200\n44\n5\n45\n0.75\n46\n3\n46\n4\n");
    let p = projected(&doc);
    assert_eq!(p["HasColumns"], true);
    assert_eq!(p["RectangleWidth"], 12.0);
    assert_eq!(p["RectangleHeight"], 8.0);
    assert_eq!(
        p["ColumnData"],
        json!({"AutoHeight":false,"ColumnCount":2,"ColumnType":2,"FlowReversed":true,"Gutter":0.75,"Heights":[3.0,4.0],"Width":5.0})
    );
    let legacy = dxf("75\n2\n76\n1\n79\n0\n48\n6\n49\n1\n50\n9\n");
    assert_eq!(native(&legacy).column_data.heights, vec![9.0]);
    assert_eq!(projected(&legacy)["HasColumns"], false);
}

#[test]
fn dxf_background_color_packing_and_transparency_follow_oracle_units() {
    let doc = dxf("421\n66051\n441\n33554560\n"); // RGB 1,2,3 and alpha 128.
    assert_eq!(
        native(&doc).background_color,
        Color::Rgb { r: 1, g: 2, b: 3 }
    );
    let p = projected(&doc);
    assert_eq!(p["BackgroundColor"]["TrueColor"], 197121);
    assert_eq!(p["BackgroundTransparency"]["Value"], 49);
}

#[test]
fn chunked_dxf_value_preserves_lexical_escapes_and_honors_edits() {
    let mut doc = dxf("3\nprefix \n3\n\\U+00B0^J\n1\n{\\C1;red}\\Ptail\n");
    assert_eq!(native(&doc).value, "prefix °\n{\\C1;red}\\Ptail");
    let p = projected(&doc);
    assert_eq!(p["Value"], "prefix \\U+00B0\n{\\C1;red}\\Ptail");
    assert_eq!(p["PlainText"], "prefix U+00B0\nred\ntail");
    let handle = native(&doc).common.handle;
    let EntityType::MText(text) = doc.get_entity_mut(handle).unwrap() else {
        panic!()
    };
    text.value = "edited".into();
    assert_eq!(projected(&doc)["Value"], "edited");
}

#[test]
fn binary_dxf_retains_lexical_text_without_changing_native_decoding() {
    for (source, decoded, expected) in [
        (r"94\U+00B0", "94°", r"94\U+00B0"),
        ("first^Jsecond", "first^Jsecond", "first\nsecond"),
    ] {
        let mut doc = CadDocument::new();
        doc.add_entity(EntityType::MText(Box::new(MText::with_value(
            source,
            Vector3::ZERO,
        ))))
        .unwrap();
        let bytes = DxfWriter::new_binary(&doc).write_to_vec().unwrap();
        let read = DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .read()
            .unwrap();
        assert_eq!(native(&read).value, decoded);
        assert_eq!(projected(&read)["Value"], expected);
    }
}

#[test]
fn dxf_caret_decoding_does_not_join_controls_across_chunks() {
    let doc = dxf("3\nfirst^\n1\nJsecond\n");
    assert_eq!(projected(&doc)["Value"], "first^Jsecond");
    let doc = dxf("3\n\\U+00B0^\n1\nJsecond^Jthird\n");
    assert_eq!(projected(&doc)["Value"], "\\U+00B0^Jsecond\nthird");
}

#[test]
fn dwg_versions_retain_text_and_project_spacing_columns_and_background() {
    for version in [
        DxfVersion::AC1014,
        DxfVersion::AC1018,
        DxfVersion::AC1021,
        DxfVersion::AC1032,
    ] {
        let mut doc = CadDocument::with_version(version);
        let mut text = MText::with_value(r"94\U+00B0", Vector3::ZERO);
        text.extents_width = 7.0;
        text.extents_height = 4.0;
        text.column_data.column_type = 2;
        text.column_data.column_count = 2;
        text.column_data.width = 5.0;
        text.column_data.heights = vec![4.0, 3.0];
        doc.add_entity(EntityType::MText(Box::new(text))).unwrap();
        let bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        assert_eq!(native(&read).value, "94°");
        assert_eq!(native(&read).extents_width, 7.0);
        let p = projected(&read);
        assert_eq!(p["Value"], r"94\U+00B0");
        assert_eq!(p["HorizontalWidth"], 0.9);
        assert_eq!(p["BackgroundColor"]["Index"], 0);
        assert_eq!(
            p["LineSpacingStyle"],
            if version == DxfVersion::AC1014 { 0 } else { 1 }
        );
        assert_eq!(p["ColumnData"]["ColumnCount"], 0);
        if version == DxfVersion::AC1032 {
            assert_eq!(native(&read).column_data.column_count, 2);
            assert_eq!(p["HasColumns"], true);
            assert_eq!(p["ColumnData"]["Heights"], json!([4.0, 3.0]));
        }
    }
}

#[test]
fn legacy_background_recovery_stays_native() {
    let doc = dxf_version("1001\nACAD\n1000\nACAD_MTEXT_BBRT\n1070\n91\n1071\n1\n1070\n46\n1040\n1.25\n1070\n64\n1071\n-1023344381\n1000\nACAD_MTEXT_BERT\n", "AC1015");
    assert!(native(&doc).background_from_xdata);
    assert_eq!(native(&doc).background_fill_flags, 1);
    assert_eq!(native(&doc).background_scale, 1.25);
    let p = projected(&doc);
    assert_eq!(p["BackgroundFillFlags"], 0);
    assert_eq!(p["BackgroundScale"], 1.5);
    assert_eq!(p["BackgroundColor"]["Index"], 0);
}

#[test]
fn dwg_text_frame_background_uses_percentage_transparency() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = MText::new();
    text.background_fill_flags = 16;
    text.background_color = Color::from_rgb(1, 2, 3);
    text.background_transparency = 65;
    doc.add_entity(EntityType::MText(Box::new(text))).unwrap();
    let bytes = DwgWriter::write_to_vec(&doc).unwrap();
    let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    assert_eq!(native(&read).background_transparency, 65);
    let p = projected(&read);
    assert_eq!(p["BackgroundTransparency"]["Value"], 65);
    assert_eq!(p["BackgroundColor"]["TrueColor"], 197121);
}
