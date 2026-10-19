// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, handle};
use acadrust::entities::{EntityType, Line};
use acadrust::io::dwg::eed_codec::decode_values;
use acadrust::io::dxf::DxfReader;
use acadrust::tables::{AppId, TextStyle};
use acadrust::types::{Handle, Vector3};
use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use acadrust::CadDocument;
use serde_json::{json, Value};
use std::io::Cursor;

fn object<'a>(dump: &'a Value, h: Handle) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["handle"] == handle(h))
        .unwrap_or_else(|| panic!("missing object {h:?}"))
}

fn line_with_xdata(data: ExtendedData) -> (CadDocument, Handle) {
    let mut doc = CadDocument::new();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.extended_data = data;
    let handle = doc.add_entity(EntityType::Line(line)).unwrap();
    (doc, handle)
}

fn add_app(doc: &mut CadDocument, value: u64, name: &str) -> Handle {
    let handle = Handle::new(value);
    let mut app = AppId::new(name);
    app.handle = handle;
    doc.app_ids.add(app).unwrap();
    handle
}

#[test]
fn structured_layer_xdata_uses_numeric_layer_handle() {
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("TEST_LAYER");
    record.add_value(XDataValue::LayerName("0".to_owned()));
    data.add_record(record);
    let (mut doc, entity) = line_with_xdata(data);
    let application = add_app(&mut doc, 0x700, "TEST_LAYER");
    let layer = doc.layers.get("0").unwrap().handle;

    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, entity)["props"]["ExtendedData"],
        json!([[handle(application), {
            "Records": [{"Code": 1003, "RawValue": layer.value()}]
        }]])
    );
}

#[test]
fn raw_eed_layer_xdata_is_decoded_with_numeric_handle() {
    let application = 0x701_u64;
    let mut data = ExtendedData::new();
    let layer = CadDocument::new().layers.get("0").unwrap().handle;
    let mut payload = vec![3_u8];
    payload.extend_from_slice(&layer.value().to_be_bytes());
    data.raw_dwg_eed.push((application, payload));
    let (mut doc, entity) = line_with_xdata(data);
    let application = add_app(&mut doc, application, "TEST_RAW");

    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, entity)["props"]["ExtendedData"],
        json!([[handle(application), {
            "Records": [{"Code": 1003, "RawValue": layer.value()}]
        }]])
    );
}

#[test]
fn raw_layer_handle_survives_structured_unknown_layer_projection() {
    let application = 0x705_u64;
    let source_handle = 0xDEAD_u64;
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("TEST_RAW");
    record.add_value(XDataValue::LayerName(String::new()));
    data.add_record(record);
    let mut payload = vec![3_u8];
    payload.extend_from_slice(&source_handle.to_be_bytes());
    data.raw_dwg_eed.push((application, payload));
    let (mut doc, entity) = line_with_xdata(data);
    let application = add_app(&mut doc, application, "TEST_RAW");

    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, entity)["props"]["ExtendedData"],
        json!([[handle(application), {
            "Records": [{"Code": 1003, "RawValue": source_handle}]
        }]])
    );
}

#[test]
fn raw_layer_handle_patch_matches_record_position() {
    let application = 0x706_u64;
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("TEST_RAW_POSITION");
    record.add_value(XDataValue::LayerName("0".to_owned()));
    record.add_value(XDataValue::LayerName(String::new()));
    data.add_record(record);
    let mut payload = vec![3_u8];
    payload.extend_from_slice(&0_u64.to_be_bytes());
    payload.push(3);
    payload.extend_from_slice(&999_u64.to_be_bytes());
    data.raw_dwg_eed.push((application, payload));
    let (mut doc, entity) = line_with_xdata(data);
    doc.layers.get_mut("0").unwrap().handle = Handle::NULL;
    let application = add_app(&mut doc, application, "TEST_RAW_POSITION");

    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, entity)["props"]["ExtendedData"],
        json!([[handle(application), {
            "Records": [
                {"Code": 1003, "RawValue": 0},
                {"Code": 1003, "RawValue": 999}
            ]
        }]])
    );
}

#[test]
fn wide_eed_nul_is_native_but_trimmed_in_compat_dump() {
    let payload = [
        0, 7, 0, b'D', 0, b'S', 0, b'T', 0, b'Y', 0, b'L', 0, b'E', 0, 0, 0,
    ];
    assert_eq!(
        decode_values(&payload, true, |_| None),
        Some(vec![XDataValue::String("DSTYLE\0".to_owned())])
    );

    let application = 0x702_u64;
    let mut data = ExtendedData::new();
    data.raw_dwg_eed.push((application, payload.to_vec()));
    let (mut doc, entity) = line_with_xdata(data);
    let application = add_app(&mut doc, application, "TEST_WIDE");
    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, entity)["props"]["ExtendedData"],
        json!([[handle(application), {
            "Records": [{"Code": 1000, "RawValue": "DSTYLE"}]
        }]])
    );
}

#[test]
fn undecodable_or_unknown_eed_is_unmapped() {
    for (application, name) in [(0x7FFE_u64, Some("TEST_BAD")), (0x7FFF, None)] {
        let mut data = ExtendedData::new();
        data.raw_dwg_eed.push((application, vec![0xFF]));
        let (mut doc, entity) = line_with_xdata(data);
        if let Some(name) = name {
            add_app(&mut doc, application, name);
        }
        let dump = dump_document(&doc, "", "new");
        assert_eq!(object(&dump, entity)["props"]["ExtendedData"], "!unmapped");
    }
}

#[test]
fn malformed_duplicate_raw_app_is_not_hidden_by_valid_block() {
    let application = 0x703_u64;
    let mut data = ExtendedData::new();
    data.raw_dwg_eed.push((application, vec![70, 1, 0]));
    data.raw_dwg_eed.push((application, vec![0xFF]));
    let (mut doc, entity) = line_with_xdata(data);
    add_app(&mut doc, application, "TEST_DUPLICATE_RAW");

    let dump = dump_document(&doc, "", "new");
    assert_eq!(object(&dump, entity)["props"]["ExtendedData"], "!unmapped");
}

#[test]
fn text_style_font_is_synthesized_as_acad_xdata() {
    let mut doc = CadDocument::new();
    let mut style = TextStyle::new("FontStyle");
    style.handle = Handle::new(0x702);
    style.true_type_font = "Arial".to_owned();
    style.true_type_font_flags = 34;
    doc.text_styles.add(style.clone()).unwrap();
    let custom = add_app(&mut doc, 0x704, "STYLE_CUSTOM");
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("STYLE_CUSTOM");
    record.add_value(XDataValue::String("retained".to_owned()));
    data.add_record(record);
    doc.object_xdata.insert(style.handle, data);
    let acad = doc.app_ids.get("ACAD").unwrap().handle;

    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, style.handle)["props"]["ExtendedData"],
        json!([
            [handle(custom), {"Records": [{"Code": 1000, "RawValue": "retained"}]}],
            [handle(acad), {
                "Records": [
                    {"Code": 1000, "RawValue": "Arial"},
                    {"Code": 1071, "RawValue": 34}
                ]
            }]
        ])
    );
}

#[test]
fn r12_layer_xdata_uses_source_handle_zero() {
    let source = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n70\n1\n0\nLAYER\n2\n0\n70\n0\n62\n7\n6\nCONTINUOUS\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTEXT\n8\n0\n10\n0\n20\n0\n30\n0\n40\n1\n1\ntext\n1001\nTEST_LAYER\n1003\n0\n0\nENDSEC\n0\nEOF\n";
    let document = DxfReader::from_reader(Cursor::new(source.as_bytes()))
        .expect("R12 reader")
        .read()
        .expect("R12 document");
    let dump = dump_document(&document, "legacy.dxf", "dxf");
    let text = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == "TextEntity")
        .expect("TEXT object");
    assert_eq!(
        text["props"]["ExtendedData"],
        json!([["#0", {"Records": [{"Code": 1003, "RawValue": 0}]}]])
    );
}
