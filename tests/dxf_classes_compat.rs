// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use std::io::Cursor;

use acadrust::classes::DxfClass;
use acadrust::compat::acadsharp::dump_document;
use acadrust::CadDocument;
use acadrust::DxfReader;

const CLASSES_DXF: &[u8] = br#"0
SECTION
2
HEADER
9
$ACADVER
1
AC1018
0
ENDSEC
0
SECTION
2
CLASSES
0
CLASS
1
LAYOUT
2
AcDbSourceLayout
3
First application
90
7
91
2
280
1
281
1
0
CLASS
1
SECOND_CLASS
2
AcDbSecondClass
3
Second application
90
16
91
4
280
0
281
0
0
ENDSEC
0
EOF
"#;

const NO_CLASSES_DXF: &[u8] = br#"0
SECTION
2
HEADER
9
$ACADVER
1
AC1018
0
ENDSEC
0
EOF
"#;

const DUPLICATE_CLASSES_DXF: &[u8] = br#"0
SECTION
2
HEADER
9
$ACADVER
1
AC1018
0
ENDSEC
0
SECTION
2
CLASSES
0
CLASS
1
DUPLICATE_CLASS
2
AcDbFirstDuplicate
3
First application
90
7
91
2
280
1
281
1
0
CLASS
1
duplicate_class
2
AcDbSecondDuplicate
3
Second application
90
16
91
4
280
0
281
0
0
ENDSEC
0
EOF
"#;

#[test]
fn compatibility_dxf_classes_preserve_source_metadata() {
    let document = DxfReader::from_reader(Cursor::new(CLASSES_DXF))
        .unwrap()
        .read()
        .unwrap();

    // The native registry retains defaults, while the compatibility view
    // projects the source-only ACadSharp class collection.
    assert!(document.classes.len() > 2);
    let value = dump_document(&document, "", "dxf");
    let classes = value["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 2);
    assert_eq!(classes[0]["ClassNumber"], 500);
    assert_eq!(
        document.classes.source_classes().unwrap()[0].class_number,
        0
    );
    assert_eq!(classes[0]["CppClassName"], "AcDbSourceLayout");
    assert_eq!(classes[0]["ApplicationName"], "First application");
    assert_eq!(classes[0]["ProxyFlags"], 7);
    assert_eq!(classes[0]["InstanceCount"], 2);
    assert_eq!(classes[0]["WasZombie"], true);
    assert_ne!(
        document
            .classes
            .get_by_name("LAYOUT")
            .unwrap()
            .cpp_class_name,
        "AcDbSourceLayout"
    );
    assert_eq!(classes[1]["ClassNumber"], 500);
    assert_eq!(classes[1]["ProxyFlags"], 16);
    assert_eq!(classes[1]["InstanceCount"], 4);
    assert_eq!(classes[1]["WasZombie"], false);
}

#[test]
fn native_dxf_reader_keeps_writer_class_registry() {
    let document = DxfReader::from_reader(Cursor::new(CLASSES_DXF))
        .unwrap()
        .read()
        .unwrap();

    assert!(document.classes.len() > 2);
    assert!(document.classes.get_by_name("LAYOUT").unwrap().class_number > 500);
}

#[test]
fn compatibility_dxf_without_classes_does_not_emit_native_defaults() {
    let document = DxfReader::from_reader(Cursor::new(NO_CLASSES_DXF))
        .unwrap()
        .read()
        .unwrap();

    // CadDocument keeps its native writer registry, but an absent source
    // CLASSES section must remain absent in the compatibility projection.
    assert!(document.classes.len() > 2);
    assert!(dump_document(&document, "", "dxf")["classes"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn compatibility_dxf_duplicate_names_keep_first_source_record() {
    let document = DxfReader::from_reader(Cursor::new(DUPLICATE_CLASSES_DXF))
        .unwrap()
        .read()
        .unwrap();
    let classes = dump_document(&document, "", "dxf")["classes"]
        .as_array()
        .unwrap()
        .to_vec();

    // ACadSharp's dictionary-backed collection keeps the first record when a
    // malformed source repeats a name with different casing.
    assert_eq!(document.classes.source_classes().unwrap().len(), 2);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["DxfName"], "DUPLICATE_CLASS");
    assert_eq!(classes[0]["CppClassName"], "AcDbFirstDuplicate");
    assert_eq!(classes[0]["ApplicationName"], "First application");
    assert_eq!(classes[0]["InstanceCount"], 2);
}

#[test]
fn dwg_view_keeps_first_name_without_changing_native_positions() {
    let mut document = CadDocument::new();
    document.classes.clear();

    let mut first = DxfClass::new("DUPLICATE_CLASS", "AcDbFirstDuplicate");
    first.class_number = 500;
    let mut second = DxfClass::new("DUPLICATE_CLASS", "AcDbSecondDuplicate");
    second.class_number = 501;
    document.classes.push_preserving(first);
    document.classes.push_preserving(second);

    let value = dump_document(&document, "", "dwg");
    let classes = value["classes"].as_array().unwrap();
    // The native DWG collection remains positional for decoding and writing,
    // while the ACadSharp compatibility collection is name-keyed.
    assert_eq!(document.classes.len(), 2);
    let native: Vec<_> = document.classes.iter().collect();
    assert_eq!(native[0].class_number, 500);
    assert_eq!(native[0].cpp_class_name, "AcDbFirstDuplicate");
    assert_eq!(native[1].class_number, 501);
    assert_eq!(native[1].cpp_class_name, "AcDbSecondDuplicate");
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0]["ClassNumber"], 500);
    assert_eq!(classes[0]["CppClassName"], "AcDbFirstDuplicate");
}
