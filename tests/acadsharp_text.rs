// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::{AttributeDefinition, EntityType, Text};
use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};
use serde_json::{json, Value};
use std::io::Cursor;

fn dxf_bytes(version: &str, header: &str, entities: &[u8]) -> CadDocument {
    let mut bytes = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{version}\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n").into_bytes();
    bytes.extend_from_slice(entities);
    bytes.extend_from_slice(b"0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(bytes))
        .unwrap()
        .read()
        .unwrap()
}

fn projected(doc: &CadDocument, kind: &str) -> Value {
    dump_document(doc, "synthetic", "new")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["type"] == kind)
        .unwrap()["props"]
        .clone()
}

fn native_text(doc: &CadDocument) -> &Text {
    doc.entities()
        .find_map(|e| {
            if let EntityType::Text(t) = e {
                Some(t)
            } else {
                None
            }
        })
        .unwrap()
}

#[test]
fn text_defaults_style_and_explicit_alignment_are_projected() {
    let mut doc = CadDocument::new();
    let mut text = Text::with_value("hello", Vector3::new(2.0, 3.0, 4.0));
    text.style = "sTaNdArD".into();
    text.generation_flags = 6;
    doc.add_entity(EntityType::Text(text)).unwrap();
    let p = projected(&doc, "TextEntity");
    assert_eq!(p["AlignmentPoint"], json!([0.0, 0.0, 0.0]));
    assert_eq!(p["Mirror"], 6);
    assert_eq!(
        p["Style"],
        format!(
            "#{:X}",
            doc.text_styles.get("Standard").unwrap().handle.value()
        )
    );
    let read = dxf_bytes("AC1032", "", b"0\nTEXT\n5\n100\n11\n7\n21\n8\n31\n9\n");
    assert_eq!(
        projected(&read, "TextEntity")["AlignmentPoint"],
        json!([7.0, 8.0, 9.0])
    );
}

#[test]
fn undeclared_invalid_utf8_is_replaced_only_in_the_view() {
    let bytes = b"0\nTEXT\n5\n100\n1\n\xD845,6\n";
    let mut doc = dxf_bytes("AC1009", "", bytes);
    assert_eq!(native_text(&doc).value, "Ø45,6");
    assert_eq!(projected(&doc, "TextEntity")["Value"], "�45,6");
    let mut plain = native_text(&doc).clone();
    plain.source = None;
    assert_eq!(&plain, native_text(&doc));
    let h = native_text(&doc).common.handle;
    if let EntityType::Text(t) = doc.get_entity_mut(h).unwrap() {
        t.value = "edited Ø".into();
    }
    assert_eq!(projected(&doc, "TextEntity")["Value"], "edited Ø");

    let declared = dxf_bytes("AC1009", "9\n$DWGCODEPAGE\n3\nANSI_1252\n", bytes);
    assert_eq!(projected(&declared, "TextEntity")["Value"], "Ø45,6");
    let valid = dxf_bytes("AC1009", "", "0\nTEXT\n5\n100\n1\nØ45,6\n".as_bytes());
    assert_eq!(projected(&valid, "TextEntity")["Value"], "Ø45,6");
}

#[test]
fn text_lexical_escapes_are_retained_for_ascii_binary_and_dwg() {
    let doc = dxf_bytes("AC1015", "", b"0\nTEXT\n5\n100\n1\n\\U+00D8^Jx\n");
    assert_eq!(native_text(&doc).value, "Ø\nx");
    assert_eq!(projected(&doc, "TextEntity")["Value"], "\\U+00D8\nx");
    for version in [DxfVersion::AC1015, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        doc.add_entity(EntityType::Text(Text::with_value(
            "\\U+00D8",
            Vector3::ZERO,
        )))
        .unwrap();
        let binary = DxfReader::from_reader(Cursor::new(
            DxfWriter::new_binary(&doc).write_to_vec().unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap();
        assert_eq!(projected(&binary, "TextEntity")["Value"], "\\U+00D8");
        let dwg = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        assert_eq!(projected(&dwg, "TextEntity")["Value"], "\\U+00D8");
    }
}

#[test]
fn attribute_fields_and_dxf_version_lock_assignment_are_separate() {
    for (kind, marker, projected_kind) in [
        ("ATTDEF", "AcDbAttributeDefinition", "AttributeDefinition"),
        ("ATTRIB", "AcDbAttribute", "AttributeEntity"),
    ] {
        let bytes = format!("0\n{kind}\n5\n100\n100\nAcDbEntity\n100\nAcDbText\n39\n2.5\n71\n6\n210\n0\n220\n1\n230\n0\n100\n{marker}\n280\n7\n2\nTAG\n280\n1\n");
        let doc = dxf_bytes("AC1032", "", bytes.as_bytes());
        let p = projected(&doc, projected_kind);
        assert_eq!(p["Height"], 1.0);
        assert_eq!(p["AttributeType"], 1);
        assert_eq!(p["Mirror"], 6);
        assert_eq!(p["Thickness"], 2.5);
        assert_eq!(p["Normal"], json!([0.0, 1.0, 0.0]));
        assert_eq!(p["IsLocked"], false);
        assert_eq!(p["Version"], 1);
        assert_ne!(p["Style"], "!unmapped");
        let (thickness, version, locked) = doc
            .entities()
            .find_map(|e| match e {
                EntityType::AttributeDefinition(a) => {
                    Some((a.thickness, a.version, a.lock_position))
                }
                EntityType::AttributeEntity(a) => Some((a.thickness, a.version, a.lock_position)),
                _ => None,
            })
            .unwrap();
        assert_eq!((thickness, version, locked), (2.5, 7, true));
    }
}

#[test]
fn embedded_multiline_value_is_native_while_view_keeps_outer_text() {
    let doc = dxf_bytes("AC1032", "", b"0\nATTRIB\n5\n100\n100\nAcDbEntity\n100\nAcDbText\n1\nouter\n100\nAcDbAttribute\n2\nTAG\n71\n2\n101\nEmbedded Object\n1\ninner\\Ptext\n");
    let p = projected(&doc, "AttributeEntity");
    assert_eq!(p["AttributeType"], 2);
    assert_eq!(p["MText"], "#0");
    assert_eq!(p["Value"], "outer");
    let native = doc
        .entities()
        .find_map(|e| {
            if let EntityType::AttributeEntity(a) = e {
                Some(a)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(native.value, "inner\\Ptext");
    assert!(native.embedded_mtext.is_some());
}

#[test]
fn native_tag_recovery_does_not_replace_the_source_tag_in_the_view() {
    use acadrust::objects::{Dictionary, ObjectType, XRecord, XRecordEntry, XRecordValue};
    let mut doc = CadDocument::new();
    let h = doc
        .add_entity(EntityType::AttributeDefinition(
            AttributeDefinition::simple("TAG_001"),
        ))
        .unwrap();
    let dh = Handle::new(0x500);
    let rh = Handle::new(0x501);
    let mut dict = Dictionary::new();
    dict.handle = dh;
    dict.owner = h;
    dict.add_entry("ACAD_MLATT", rh);
    let mut record = XRecord::new();
    record.handle = rh;
    record.owner = dh;
    record.entries.push(XRecordEntry {
        code: 2,
        value: XRecordValue::String("TAG".into()),
    });
    doc.objects.insert(dh, ObjectType::Dictionary(dict));
    doc.objects.insert(rh, ObjectType::XRecord(record));
    doc.resolve_xrecord_backed_properties();
    let EntityType::AttributeDefinition(a) = doc.get_entity(h).unwrap() else {
        panic!()
    };
    assert_eq!(a.tag, "TAG");
    assert_eq!(projected(&doc, "AttributeDefinition")["Tag"], "TAG_001");
}

#[test]
fn dwg_keeps_stored_left_baseline_alignment_and_attribute_fields() {
    use acadrust::entities::{AttributeEntity, Insert};
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        let alignment = Vector3::new(7.0, 8.0, 4.0);
        let mut text = Text::with_value("text", Vector3::new(1.0, 2.0, 4.0));
        text.alignment_point = Some(alignment);
        doc.add_entity(EntityType::Text(text)).unwrap();
        let mut definition = AttributeDefinition::simple("TAG");
        definition.insertion_point.z = 4.0;
        definition.alignment_point = alignment;
        definition.thickness = 2.5;
        definition.version = 7;
        definition.lock_position = true;
        let attribute = AttributeEntity::from_definition(&definition, Some("value".into()));
        assert_eq!(attribute.thickness, 2.5);
        assert_eq!(attribute.version, 7);
        doc.add_entity(EntityType::AttributeDefinition(definition))
            .unwrap();
        let mut insert = Insert::new("*Model_Space", Vector3::ZERO);
        insert.attributes.push(attribute);
        doc.add_entity(EntityType::Insert(insert)).unwrap();
        let read = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        assert_eq!(native_text(&read).alignment_point, Some(alignment));
        for kind in ["TextEntity", "AttributeDefinition", "AttributeEntity"] {
            assert_eq!(
                projected(&read, kind)["AlignmentPoint"],
                json!([7.0, 8.0, 4.0])
            );
        }
        for kind in ["AttributeDefinition", "AttributeEntity"] {
            let p = projected(&read, kind);
            assert_eq!(p["Thickness"], 2.5);
            assert_eq!(
                p["Version"],
                if version >= DxfVersion::AC1024 { 7 } else { 0 }
            );
            assert_eq!(p["IsLocked"], version >= DxfVersion::AC1021);
        }
        let dxf = DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap();
        assert_eq!(projected(&dxf, "AttributeDefinition")["Thickness"], 2.5);
        assert_eq!(projected(&dxf, "AttributeEntity")["Thickness"], 2.5);
    }
}
