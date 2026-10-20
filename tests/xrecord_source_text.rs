// SPDX-License-Identifier: MPL-2.0
use acadrust::objects::{ObjectType, XRecordValue};
use acadrust::{DxfReader, Handle};
use std::io::Cursor;

fn xrecord(document: &acadrust::CadDocument) -> &acadrust::objects::XRecord {
    match document.objects.get(&Handle::new(0xA1)) {
        Some(ObjectType::XRecord(record)) => record,
        _ => panic!("XRecord A1 missing"),
    }
}

#[cfg(feature = "acadsharp-compat")]
fn compat_xrecord_entry_value(document: &acadrust::CadDocument, index: usize) -> serde_json::Value {
    let dump = acadrust::compat::acadsharp::dump_document(document, "synthetic.dxf", "dxf");
    dump["objects"]
        .as_array()
        .expect("dump objects")
        .iter()
        .find(|object| object["handle"] == serde_json::json!("#A1"))
        .expect("XRecord A1 dump")
        .get("props")
        .and_then(|props| props.get("Entries"))
        .and_then(serde_json::Value::as_array)
        .and_then(|entries| entries.get(index))
        .and_then(|entry| entry.get("Value"))
        .cloned()
        .expect("XRecord entry value")
}

const ASCII_DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nXRECORD\n5\nA1\n330\n0\n100\nAcDbXrecord\n280\n0\n1\ncaret^Jlegacy\\U+00E9\n2\nplain\n0\nENDSEC\n0\nEOF\n";

fn push_binary_string(bytes: &mut Vec<u8>, code: i16, value: &[u8]) {
    bytes.extend_from_slice(&code.to_le_bytes());
    bytes.extend_from_slice(value);
    bytes.push(0);
}

fn push_binary_i16(bytes: &mut Vec<u8>, code: i16, value: i16) {
    bytes.extend_from_slice(&code.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn binary_dxf() -> Vec<u8> {
    let mut bytes = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
    push_binary_string(&mut bytes, 0, b"SECTION");
    push_binary_string(&mut bytes, 2, b"HEADER");
    push_binary_string(&mut bytes, 9, b"$ACADVER");
    push_binary_string(&mut bytes, 1, b"AC1021");
    push_binary_string(&mut bytes, 0, b"ENDSEC");
    push_binary_string(&mut bytes, 0, b"SECTION");
    push_binary_string(&mut bytes, 2, b"OBJECTS");
    push_binary_string(&mut bytes, 0, b"XRECORD");
    push_binary_string(&mut bytes, 5, b"A1");
    push_binary_string(&mut bytes, 330, b"0");
    push_binary_string(&mut bytes, 100, b"AcDbXrecord");
    push_binary_i16(&mut bytes, 280, 0);
    push_binary_string(&mut bytes, 1, b"binary\\U+00E9");
    push_binary_string(&mut bytes, 2, b"plain");
    push_binary_string(&mut bytes, 0, b"ENDSEC");
    push_binary_string(&mut bytes, 0, b"EOF");
    bytes
}

#[test]
fn ascii_xrecord_retains_caret_and_legacy_source_spelling() {
    let mut document = DxfReader::from_reader(Cursor::new(ASCII_DXF.as_bytes()))
        .expect("reader")
        .read()
        .expect("document");
    let record = xrecord(&document);

    assert_eq!(record.entries[0].code, 1);
    assert_eq!(record.entries[0].value.as_string(), Some("caret\nlegacyé"));
    assert_eq!(record.source_value(0), Some("caret^Jlegacy\\U+00E9"));
    assert_eq!(record.source_value(1), None);

    #[cfg(feature = "acadsharp-compat")]
    assert_eq!(
        compat_xrecord_entry_value(&document, 0),
        serde_json::json!("caret^Jlegacy\\U+00E9")
    );

    let record = match document.objects.get_mut(&Handle::new(0xA1)) {
        Some(ObjectType::XRecord(record)) => record,
        _ => panic!("XRecord A1 missing"),
    };
    record.entries[0].value = XRecordValue::String("edited".to_string());
    assert_eq!(record.source_value(0), None);

    #[cfg(feature = "acadsharp-compat")]
    assert_eq!(
        compat_xrecord_entry_value(&document, 0),
        serde_json::json!("edited")
    );
}

#[test]
fn binary_xrecord_retains_legacy_source_spelling() {
    let mut document = DxfReader::from_reader(Cursor::new(binary_dxf()))
        .expect("reader")
        .read()
        .expect("document");
    let record = xrecord(&document);

    assert_eq!(record.entries[0].value.as_string(), Some("binaryé"));
    assert_eq!(record.source_value(0), Some("binary\\U+00E9"));

    #[cfg(feature = "acadsharp-compat")]
    assert_eq!(
        compat_xrecord_entry_value(&document, 0),
        serde_json::json!("binary\\U+00E9")
    );

    let record = match document.objects.get_mut(&Handle::new(0xA1)) {
        Some(ObjectType::XRecord(record)) => record,
        _ => panic!("XRecord A1 missing"),
    };
    record.entries[0].value = XRecordValue::String("edited".to_string());
    assert_eq!(record.source_value(0), None);

    #[cfg(feature = "acadsharp-compat")]
    assert_eq!(
        compat_xrecord_entry_value(&document, 0),
        serde_json::json!("edited")
    );
}
