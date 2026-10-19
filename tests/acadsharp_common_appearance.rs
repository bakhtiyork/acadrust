// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_reader_document, handle};
use acadrust::entities::{EntityType, Line};
use acadrust::objects::{Group, ObjectType, Scale};
use acadrust::types::{Color, DxfVersion, Handle, LineWeight, Transparency, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn object(dump: &Value, h: Handle) -> &Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == handle(h))
        .unwrap()
}

#[test]
fn old_dwg_omitted_lineweights_keep_entity_and_layer_defaults() {
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let mut doc = CadDocument::with_version(version);
        let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
        line.common.line_weight = LineWeight::Value(35);
        doc.layers.get_mut("0").unwrap().line_weight = LineWeight::Value(50);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();
        let read = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        assert_eq!(
            read.get_entity(h).unwrap().common().line_weight,
            if version == DxfVersion::AC1014 {
                LineWeight::ByLayer
            } else {
                LineWeight::Value(35)
            }
        );
        assert_eq!(
            read.layers.get("0").unwrap().line_weight,
            if version == DxfVersion::AC1014 {
                LineWeight::Default
            } else {
                LineWeight::Value(50)
            }
        );
    }
}

#[test]
fn reader_color_packing_and_zero_enc_transparency_leave_native_values_intact() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.color = Color::from_rgb(12, 34, 56);
    let rgb = doc.add_entity(EntityType::Line(line)).unwrap();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_Y);
    line.common.color = Color::ByBlock;
    let block = doc.add_entity(EntityType::Line(line)).unwrap();
    let read = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let dump = dump_reader_document(&read, "synthetic.dwg", "dwg");
    assert_eq!(object(&dump, rgb)["props"]["Color"]["TrueColor"], 0x38220C);
    assert_eq!(object(&dump, block)["props"]["Transparency"]["Value"], 0);
    assert_eq!(
        read.get_entity(rgb).unwrap().common().color,
        Color::from_rgb(12, 34, 56)
    );
    assert_eq!(
        read.get_entity(block).unwrap().common().transparency,
        Transparency::ByLayer
    );
}

#[test]
fn reader_resolves_reactors_and_appends_group_attachment_without_deduplicating_it() {
    let mut doc = CadDocument::new();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    let group_handle = Handle::new(0x700);
    line.common.reactors = vec![group_handle, group_handle, Handle::new(0xFFFF)];
    line.common.xdictionary_handle = Some(group_handle); // wrong reference type
    line.common.material_handle = Some(group_handle);
    let line_handle = doc.add_entity(EntityType::Line(line)).unwrap();
    let mut group = Group::new("G");
    group.handle = group_handle;
    group.entities = vec![line_handle];
    doc.objects.insert(group_handle, ObjectType::Group(group));
    doc.header.source_handle_seed = Some(0x800);
    let dump = dump_reader_document(&doc, "synthetic.dwg", "dwg");
    let p = &object(&dump, line_handle)["props"];
    assert_eq!(
        p["Reactors"],
        json!([handle(group_handle), handle(group_handle)])
    );
    assert_eq!(p["XDictionary"], Value::Null);
    assert_eq!(p["Material"], Value::Null);
    assert_eq!(
        doc.get_entity(line_handle).unwrap().common().reactors.len(),
        3
    );
}

#[test]
fn legacy_polyline_seqend_keeps_scalar_codes_but_does_not_build_reference_templates() {
    let source = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n2\n0\nLAYER\n2\n0\n70\n0\n62\n7\n6\nCONTINUOUS\n0\nLAYER\n2\nA\n70\n0\n62\n3\n6\nCONTINUOUS\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n5\n20\n8\nA\n66\n1\n70\n0\n0\nVERTEX\n5\n21\n8\nA\n10\n1\n20\n2\n30\n0\n0\nSEQEND\n5\n22\n8\nA\n62\n3\n0\nPOINT\n5\n23\n8\nA\n6\nBYBLOCK\n10\n0\n20\n0\n30\n0\n0\nENDSEC\n0\nEOF\n";
    let doc = DxfReader::from_reader(Cursor::new(source.as_bytes()))
        .unwrap()
        .read()
        .unwrap();
    let dump = dump_reader_document(&doc, "legacy.dxf", "dxf");
    let objects = dump["objects"].as_array().unwrap();
    let zero = objects
        .iter()
        .find(|o| o["type"] == "Layer" && o["props"]["Name"] == "0")
        .unwrap();
    let bylayer = objects
        .iter()
        .find(|o| {
            o["type"] == "LineType"
                && o["props"]["Name"]
                    .as_str()
                    .is_some_and(|s| s.eq_ignore_ascii_case("ByLayer"))
        })
        .unwrap();
    let end = &object(&dump, Handle::new(0x22))["props"];
    assert_eq!(end["Layer"], zero["handle"]);
    assert_eq!(end["Color"]["Index"], 3);
    assert_eq!(
        object(&dump, Handle::new(0x23))["props"]["LineType"],
        bylayer["handle"]
    );
    let EntityType::Polyline2D(polyline) = doc.get_entity(Handle::new(0x20)).unwrap() else {
        panic!("polyline")
    };
    assert_eq!(polyline.seqend.as_ref().unwrap().common.layer, "A");
}

#[test]
fn absent_object_common_data_has_empty_collections_and_null_dictionary() {
    let mut doc = CadDocument::new();
    let mut scale = Scale::default();
    scale.handle = Handle::new(0x700);
    doc.objects.insert(scale.handle, ObjectType::Scale(scale));
    let dump = acadrust::compat::acadsharp::dump_document(&doc, "", "new");
    let p = &object(&dump, Handle::new(0x700))["props"];
    assert_eq!(p["ExtendedData"], json!([]));
    assert_eq!(p["Reactors"], json!([]));
    assert_eq!(p["XDictionary"], Value::Null);
}

#[test]
fn color_provenance_does_not_change_native_equality() {
    let original = acadrust::entities::EntityCommon::new();
    let mut decoded = original.clone();
    decoded.dwg_color_source = Some((256, 0));
    assert_eq!(original, decoded);
    decoded.color = Color::Index(3);
    assert_ne!(original, decoded);
    decoded.color = original.color;
    decoded.transparency = Transparency::Explicit(128);
    assert_ne!(original, decoded);
}
