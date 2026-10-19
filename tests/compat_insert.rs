// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::{AttributeEntity, EntityType, Insert};
use acadrust::objects::{Dictionary, ObjectType, SpatialFilter};
use acadrust::tables::BlockRecord;
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn object<'a>(dump: &'a Value, ty: &str) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["type"] == ty)
        .unwrap()
}

#[test]
fn insert_spatial_filter_uses_the_acad_filter_dictionary_path() {
    let mut document = CadDocument::new();

    let mut block = BlockRecord::new("B");
    block.handle = Handle::new(0x300);
    document.block_records.add(block).unwrap();

    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.handle = Handle::new(0x100);
    insert.common.xdictionary_handle = Some(Handle::new(0x200));
    document.add_entity(EntityType::Insert(insert)).unwrap();

    let mut extension = Dictionary::new();
    extension.handle = Handle::new(0x200);
    extension.owner = Handle::new(0x100);
    extension.add_entry("ACAD_FILTER", Handle::new(0x201));

    let mut filters = Dictionary::new();
    filters.handle = Handle::new(0x201);
    filters.owner = Handle::new(0x200);
    filters.add_entry("SPATIAL", Handle::new(0x202));

    let mut spatial = SpatialFilter::new();
    spatial.handle = Handle::new(0x202);
    spatial.owner = Handle::new(0x201);

    document
        .objects
        .insert(extension.handle, ObjectType::Dictionary(extension));
    document
        .objects
        .insert(filters.handle, ObjectType::Dictionary(filters));
    document
        .objects
        .insert(spatial.handle, ObjectType::SpatialFilter(spatial));

    let dump = dump_document(&document, "synthetic.dxf", "dxf");
    let insert = object(&dump, "Insert");
    assert_eq!(insert["props"]["Block"], "#300");
    assert_eq!(insert["props"]["SpatialFilter"], "#202");
}

#[test]
fn r12_ownerless_attributes_remain_records_but_are_not_insert_children() {
    let dxf = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n5\n30\n8\n0\n66\n1\n2\nB\n10\n0\n20\n0\n0\nATTRIB\n5\n31\n8\n0\n10\n0\n20\n0\n40\n1\n1\nvalue\n2\nTAG\n70\n0\n0\nSEQEND\n5\n32\n0\nENDSEC\n0\nEOF\n";
    let document = DxfReader::from_reader(Cursor::new(dxf.as_bytes()))
        .unwrap()
        .read()
        .unwrap();

    let dump = dump_document(&document, "synthetic.dxf", "dxf");
    let insert = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["handle"] == "#30" && value["type"] == "Insert")
        .unwrap();
    assert_eq!(insert["props"]["Attributes"], json!([]));
    assert_eq!(insert["props"]["HasAttributes"], false);
    assert!(dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value["handle"] == "#31" && value["type"] == "AttributeEntity"));
}

#[test]
fn constructed_insert_attributes_stay_in_the_compatibility_collection() {
    let mut document = CadDocument::new();
    let mut block = BlockRecord::new("B");
    block.handle = Handle::new(0x300);
    document.block_records.add(block).unwrap();

    let mut insert = Insert::new("B", Vector3::ZERO);
    insert.common.handle = Handle::new(0x100);
    let mut attribute = AttributeEntity::simple("TAG", "value");
    attribute.common.handle = Handle::new(0x101);
    insert.attributes.push(attribute);
    document.add_entity(EntityType::Insert(insert)).unwrap();

    let dump = dump_document(&document, "synthetic.dxf", "dxf");
    let insert = object(&dump, "Insert");
    assert_eq!(insert["props"]["Attributes"], json!(["#101"]));
    assert_eq!(insert["props"]["HasAttributes"], true);
}
