// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, new_document};
use acadrust::entities::{Block, EntityType};
use acadrust::tables::{AppId, BlockRecord};
use acadrust::types::{Color, DxfVersion, Handle, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn dxf(record: &str, begin: &str, end: &str) -> CadDocument {
    dxf_named("Door", record, begin, end)
}

fn dxf_named(name: &str, record: &str, begin: &str, end: &str) -> CadDocument {
    let text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nBLOCK_RECORD\n0\nBLOCK_RECORD\n5\nF000\n2\n{name}\n{record}0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\nF001\n2\n{name}\n{begin}0\nENDBLK\n5\nF002\n{end}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn props(doc: &CadDocument, h: Handle) -> Value {
    dump_document(doc, "", "dxf")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{:X}", h.value()))
        .unwrap()["props"]
        .clone()
}

#[test]
fn folded_dxf_markers_retain_distinct_common_state() {
    let doc = dxf("", "62\n3\n48\n2.5\n60\n1\n370\n35\n102\n{ACAD_REACTORS\n330\nFE00\n102\n}\n102\n{ACAD_XDICTIONARY\n360\nFE01\n102\n}\n1001\nCustom\n1000\nmarker\n", "62\n5\n48\n0.5\n370\n50\n");
    let b = doc.block_records.get("Door").unwrap();
    let begin = b.block_entity_common.as_ref().unwrap();
    let end = b.block_end_common.as_ref().unwrap();
    assert_eq!(begin.color, Color::from_index(3));
    assert_eq!(end.color, Color::from_index(5));
    assert_eq!(begin.reactors, vec![Handle::new(0xFE00)]);
    assert_eq!(
        begin.extended_data.get_record("Custom").unwrap().values,
        vec![XDataValue::String("marker".into())]
    );
    assert!(b.entity_handles.is_empty());
    let p = props(&doc, b.block_entity_handle);
    assert_eq!(p["LineTypeScale"], 2.5);
    assert_eq!(p["LineWeight"], 35);
    assert_eq!(p["IsInvisible"], true);
    assert_eq!(p["Reactors"], json!(["#FE00"]));
    assert_eq!(p["XDictionary"], "#FE01");
    assert_eq!(props(&doc, b.block_end_handle)["LineTypeScale"], 0.5);
    assert_eq!(props(&doc, b.block_end_handle)["LineWeight"], 50);
}

#[test]
fn explicit_marker_entity_takes_precedence_over_folded_state() {
    let mut doc = dxf("", "62\n3\n", "");
    let b = doc.block_records.get("Door").unwrap();
    let h = b.block_entity_handle;
    let mut marker = Block::new("Door", Vector3::ZERO);
    marker.common.handle = h;
    marker.common.owner_handle = b.handle;
    marker.common.linetype_scale = 7.0;
    doc.add_entity(EntityType::Block(marker)).unwrap();
    assert_eq!(props(&doc, h)["LineTypeScale"], 7.0);
}

#[test]
fn block_source_defaults_and_explicit_zero_are_distinct() {
    let omitted = dxf("", "", "");
    let b = omitted.block_records.get("Door").unwrap();
    let p = props(&omitted, b.handle);
    assert_eq!(p["CanScale"], true);
    assert_eq!(p["IsExplodable"], false);
    assert_eq!(p["Preview"], Value::Null);
    assert_eq!(
        props(&omitted, b.block_entity_handle)["Comments"],
        Value::Null
    );
    assert!(b.explodable); // Native constructor defaults are preserved.

    let explicit = dxf("280\n0\n281\n0\n", "1\n\n4\n\n70\n127\n71\n1\n", "");
    let b = explicit.block_records.get("Door").unwrap();
    let p = props(&explicit, b.handle);
    assert_eq!(p["CanScale"], false);
    assert_eq!(p["Flags"], 127);
    assert_eq!(p["IsUnloaded"], true);
    assert!(b.flags.is_external);
    let p = props(&explicit, b.block_entity_handle);
    assert_eq!(p["Comments"], "");
    assert_eq!(p["XRefPath"], "");
    assert_eq!(p["Flags"], 127);
}

#[test]
fn preview_projection_keeps_last_dxf_chunk_and_full_native_payload() {
    let doc = dxf("310\n010203\n310\nAABB\n", "", "");
    let b = doc.block_records.get("Door").unwrap();
    assert_eq!(b.preview_data, [1, 2, 3, 0xAA, 0xBB]);
    assert_eq!(props(&doc, b.handle)["Preview"], "aabb");
}

#[test]
fn anonymous_source_resolves_the_first_handle_record() {
    let mut doc = new_document();
    let mut app = AppId::new("ACDBBLOCKREPBTAG");
    app.handle = doc.allocate_handle();
    doc.app_ids.add(app).unwrap();
    let source = doc.block_records.get("*Model_Space").unwrap().handle;
    let mut b = BlockRecord::new("*U1");
    b.handle = doc.allocate_handle();
    b.flags.anonymous = true;
    let h = b.handle;
    doc.block_records.add(b).unwrap();
    let mut data = ExtendedDataRecord::new("ACDBBLOCKREPBTAG");
    data.values = vec![XDataValue::Integer16(1), XDataValue::Handle(source)];
    doc.set_object_xdata_record(h, data);
    assert_eq!(props(&doc, h)["Source"], format!("#{:X}", source.value()));
    doc.block_records.get_mut("*U1").unwrap().flags.anonymous = false;
    assert_eq!(props(&doc, h)["Source"], Value::Null);
}

#[test]
fn dwg_block_fields_follow_source_version_without_changing_native_values() {
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        let mut b = BlockRecord::new("Door");
        b.handle = doc.allocate_handle();
        b.block_entity_handle = doc.allocate_handle();
        b.block_end_handle = doc.allocate_handle();
        b.flags.has_attributes = true;
        b.units = 4;
        b.scale_uniformly = false;
        b.description = "Description".into();
        doc.block_records.add(b).unwrap();
        let bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        let b = read.block_records.get("Door").unwrap();
        let p = props(&read, b.handle);
        assert_eq!(p["Flags"], 0);
        assert!(b.flags.has_attributes);
        assert_eq!(p["CanScale"], version < DxfVersion::AC1021);
        assert_eq!(p["IsExplodable"], version >= DxfVersion::AC1021);
        assert_eq!(p["Units"], if version < DxfVersion::AC1021 { 0 } else { 4 });
        assert_eq!(
            p["Preview"],
            if version < DxfVersion::AC1015 {
                Value::Null
            } else {
                json!("")
            }
        );
        assert_eq!(
            props(&read, b.block_entity_handle)["Comments"],
            if version < DxfVersion::AC1015 {
                Value::Null
            } else {
                json!("Description")
            }
        );
    }
}

#[test]
fn dwg_anonymous_flag_survives_native_true_name_recovery() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    let mut app = AppId::new("AcDbDynamicBlockTrueName");
    app.handle = doc.allocate_handle();
    doc.app_ids.add(app).unwrap();
    let mut b = BlockRecord::new("*U");
    b.handle = doc.allocate_handle();
    b.block_entity_handle = doc.allocate_handle();
    b.block_end_handle = doc.allocate_handle();
    b.flags.anonymous = true;
    let h = b.handle;
    doc.block_records.add(b).unwrap();
    let mut data = ExtendedDataRecord::new("AcDbDynamicBlockTrueName");
    data.values.push(XDataValue::String("PublicName".into()));
    doc.set_object_xdata_record(h, data);
    let read = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let b = read.block_records.get("PublicName").unwrap();
    assert!(!b.flags.anonymous);
    assert_eq!(props(&read, b.handle)["IsAnonymous"], true);
    assert_eq!(props(&read, b.handle)["Flags"], 1);
    assert_eq!(props(&read, b.handle)["Name"], "*U");
}

#[test]
fn new_document_block_defaults_are_nullable() {
    let doc = new_document();
    for b in doc.block_records.iter() {
        let p = props(&doc, b.handle);
        assert_eq!(p["CanScale"], true);
        assert_eq!(p["IsExplodable"], false);
        assert_eq!(p["Preview"], Value::Null);
        assert_eq!(p["Source"], Value::Null);
        let p = props(&doc, b.block_entity_handle);
        assert_eq!(p["Comments"], Value::Null);
        assert_eq!(p["XRefPath"], Value::Null);
    }
}

#[test]
fn existing_space_records_keep_all_source_table_fields() {
    let doc = dxf_named(
        "*Model_Space",
        "280\n0\n281\n1\n310\n0102\n310\nAABB\n",
        "",
        "",
    );
    let b = doc.block_records.get("*Model_Space").unwrap();
    assert!(!b.explodable);
    assert!(b.scale_uniformly);
    assert_eq!(b.preview_data, [1, 2, 0xAA, 0xBB]);
    let p = props(&doc, b.handle);
    assert_eq!(p["IsExplodable"], false);
    assert_eq!(p["CanScale"], true);
    assert_eq!(p["Preview"], "aabb");
}

#[test]
fn recovered_design_center_units_remain_native_only() {
    let xdata = "1001\nACAD\n1000\nDesignCenter Data\n1002\n{\n1070\n1\n1070\n4\n1002\n}\n";
    for explicit in [false, true] {
        let record = format!("{}{xdata}", if explicit { "70\n4\n" } else { "" });
        let doc = dxf(&record, "", "");
        let b = doc.block_records.get("Door").unwrap();
        assert_eq!(b.units, 4);
        assert_eq!(props(&doc, b.handle)["Units"], if explicit { 4 } else { 0 });
    }
}
