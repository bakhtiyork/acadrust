// SPDX-License-Identifier: MPL-2.0
//! Focused tests for the R4-C2 corpus fixes: the ACadSharp view of proxy
//! records (class lookup, version fields, retained data) and of FIELD /
//! FIELDLIST objects (child registration, `CadValue` values, value strings).
use super::dump_reader_document;
use crate::classes::{DxfClass, ProxyFlags};
use crate::entities::{
    CellValue, CellValueType, EntityCommon, EntityType, ExtendedEntity, ExtendedEntityData,
    ProxyEntityData,
};
use crate::objects::{
    Dictionary, Field, FieldChildValue, FieldList, ObjectType, ProxyObject, ProxyPayload,
};
use crate::types::{DxfVersion, Handle};
use crate::CadDocument;
use serde_json::Value;

fn find<'a>(dump: &'a Value, h: u64) -> Option<&'a Value> {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{h:X}"))
}

fn proxy_doc(version: DxfVersion) -> CadDocument {
    let mut doc = CadDocument::new();
    doc.version = version;
    doc.classes.clear();
    let mut class = DxfClass::new("AECC_TREE_NODE", "AeccDbTreeNode");
    class.application_name = "AeccvBase10".into();
    class.class_number = 526;
    class.instance_count = 3;
    class.was_zombie = true;
    class.proxy_flags = ProxyFlags(1023);
    doc.classes.push_preserving(class);
    doc
}

fn proxy_object(class_id: i32) -> ProxyObject {
    ProxyObject {
        handle: Handle::new(0x4EDC),
        owner: Handle::new(0xC),
        proxy_id: 499,
        class_id,
        version: 6_815_769,
        dwg_version: 25,
        maintenance_version: 104,
        payload: ProxyPayload::from_bits(&[0xAB], 8),
        ..ProxyObject::default()
    }
}

#[test]
fn proxy_object_resolves_its_class_and_swaps_the_version_fields() {
    let mut doc = proxy_doc(DxfVersion::AC1027);
    doc.objects
        .insert(Handle::new(0x4EDC), ObjectType::ProxyObject(proxy_object(526)));
    let dump = dump_reader_document(&doc, "", "dwg");
    let p = &find(&dump, 0x4EDC).unwrap()["props"];
    assert_eq!(p["ClassId"], 526);
    assert_eq!(p["DxfClass"]["DxfName"], "AECC_TREE_NODE");
    assert_eq!(p["DxfClass"]["ClassNumber"], 526);
    assert_eq!(p["DxfClass"]["ProxyFlags"], 1023);
    // Version is the low word, DrawingFormat the combined value.
    assert_eq!(p["Version"], 25);
    assert_eq!(p["MaintenanceVersion"], 104);
    assert_eq!(p["DrawingFormat"], 6_815_769);
    // The DWG reader keeps the data as a stream and never fills BinaryData.
    assert_eq!(p["Data"], serde_json::json!({}));
    assert_eq!(p["BinaryData"], Value::Null);
}

#[test]
fn proxy_object_without_a_class_throws_on_class_id() {
    let mut doc = proxy_doc(DxfVersion::AC1027);
    doc.objects
        .insert(Handle::new(0x4EDC), ObjectType::ProxyObject(proxy_object(999)));
    let dump = dump_reader_document(&doc, "", "dwg");
    let p = &find(&dump, 0x4EDC).unwrap()["props"];
    assert_eq!(p["ClassId"], "!throws:NullReferenceException");
    assert_eq!(p["DxfClass"], Value::Null);
}

#[test]
fn proxy_data_stream_needs_r2010_and_version_fields_need_r2000() {
    let mut doc = proxy_doc(DxfVersion::AC1018);
    doc.objects
        .insert(Handle::new(0x4EDC), ObjectType::ProxyObject(proxy_object(526)));
    let dump = dump_reader_document(&doc, "", "dwg");
    let p = &find(&dump, 0x4EDC).unwrap()["props"];
    assert_eq!(p["Data"], Value::Null, "handle section offset is only known from R2010");
    assert_eq!(p["Version"], 25);

    let mut doc = proxy_doc(DxfVersion::AC1014);
    doc.objects
        .insert(Handle::new(0x4EDC), ObjectType::ProxyObject(proxy_object(526)));
    let dump = dump_reader_document(&doc, "", "dwg");
    let p = &find(&dump, 0x4EDC).unwrap()["props"];
    assert_eq!(p["Version"], 0);
    assert_eq!(p["MaintenanceVersion"], 0);
    assert_eq!(p["DrawingFormat"], 0);
    assert_eq!(p["OriginalDataFormatDxf"], false);
}

#[test]
fn proxy_entity_shares_the_proxy_view() {
    let mut doc = proxy_doc(DxfVersion::AC1027);
    let data = ProxyEntityData {
        proxy_id: 498,
        class_id: 526,
        dxf_subclass: String::new(),
        version: 6_815_769,
        dwg_version: 25,
        maintenance_version: 104,
        from_dxf: false,
        graphics: ProxyPayload::default(),
        payload: ProxyPayload::from_bits(&[0x40], 8),
        text_payload: ProxyPayload::default(),
        object_ids: Vec::new(),
    };
    let handle = doc
        .add_entity(EntityType::Extended(Box::new(ExtendedEntity {
            common: EntityCommon::new(),
            data: ExtendedEntityData::Proxy(data),
        })))
        .unwrap();
    let dump = dump_reader_document(&doc, "", "dwg");
    let p = &find(&dump, handle.value()).unwrap()["props"];
    assert_eq!(p["ClassId"], 526);
    assert_eq!(p["DxfClass"]["CppClassName"], "AeccDbTreeNode");
    assert_eq!(p["Version"], 25);
    assert_eq!(p["DrawingFormat"], 6_815_769);
    assert_eq!(p["Data"], serde_json::json!({}));
    assert_eq!(p["ProxyClassId"], 498);
}

fn text_value(text: &str, flags: i32) -> CellValue {
    let mut value = CellValue::new();
    value.value_type = CellValueType::String;
    value.raw_type_code = 4;
    value.flags = flags;
    value.text = text.into();
    value
}

/// Root -> ACAD_FIELD -> TEXT (container field #12) -> child field #13.
fn field_doc(version: DxfVersion) -> CadDocument {
    let mut doc = CadDocument::new();
    doc.version = version;
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x2000);
    for (h, name, child) in [(0x2000, "ACAD_FIELD", 0x2001), (0x2001, "TEXT", 0x2002)] {
        let mut d = Dictionary::new();
        d.handle = Handle::new(h);
        d.add_entry(name, Handle::new(child));
        doc.objects.insert(d.handle, ObjectType::Dictionary(d));
    }
    let container = Field {
        handle: Handle::new(0x2002),
        owner: Handle::new(0x2001),
        evaluator_id: "_text".into(),
        code: "%<\\_FldIdx 0>%".into(),
        child_fields: vec![Handle::new(0x2003)],
        value: CellValue::new(),
        child_values: vec![FieldChildValue {
            key: "ACFD_FIELDTEXT_CHECKSUM".into(),
            value: {
                let mut v = CellValue::new();
                v.value_type = CellValueType::Double;
                v.raw_type_code = 2;
                v.numeric_value = 89514.0;
                v
            },
        }],
        ..Field::default()
    };
    let child = Field {
        handle: Handle::new(0x2003),
        owner: Handle::new(0x2002),
        evaluator_id: "AcVar".into(),
        code: "\\AcVar Title".into(),
        referenced_objects: vec![Handle::new(0x2000), Handle::new(0x9999)],
        value: text_value("fox", 0),
        value_string: "The quick fox".into(),
        // Older format string, overwritten by the value string in the reference.
        format: "%tc1".into(),
        ..Field::default()
    };
    doc.objects.insert(container.handle, ObjectType::Field(container));
    doc.objects.insert(child.handle, ObjectType::Field(child));
    doc
}

#[test]
fn child_fields_are_registered_and_cad_objects_hold_only_references() {
    let doc = field_doc(DxfVersion::AC1018);
    let dump = dump_reader_document(&doc, "", "dwg");
    let container = find(&dump, 0x2002).expect("container field");
    assert_eq!(container["props"]["Name"], "TEXT");
    assert_eq!(container["props"]["Children"], serde_json::json!(["#2003"]));
    assert_eq!(container["props"]["CadObjects"], serde_json::json!([]));
    let child = find(&dump, 0x2003).expect("child field is registered through Children");
    assert_eq!(child["owner"], "#2002");
    assert_eq!(child["props"]["Name"], "");
    // #9999 does not resolve, so the template drops it.
    assert_eq!(child["props"]["CadObjects"], serde_json::json!(["#2000"]));
    assert_eq!(child["props"]["FormatString"], "The quick fox");
}

#[test]
fn field_values_are_cad_values_with_the_reference_string_terminator() {
    let doc = field_doc(DxfVersion::AC1018);
    let dump = dump_reader_document(&doc, "", "dwg");
    let container = &find(&dump, 0x2002).unwrap()["props"];
    // An unset value reads as the Unknown/BL default.
    assert_eq!(container["Value"]["ValueType"], 0);
    assert_eq!(container["Value"]["Value"], 0);
    assert_eq!(container["Value"]["Format"], Value::Null);
    assert_eq!(container["Values"][0][0], "ACFD_FIELDTEXT_CHECKSUM");
    assert_eq!(container["Values"][0][1]["Value"], 89514.0);
    assert_eq!(container["Values"][0][1]["ValueType"], 2);
    // Before R2007 the decoded bytes keep the stored NUL.
    let child = &find(&dump, 0x2003).unwrap()["props"];
    assert_eq!(child["Value"]["Value"], "fox\u{0}");

    let doc = field_doc(DxfVersion::AC1021);
    let dump = dump_reader_document(&doc, "", "dwg");
    let child = &find(&dump, 0x2003).unwrap()["props"];
    assert_eq!(child["Value"]["Value"], "fox");
    assert_eq!(child["Value"]["Format"], "");
}

#[test]
fn empty_flagged_value_has_no_body() {
    let mut doc = field_doc(DxfVersion::AC1021);
    if let Some(ObjectType::Field(child)) = doc.objects.get_mut(&Handle::new(0x2003)) {
        child.value = text_value("ignored", 1);
    }
    let dump = dump_reader_document(&doc, "", "dwg");
    let child = &find(&dump, 0x2003).unwrap()["props"];
    assert_eq!(child["Value"]["Value"], Value::Null);
    assert_eq!(child["Value"]["Flags"], 1);
}

#[test]
fn field_list_keeps_each_resolved_field_once() {
    let mut doc = field_doc(DxfVersion::AC1018);
    let list = FieldList {
        handle: Handle::new(0x2004),
        owner: Handle::new(0x2000),
        fields: vec![
            Handle::new(0x2002),
            Handle::new(0x2002),
            Handle::new(0x2077),
            Handle::new(0x2000),
            Handle::new(0x2003),
        ],
        ..FieldList::default()
    };
    doc.objects.insert(list.handle, ObjectType::FieldList(list));
    if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&Handle::new(0x2000)) {
        root.add_entry("ACAD_FIELDLIST", Handle::new(0x2004));
    }
    let dump = dump_reader_document(&doc, "", "dwg");
    assert_eq!(
        find(&dump, 0x2004).unwrap()["props"]["Fields"],
        serde_json::json!(["#2002", "#2003"])
    );
}

#[test]
fn dxf_field_keeps_the_source_value_string_and_the_full_field_code() {
    use crate::io::dxf::DxfReader;
    use std::io::Cursor;
    let text = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n\
        0\nSECTION\n2\nOBJECTS\n\
        0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nTEXT\n350\n20\n0\nFIELD\n5\n20\n330\n0\n100\nAcDbField\n1\n_text\n3\nab\n3\ncd\n2\nef\n90\n0\n97\n0\n\
        91\n63\n92\n0\n94\n0\n95\n0\n96\n0\n300\n\n93\n0\n\
        7\nACFD_FIELD_VALUE\n93\n0\n90\n4\n1\nv\n94\n0\n300\n\n302\n\n304\nACVALUE_END\n\
        301\nx^Iy\n98\n3\n0\nENDSEC\n0\nEOF\n";
    let doc = DxfReader::from_reader(Cursor::new(text.as_bytes())).unwrap().read().unwrap();
    let Some(ObjectType::Field(field)) = doc.objects.get(&Handle::new(0x20)) else {
        panic!("field");
    };
    // The native model keeps the decoded text and the complete field code.
    assert_eq!(field.value_string, "x\ty");
    assert_eq!(field.code, "abcdef");
    assert_eq!(field.value_string_source.0.as_deref(), Some("x^Iy"));
    let dump = dump_reader_document(&doc, "", "dxf");
    let props = &find(&dump, 0x20).unwrap()["props"];
    assert_eq!(props["FormatString"], "x^Iy");
    assert_eq!(props["Value"]["Value"], "v");
    assert_eq!(props["Value"]["Format"], "");
}
