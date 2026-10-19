// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, handle as dump_handle};
use acadrust::entities::{EntityType, Underlay, UnderlayType};
use acadrust::objects::{BookColor, Dictionary, Group, ObjectType, UnderlayDefinition};
use acadrust::types::{Color, Handle};
use acadrust::{CadDocument, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn object<'a>(dump: &'a Value, handle: Handle) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["handle"] == dump_handle(handle))
        .unwrap()
}

#[test]
fn media_object_names_and_markers_follow_acadsharp() {
    let owner = Handle::new(0x100);
    let group_handle = Handle::new(0x101);
    let color_handle = Handle::new(0x102);
    let definition_handle = Handle::new(0x103);

    let mut dictionary = Dictionary::new();
    dictionary.handle = owner;
    dictionary.add_entry("*A1", group_handle);
    dictionary.add_entry("RAL CLASSIC$RAL 1006", color_handle);
    dictionary.add_entry("pdf-definition - 1", definition_handle);

    let mut group = Group::new("");
    group.handle = group_handle;
    group.owner = owner;

    let color = BookColor {
        dxf_color_present: None,
        handle: color_handle,
        owner,
        color: Color::from_rgb(226, 144, 0),
        color_name: "RAL 1006".into(),
        book_name: "RAL CLASSIC".into(),
    };

    let mut definition = UnderlayDefinition::pdf("..\\pdf-definition.pdf", "1");
    definition.handle = definition_handle;
    definition.owner_handle = owner;

    let mut doc = CadDocument::new();
    doc.objects
        .insert(owner, ObjectType::Dictionary(dictionary));
    doc.objects.insert(group_handle, ObjectType::Group(group));
    doc.objects
        .insert(color_handle, ObjectType::BookColor(color));
    doc.objects.insert(
        definition_handle,
        ObjectType::UnderlayDefinition(definition),
    );

    let mut underlay = Underlay::new(UnderlayType::Pdf);
    underlay.definition_handle = definition_handle;
    doc.add_entity(EntityType::Underlay(underlay)).unwrap();

    let dump = dump_document(&doc, "", "new");
    assert_eq!(object(&dump, group_handle)["props"]["Name"], "*A1");
    assert_eq!(
        object(&dump, color_handle)["props"]["Name"],
        "RAL CLASSIC$RAL 1006"
    );
    assert_eq!(
        object(&dump, color_handle)["props"]["Color"]["TrueColor"],
        37090
    );
    assert_eq!(
        object(&dump, definition_handle)["props"]["Name"],
        "pdf-definition - 1"
    );
    assert_eq!(
        object(&dump, definition_handle)["props"]["SubclassMarker"],
        "AcDbUnderlayDefinition"
    );
    let entity = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["type"] == "PdfUnderlay")
        .unwrap();
    assert_eq!(entity["props"]["SubclassMarker"], "AcDbUnderlayReference");
}

#[test]
fn dxf_image_definition_reads_resolution_units() {
    let source = concat!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1018\n0\nENDSEC\n",
        "0\nSECTION\n2\nOBJECTS\n",
        "0\nIMAGEDEF\n5\n10\n100\nAcDbRasterImageDef\n",
        "90\n0\n1\nimage.JPG\n10\n994.0\n20\n965.0\n",
        "11\n0.2645833333333333\n21\n0.2645833333333333\n",
        "280\n1\n281\n5\n",
        "0\nENDSEC\n0\nEOF\n"
    );
    let doc = DxfReader::from_reader(Cursor::new(source.as_bytes()))
        .unwrap()
        .read()
        .unwrap();
    let dump = dump_document(&doc, "", "dxf");
    assert_eq!(object(&dump, Handle::new(0x10))["type"], "ImageDefinition");
    assert_eq!(object(&dump, Handle::new(0x10))["props"]["Units"], json!(5));
}

#[test]
fn dxf_image_definition_keeps_native_defaults_when_fields_are_omitted() {
    let source = concat!(
        "0\nSECTION\n2\nOBJECTS\n",
        "0\nIMAGEDEF\n5\n10\n100\nAcDbRasterImageDef\n",
        "1\nimage.JPG\n",
        "0\nENDSEC\n0\nEOF\n"
    );
    let doc = DxfReader::from_reader(Cursor::new(source.as_bytes()))
        .unwrap()
        .read()
        .unwrap();
    let image = match doc.objects.get(&Handle::new(0x10)).unwrap() {
        acadrust::objects::ObjectType::ImageDefinition(image) => image,
        object => panic!("expected IMAGEDEF, got {object:?}"),
    };
    assert!(!image.is_loaded);
    assert_eq!(image.pixel_size, (0.0, 0.0));
    assert_eq!(
        image.resolution_unit,
        acadrust::objects::ResolutionUnit::None
    );
}

#[test]
fn dxf_book_color_presence_names_and_packing_preserve_native_state() {
    for (codes, native_color, index, packed) in [
        ("", Color::ByLayer, 0, -1),
        ("62\n256\n", Color::ByLayer, 256, -1),
        ("62\n42\n", Color::from_index(42), 42, -1),
        ("420\n66051\n", Color::from_rgb(1, 2, 3), -1, 197121),
    ] {
        let source = format!(
            "0\nSECTION\n2\nOBJECTS\n0\nDICTIONARY\n5\n100\n100\nAcDbDictionary\n3\nRAL_CLASSIC$RAL_1006\n350\n101\n0\nDBCOLOR\n5\n101\n330\n100\n100\nAcDbColor\n{codes}0\nENDSEC\n0\nEOF\n"
        );
        let doc = DxfReader::from_reader(Cursor::new(source.into_bytes()))
            .unwrap()
            .read()
            .unwrap();
        let ObjectType::BookColor(native) = doc.objects.get(&Handle::new(0x101)).unwrap() else {
            panic!("book color");
        };
        assert_eq!(native.color, native_color);
        assert!(native.book_name.is_empty());
        assert!(native.color_name.is_empty());
        let mut without_source = native.clone();
        without_source.dxf_color_present = None;
        assert_eq!(native, &without_source);
        without_source.color = Color::from_rgb(9, 8, 7);
        assert_ne!(native, &without_source);
        let dump = dump_document(&doc, "", "dxf");
        let p = &object(&dump, Handle::new(0x101))["props"];
        assert_eq!(p["Name"], "RAL_CLASSIC$RAL_1006");
        assert_eq!(p["BookName"], "RAL_CLASSIC");
        assert_eq!(p["ColorName"], "RAL_1006");
        assert_eq!(p["Color"]["Index"], index);
        assert_eq!(p["Color"]["TrueColor"], packed);
    }
}
