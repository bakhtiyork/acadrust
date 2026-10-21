// SPDX-License-Identifier: MPL-2.0
use super::super::dump_reader_document as dump_document;
use crate::objects::{Dictionary, ObjectType, VisualStyle, XRecord};
use crate::types::Handle;
use crate::CadDocument;
use serde_json::{json, Value};

fn dict(doc: &mut CadDocument, h: u64, entries: &[(&str, u64)]) {
    let mut d = Dictionary::new();
    d.handle = Handle::new(h);
    for (name, child) in entries {
        d.add_entry(*name, Handle::new(*child));
    }
    doc.objects.insert(d.handle, ObjectType::Dictionary(d));
}
fn xrecord(doc: &mut CadDocument, h: u64, owner: u64) {
    let mut x = XRecord::new();
    x.handle = Handle::new(h);
    x.owner = Handle::new(owner);
    doc.objects.insert(x.handle, ObjectType::XRecord(x));
}
fn objects(doc: &CadDocument, reader: &str) -> Vec<Value> {
    dump_document(doc, "synthetic", reader)["objects"]
        .as_array()
        .unwrap()
        .clone()
}
fn object(objects: &[Value], h: u64) -> &Value {
    objects
        .iter()
        .find(|o| o["handle"] == format!("#{h:X}"))
        .unwrap()
}

#[test]
fn read_registration_follows_entries_and_attachments_not_raw_owners() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x100);
    dict(&mut doc, 0x100, &[("child", 0x101)]);
    dict(&mut doc, 0x101, &[("payload", 0x102)]);
    xrecord(&mut doc, 0x102, 0x999);
    // An owner field alone does not insert an object into a collection.
    xrecord(&mut doc, 0x103, 0x101);
    dict(&mut doc, 0x104, &[("extension payload", 0x105)]);
    xrecord(&mut doc, 0x105, 0);
    if let ObjectType::XRecord(x) = doc.objects.get_mut(&Handle::new(0x102)).unwrap() {
        x.xdictionary_handle = Some(Handle::new(0x104));
    }
    let before = doc.objects.clone();
    let result = objects(&doc, "dwg");
    assert_eq!(doc.objects, before);
    assert_eq!(object(&result, 0x100)["props"]["Name"], "");
    assert_eq!(object(&result, 0x102)["owner"], "#101");
    assert_eq!(object(&result, 0x102)["props"]["Name"], "payload");
    assert_eq!(object(&result, 0x104)["owner"], "#102");
    assert_eq!(object(&result, 0x105)["owner"], "#104");
    assert!(!result.iter().any(|o| o["handle"] == "#103"));
}

#[test]
fn dxf_visual_style_callback_and_unknown_policy_prune_owned_subgraphs() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x100);
    dict(
        &mut doc,
        0x100,
        &[("style", 0x101), ("unknown", 0x104), ("retained", 0x107)],
    );
    let mut style = VisualStyle::new();
    style.handle = Handle::new(0x101);
    doc.objects
        .insert(style.handle, ObjectType::VisualStyle(style));
    doc.objects.insert(
        Handle::new(0x104),
        ObjectType::Unknown {
            type_name: "UNSUPPORTED_APPLICATION_OBJECT".into(),
            handle: Handle::new(0x104),
            owner: Handle::new(0x100),
            raw_dxf_codes: None,
            raw_dwg_data: None,
            raw_dwg_handle_bits: 0,
            raw_dwg_version: None,
        },
    );
    for (owner, d, x) in [(0x101, 0x102, 0x103), (0x104, 0x105, 0x106)] {
        dict(&mut doc, d, &[("payload", x)]);
        xrecord(&mut doc, x, d);
        doc.xdic_by_handle
            .insert(Handle::new(owner), Handle::new(d));
    }
    xrecord(&mut doc, 0x107, 0x100);
    let dxf = objects(&doc, "dxf");
    assert_eq!(
        object(&dxf, 0x100)["props"]["EntryHandles"],
        // The reference creates all eleven missing collection dictionaries.
        json!([0x107, 0x1000, 0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008, 0x1009, 0x100A])
    );
    assert_eq!(
        object(&dxf, 0x100)["props"]["EntryNames"],
        json!([
            "retained",
            "ACAD_LAYOUT",
            "ACAD_GROUP",
            "ACAD_SCALELIST",
            "ACAD_MLINESTYLE",
            "ACAD_MLEADERSTYLE",
            "ACAD_TABLESTYLE",
            "ACAD_IMAGE_DICT",
            "ACAD_PDFDEFINITIONS",
            "ACAD_COLOR",
            "AcDbVariableDictionary",
            "ACAD_MATERIAL"
        ])
    );
    for h in 0x101..=0x106 {
        assert!(!dxf.iter().any(|o| o["handle"] == format!("#{h:X}")));
    }
    let dwg = objects(&doc, "dwg");
    assert_eq!(object(&dwg, 0x101)["type"], "VisualStyle");
    assert_eq!(object(&dwg, 0x103)["type"], "XRecord");
    assert!(!dwg.iter().any(|o| o["handle"] == "#104"));
}

#[test]
fn style_description_is_not_a_dictionary_identity() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x100);
    dict(
        &mut doc,
        0x100,
        &[("Standard", 0x101), ("Annotative", 0x102)],
    );
    for h in [0x101, 0x102] {
        let mut style = crate::objects::MultiLeaderStyle::new("Standard");
        style.handle = Handle::new(h);
        style.owner_handle = Handle::new(0x100);
        doc.objects
            .insert(style.handle, ObjectType::MultiLeaderStyle(style));
    }
    let result = objects(&doc, "dxf");
    assert_eq!(object(&result, 0x102)["props"]["Name"], "Annotative");
    assert_eq!(
        object(&result, 0x100)["props"]["EntryHandles"],
        json!([0x101, 0x102, 0x1000, 0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x1007, 0x1008, 0x1009, 0x100A])
    );
}

#[test]
fn source_appids_distinguish_writer_defaults_from_explicit_same_named_entries() {
    use std::io::Cursor;
    for explicit in [false, true] {
        let entry = if explicit {
            "0\nAPPID\n5\n120\n2\nAcCmTransparency\n70\n0\n"
        } else {
            ""
        };
        let dxf = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n9\n$HANDSEED\n5\n200\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nAPPID\n5\n9\n70\n1\n0\nAPPID\n5\n12\n2\nACAD\n70\n0\n{entry}0\nENDTAB\n0\nENDSEC\n0\nEOF\n");
        let doc = crate::DxfReader::from_reader(Cursor::new(dxf.into_bytes()))
            .unwrap()
            .read()
            .unwrap();
        assert!(
            doc.app_ids.get("AcCmTransparency").is_some(),
            "native defaults survive"
        );
        let result = objects(&doc, "dxf");
        let appids: Vec<_> = result.iter().filter(|o| o["type"] == "AppId").collect();
        assert_eq!(appids.len(), if explicit { 2 } else { 1 });
        assert_eq!(object(&result, 9)["props"]["Count"], appids.len());
        if explicit {
            assert_eq!(object(&result, 0x120)["props"]["Name"], "AcCmTransparency");
        }
    }
}

#[test]
fn xrecord_references_use_template_policy_without_coercing_literal_strings() {
    use crate::objects::{XRecordEntry, XRecordValue};
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x100);
    dict(&mut doc, 0x100, &[("record", 0x101)]);
    xrecord(&mut doc, 0x101, 0x100);
    // A known orphan is in the builder's template map but not the registry.
    xrecord(&mut doc, 0x102, 0);
    let mut style = VisualStyle::new();
    style.handle = Handle::new(0x103);
    doc.objects
        .insert(style.handle, ObjectType::VisualStyle(style));
    if let ObjectType::XRecord(x) = doc.objects.get_mut(&Handle::new(0x101)).unwrap() {
        x.entries = vec![
            XRecordEntry::new(360, XRecordValue::Handle(Handle::new(0x102))),
            XRecordEntry::new(360, XRecordValue::Handle(Handle::new(0x103))),
            XRecordEntry::new(1, XRecordValue::String("#103".into())),
        ];
    }
    let result = objects(&doc, "dxf");
    let entries = &object(&result, 0x101)["props"]["Entries"];
    assert_eq!(entries[0]["Value"], "#102");
    assert_eq!(entries[1]["Value"], 0x103);
    assert_eq!(entries[2]["Value"], "#103");
    assert!(!result.iter().any(|o| o["handle"] == "#102"));
}

#[test]
fn table_extension_dictionary_is_registered_but_not_counted_as_an_entry() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    let table = doc.layers.handle();
    let count = doc.layers.len();
    dict(&mut doc, 0x100, &[]);
    if let ObjectType::Dictionary(d) = doc.objects.get_mut(&Handle::new(0x100)).unwrap() {
        d.owner = table;
    }
    doc.xdic_by_handle.insert(table, Handle::new(0x100));
    let result = objects(&doc, "dwg");
    assert_eq!(object(&result, table.value())["props"]["Count"], count);
    assert_eq!(
        object(&result, 0x100)["owner"],
        crate::compat::acadsharp::handle(table)
    );
}

#[test]
fn an_explicit_extension_pointer_wins_over_an_unlinked_owner_pointer() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    let table = doc.layers.handle();
    for h in [0x100, 0x101] {
        dict(&mut doc, h, &[]);
        if let ObjectType::Dictionary(d) = doc.objects.get_mut(&Handle::new(h)).unwrap() {
            d.owner = table;
        }
    }
    doc.xdic_by_handle.insert(table, Handle::new(0x100));
    let result = objects(&doc, "dwg");
    assert_eq!(object(&result, 0x100)["type"], "CadDictionary");
    assert!(!result.iter().any(|o| o["handle"] == "#101"));
}

#[test]
fn anonymous_extension_dictionaries_have_empty_names_and_named_children_keep_order() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    let table = doc.layers.handle();
    dict(&mut doc, 0x100, &[("second", 0x102), ("first", 0x101)]);
    dict(&mut doc, 0x101, &[]);
    dict(&mut doc, 0x102, &[]);
    doc.xdic_by_handle.insert(table, Handle::new(0x100));
    let result = objects(&doc, "dwg");
    assert_eq!(object(&result, 0x100)["props"]["Name"], "");
    assert_eq!(object(&result, 0x101)["props"]["Name"], "first");
    assert_eq!(object(&result, 0x102)["props"]["Name"], "second");
    assert_eq!(
        object(&result, 0x100)["props"]["EntryNames"],
        json!(["second", "first"])
    );
    assert_eq!(
        object(&result, 0x100)["props"]["EntryHandles"],
        json!([0x102, 0x101])
    );
}

#[test]
fn pre_r2000_dwg_dictionary_flags_are_reference_defaults_only() {
    use crate::objects::DictionaryWithDefault;
    use crate::types::DxfVersion;

    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x100);
    dict(&mut doc, 0x100, &[("default", 0x101)]);
    if let ObjectType::Dictionary(d) = doc.objects.get_mut(&Handle::new(0x100)).unwrap() {
        d.hard_owner = true;
        d.duplicate_cloning = 2;
    }
    let mut default = DictionaryWithDefault::new();
    default.handle = Handle::new(0x101);
    default.hard_owner = true;
    default.duplicate_cloning = 1;
    doc.objects
        .insert(default.handle, ObjectType::DictionaryWithDefault(default));
    let native = doc.objects.clone();
    for (version, reader, projected) in [
        (DxfVersion::AC1014, "dwg", true),
        (DxfVersion::AC1015, "dwg", false),
        (DxfVersion::AC1014, "dxf", false),
    ] {
        doc.version = version;
        let result = objects(&doc, reader);
        for (h, cloning) in [(0x100, 2), (0x101, 1)] {
            let props = &object(&result, h)["props"];
            assert_eq!(props["HardOwnerFlag"], !projected);
            assert_eq!(props["ClonningFlags"], if projected { 0 } else { cloning });
        }
        assert_eq!(doc.objects, native);
    }
}
