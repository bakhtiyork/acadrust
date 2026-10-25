// SPDX-License-Identifier: MPL-2.0
//! R5b-10 (gate G5-RR and the sample_V cells): the visual-style round-trip records a reader takes out of the model, the records a
//! writer makes stated as AutoCAD states them, and the native dump's note of the handles a reader consumed. Synthetic documents
//! only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing (sample_AC1014.dwg to
//! sample_AC1032.dwg; the C# dumps under Tests/CadCoreTests/Fixtures/dumps).

use std::collections::HashMap;
use std::io::Cursor;

use acadrust::objects::{DictionaryCloningFlags, ObjectType, VisualStyle, VisualStyleProperty, VisualStylePropertyValue, XRecord};
use acadrust::types::{DxfVersion, Handle};
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

/// The native dump of a document by key.
fn dump(document: &CadDocument) -> HashMap<String, serde_json::Value> {
    let mut bytes = Vec::new();
    acadrust::native_dump::write_native_dump(document, &mut bytes).expect("native dump");
    String::from_utf8(bytes)
        .unwrap()
        .lines()
        .map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).unwrap();
            (record["k"].as_str().unwrap().to_string(), record["v"].clone())
        })
        .collect()
}

fn reactors_of(dump: &HashMap<String, serde_json::Value>, handle: Handle) -> Vec<u64> {
    dump.get(&format!("reactors/{:016X}", handle.value()))
        .and_then(|value| value.as_array())
        .map(|list| list.iter().filter_map(|value| value.as_u64()).collect())
        .unwrap_or_default()
}

fn root_entry(document: &CadDocument, name: &str) -> Handle {
    match document.objects.get(&document.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.get(name).unwrap_or_else(|| panic!("no {name} in the root dictionary")),
        _ => panic!("no root dictionary"),
    }
}

/// A drawing read from an R2018 DWG with one visual style (58 properties) whose property 40 (after the 28th) is 7.
fn source_with_visual_style() -> (CadDocument, Handle) {
    let mut document = CadDocument::new();
    let styles = root_entry(&document, "ACAD_VISUALSTYLE");
    let mut style = VisualStyle::new();
    style.handle = document.allocate_handle();
    style.owner = styles;
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::VisualStyle(style));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&styles) {
        entries.add_entry("MYSTYLE", handle);
    }
    let mut source = write_and_read(&document, DxfVersion::AC1032);
    let handle = style_handle(&source);
    let Some(ObjectType::VisualStyle(style)) = source.objects.get_mut(&handle) else { unreachable!() };
    assert_eq!(style.properties.len(), 58);
    style.properties[40] = VisualStyleProperty { value: VisualStylePropertyValue::Long(7), enabled: 0 };
    (source, handle)
}

fn style_handle(document: &CadDocument) -> Handle {
    document
        .objects
        .iter()
        .find_map(|(handle, object)| matches!(object, ObjectType::VisualStyle(style) if style.description.is_empty()).then_some(*handle))
        .expect("the visual style")
}

fn style_of(document: &CadDocument) -> &VisualStyle {
    match document.objects.get(&style_handle(document)) {
        Some(ObjectType::VisualStyle(style)) => style,
        _ => unreachable!(),
    }
}

/// The extension dictionary and the round-trip record of the style, as the file held them.
fn held_record(document: &CadDocument) -> Option<(Handle, XRecord, Handle)> {
    let held = document.with_round_trip_records();
    let style = style_of(&held);
    let dictionary = style.xdictionary_handle?;
    let Some(ObjectType::Dictionary(entries)) = held.objects.get(&dictionary) else { return None };
    let record = entries.get("ACAD_XREC_ROUNDTRIP")?;
    match held.objects.get(&record) {
        Some(ObjectType::XRecord(xrecord)) => Some((dictionary, xrecord.clone(), record)),
        _ => None,
    }
}

fn has_rtvs_record(document: &CadDocument) -> bool {
    document.objects.values().any(|object| {
        matches!(object, ObjectType::XRecord(record) if record.entries.iter().any(|entry| entry.value.as_string().is_some_and(|text| text.starts_with("RTVS"))))
    })
}

/// AutoCAD's files from R14 to R2010 keep the properties of a visual style after the 24th (R2010: the 28th) in its round-trip
/// record, which the reader applies; the record and the extension dictionary it leaves empty leave the model (the file's way of
/// holding the properties is not CAD data), and the ACadSharp view and the writers put them back.
#[test]
fn a_visual_style_record_is_taken_out_of_the_model_when_the_reader_applied_it() {
    let (source, _) = source_with_visual_style();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
        let out = write_and_read(&source, version);
        let style = style_of(&out);
        assert_eq!(style.properties.len(), 58, "{version:?}: restored from the record");
        assert_eq!(style.properties[40], VisualStyleProperty { value: VisualStylePropertyValue::Long(7), enabled: 0 }, "{version:?}");
        assert!(style.xdictionary_handle.is_none(), "{version:?}: the extension dictionary the record left empty is gone");
        assert!(out.extension_dictionary_handle(style_handle(&out)).is_none(), "{version:?}");
        assert!(!has_rtvs_record(&out), "{version:?}: the record is out of the model");
        // the file held both
        let (dictionary, record, record_handle) = held_record(&out).unwrap_or_else(|| panic!("{version:?}: the file held no record"));
        let names: Vec<_> = record.entries.iter().filter_map(|entry| entry.value.as_string()).collect();
        assert!(names.contains(&"RTVSPost2010Prop40"), "{version:?}: {names:?}");
        assert_eq!(record.owner, dictionary, "{version:?}");
        assert_eq!(names.contains(&"RTVSPropertyOp0"), version != DxfVersion::AC1024, "{version:?}: the 63 flags are the form before R2010");
        // the native dump states the handles of what the reader took out, not their contents
        let notes = dump(&out);
        let consumed: Vec<u64> = notes["note/consumed_records"].as_array().expect("the note").iter().filter_map(|v| v.as_u64()).collect();
        assert!(consumed.contains(&record_handle.value()) && consumed.contains(&dictionary.value()), "{version:?}: {consumed:?}");
    }
    // R2013 and R2018 hold the properties in the style: no record to apply, nothing consumed
    let out = write_and_read(&source, DxfVersion::AC1027);
    assert!(held_record(&out).is_none());
    assert!(!dump(&out).contains_key("note/consumed_records"));
}

/// A drawing written again at the version it was read from keeps the handles of the records the file had; an edit made after the
/// read wins over the record's old value.
#[test]
fn a_visual_style_record_is_written_again_and_an_edit_wins() {
    let (source, _) = source_with_visual_style();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
        let out = write_and_read(&source, version);
        let (dictionary, record, record_handle) = held_record(&out).expect("the record");
        let again = write_and_read(&out, version);
        let (dictionary_again, record_again, record_handle_again) = held_record(&again).expect("the record again");
        assert_eq!((dictionary_again, record_handle_again), (dictionary, record_handle), "{version:?}: the file's handles");
        assert_eq!(record_again.entries, record.entries, "{version:?}");
        // an edit after the read
        let mut edited = out.clone();
        let handle = style_handle(&edited);
        let Some(ObjectType::VisualStyle(style)) = edited.objects.get_mut(&handle) else { unreachable!() };
        style.properties[40] = VisualStyleProperty { value: VisualStylePropertyValue::Long(9), enabled: 1 };
        let written = write_and_read(&edited, version);
        assert_eq!(style_of(&written).properties[40], VisualStyleProperty { value: VisualStylePropertyValue::Long(9), enabled: 1 }, "{version:?}");
        assert_eq!(held_record(&written).expect("the record").1.entries.len(), record.entries.len(), "{version:?}");
    }
}

/// R2013 states every property in the style and AutoCAD's files of it have no extension dictionary on a style: a drawing read from
/// an older file does not get the records back.
#[test]
fn a_visual_style_written_at_r2013_gets_neither_the_record_nor_the_dictionary_back() {
    let (source, _) = source_with_visual_style();
    for from in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
        let older = write_and_read(&source, from);
        assert!(held_record(&older).is_some(), "{from:?}");
        for to in [DxfVersion::AC1027, DxfVersion::AC1032] {
            let up = write_and_read(&older, to);
            assert_eq!(style_of(&up).properties[40], VisualStyleProperty { value: VisualStylePropertyValue::Long(7), enabled: 0 }, "{from:?} -> {to:?}");
            assert!(held_record(&up).is_none() && style_of(&up).xdictionary_handle.is_none(), "{from:?} -> {to:?}");
            assert!(!has_rtvs_record(&up.with_round_trip_records()), "{from:?} -> {to:?}");
        }
    }
}

/// A record that is not exactly the one the writer states for the style (here a group of another producer) is not the file's way of
/// holding what the reader applied: it stays in the model with its extension dictionary.
#[test]
fn a_visual_style_record_with_another_producers_group_stays_in_the_model() {
    let (source, _) = source_with_visual_style();
    let mut edited = write_and_read(&source, DxfVersion::AC1018).with_round_trip_records();
    let (_, _, record_handle) = held_record(&edited).expect("the record");
    if let Some(ObjectType::XRecord(record)) = edited.objects.get_mut(&record_handle) {
        record.add_string(102, "OTHER_PRODUCER");
        record.add_int16(70, 5);
    }
    let out = write_and_read(&edited, DxfVersion::AC1018);
    assert_eq!(style_of(&out).properties.len(), 58);
    assert!(has_rtvs_record(&out), "the record stays in the model");
    let dictionary = style_of(&out).xdictionary_handle.expect("the extension dictionary stays");
    let notes = dump(&out);
    let consumed: Vec<u64> = notes.get("note/consumed_records").and_then(|v| v.as_array()).map(|l| l.iter().filter_map(|v| v.as_u64()).collect()).unwrap_or_default();
    assert!(!consumed.contains(&dictionary.value()), "the dictionary was not consumed: {consumed:?}");
}

/// AutoCAD's files state every XRECORD of an extension dictionary with the dictionary as its reactor and the cloning flags of the
/// version (every record of sample_AC1014.dwg to sample_AC1032.dwg: one reactor, its owner; 0 in R14, 1 from R2000). The records the
/// writer makes were stated without either.
#[test]
fn a_record_the_writer_makes_is_stated_as_autocad_states_it() {
    let (source, _) = source_with_visual_style();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
        let out = write_and_read(&source, version);
        let (dictionary, record, handle) = held_record(&out).expect("the record");
        let held = out.with_round_trip_records();
        assert_eq!(reactors_of(&dump(&held), handle), vec![dictionary.value()], "{version:?}: the dictionary is the reactor");
        let flags = if version >= DxfVersion::AC1015 { DictionaryCloningFlags::KeepExisting } else { DictionaryCloningFlags::NotApplicable };
        assert_eq!(record.cloning_flags, flags, "{version:?}");
    }
}

// ---- the other records the writer makes -------------------------------------------------------------------------------

/// The XRECORDs of every extension dictionary of the document as the file held them: (handle, owner, cloning flags, reactors).
fn all_records(document: &CadDocument) -> Vec<(Handle, Handle, DictionaryCloningFlags, Vec<u64>)> {
    let held = document.with_round_trip_records();
    let notes = dump(&held);
    let mut found: Vec<_> = held
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::XRecord(record) => Some((*handle, record.owner, record.cloning_flags, reactors_of(&notes, *handle))),
            _ => None,
        })
        .collect();
    found.sort_by_key(|(handle, ..)| handle.value());
    found
}

fn multiline_attribute_document() -> CadDocument {
    let mut text = acadrust::entities::MText::with_value("first line\\Psecond line", acadrust::types::Vector3::new(12.0, 3.0, 0.0));
    text.height = 2.5;
    text.rectangle_width = 7.0;
    let mut document = CadDocument::new();
    let mut definition = acadrust::entities::AttributeDefinition::new("MLTAG".into(), "prompt".into(), String::new());
    definition.mtext_flag = acadrust::entities::MTextFlag::ConstantMultiLine;
    definition.is_multiline = true;
    definition.embedded_mtext = Some(Box::new(text));
    document.add_entity(acadrust::entities::EntityType::AttributeDefinition(definition)).unwrap();
    document
}

fn typed_table_document() -> CadDocument {
    let mut table = acadrust::entities::Table::new(acadrust::types::Vector3::new(1.0, 2.0, 0.0), 3, 2);
    table.set_cell_text(1, 1, "0.500000");
    if let Some(content) = table.rows[1].cells[1].contents.first_mut() {
        content.value.value_type = acadrust::entities::CellValueType::Double;
        content.value.numeric_value = 0.5;
        content.value.text = "0.5".into();
        content.value.formatted_value = "0.500000".into();
    }
    let mut document = CadDocument::new();
    document.add_entity(acadrust::entities::EntityType::Table(Box::new(table))).unwrap();
    document
}

/// Every XRECORD of an extension dictionary in AutoCAD's files lists its dictionary as its one reactor (sample_AC1014.dwg to
/// sample_AC1032.dwg: 297, 179, 165, 136 and 97 records, all of them): also the height record of a multi-line attribute and the
/// record that links a table to its TABLECONTENT, which the writer made without.
#[test]
fn the_multiline_attribute_and_table_records_list_their_dictionary_as_reactor() {
    for (source, versions) in [
        (multiline_attribute_document(), [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024]),
        (typed_table_document(), [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1014]),
    ] {
        for version in versions {
            let out = write_and_read(&source, version);
            let records = all_records(&out);
            assert!(!records.is_empty(), "{version:?}");
            for (handle, owner, flags, reactors) in records {
                assert_eq!(reactors, vec![owner.value()], "{version:?}: {handle:?}");
                let want = if version >= DxfVersion::AC1015 { DictionaryCloningFlags::KeepExisting } else { DictionaryCloningFlags::NotApplicable };
                assert_eq!(flags, want, "{version:?}: {handle:?}");
            }
        }
    }
}

/// A style read from an R2004 file (AutoCAD's 252-entry record with the flags of the 63 property ids) written at R2010 states the
/// R2010 record (126 entries: sample_AC1024.dwg), not the record it came with.
#[test]
fn a_record_read_from_an_r2004_file_is_written_in_the_r2010_form() {
    let (source, _) = source_with_visual_style();
    let r2004 = write_and_read(&source, DxfVersion::AC1018);
    let (_, record, _) = held_record(&r2004).expect("the record");
    assert_eq!(record.entries.len(), 252);
    let r2010 = write_and_read(&r2004, DxfVersion::AC1024);
    let (_, record, _) = held_record(&r2010).expect("the record");
    assert_eq!(record.entries.len(), 126);
    assert!(!record.entries.iter().any(|entry| entry.value.as_string().is_some_and(|text| text.starts_with("RTVSPropertyOp"))));
    assert_eq!(style_of(&r2010).properties[40], VisualStyleProperty { value: VisualStylePropertyValue::Long(7), enabled: 0 });
}

// ---- layer colour (R14 and R2000) ---------------------------------------------------------------------------------------

use acadrust::tables::Layer;
use acadrust::types::Color;

fn layer_document() -> CadDocument {
    let mut document = CadDocument::new();
    let mut true_color = Layer::new("LAYER_TRUE_COLOR");
    true_color.color = Color::Rgb { r: 17, g: 146, b: 238 };
    document.layers.add(true_color).unwrap();
    let mut book = Layer::new("LAYER_COLOR_BOOK");
    book.color = Color::Rgb { r: 252, g: 193, b: 135 };
    book.book_name = Some("DIC COLOR GUIDE(R)".into());
    book.color_name = Some("DIC 7".into());
    document.layers.add(book).unwrap();
    let mut plain = Layer::new("PLAIN");
    plain.color = Color::Index(3);
    document.layers.add(plain).unwrap();
    document
}

fn layer<'a>(document: &'a CadDocument, name: &str) -> &'a Layer {
    document.layers.iter().find(|layer| layer.name.eq_ignore_ascii_case(name)).unwrap_or_else(|| panic!("no layer {name}"))
}

/// The groups of the round-trip record of a layer, by name, as the file held them.
fn layer_groups(document: &CadDocument, name: &str) -> Option<Vec<(String, Vec<acadrust::objects::XRecordEntry>)>> {
    let held = document.with_round_trip_records();
    let record = held.xrecord(layer(&held, name).handle, "ACAD_XREC_ROUNDTRIP")?.clone();
    let mut groups: Vec<(String, Vec<acadrust::objects::XRecordEntry>)> = Vec::new();
    for entry in record.entries {
        match entry.value.as_string() {
            Some(text) if entry.code == 102 => groups.push((text.to_string(), Vec::new())),
            _ => groups.last_mut().expect("a group name first").1.push(entry),
        }
    }
    Some(groups)
}

/// AutoCAD's files of R14 and R2000 keep the true colour of a layer, and its colour-book identity, in `ACAD_XREC_ROUNDTRIP
/// {COLORACI, COLORTRUE, COLORNAME}` of the layer's extension dictionary (sample_AC1014.dwg, sample_AC1015.dwg: LAYER_TRUE_COLOR index
/// 150, word 0xC21192EE; LAYER_COLOR_BOOK 31, 0xC2FCC187, `DIC COLOR GUIDE(R)$DIC 7`); the layer record holds the index. The reader
/// restores the colour and takes the record out of the model.
#[test]
fn a_layer_keeps_its_true_colour_and_book_colour_before_r2004() {
    let source = layer_document();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let out = write_and_read(&source, version);
        assert_eq!(layer(&out, "LAYER_TRUE_COLOR").color, Color::Rgb { r: 17, g: 146, b: 238 }, "{version:?}");
        let book = layer(&out, "LAYER_COLOR_BOOK");
        assert_eq!(book.color, Color::Rgb { r: 252, g: 193, b: 135 }, "{version:?}");
        assert_eq!((book.book_name.as_deref(), book.color_name.as_deref()), (Some("DIC COLOR GUIDE(R)"), Some("DIC 7")), "{version:?}");
        assert_eq!(layer(&out, "PLAIN").color, Color::Index(3));
        // the model holds no record; the file held AutoCAD's three groups
        assert!(out.xrecord(layer(&out, "LAYER_TRUE_COLOR").handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}");
        let groups = layer_groups(&out, "LAYER_TRUE_COLOR").unwrap_or_else(|| panic!("{version:?}: no record"));
        let names: Vec<_> = groups.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["COLORACI", "COLORTRUE"], "{version:?}");
        assert_eq!(groups[0].1[0].value.as_string(), None);
        assert_eq!((groups[0].1[0].code, groups[1].1[0].code), (70, 90));
        let word = match groups[1].1[0].value {
            acadrust::objects::XRecordValue::Int32(v) => v,
            ref other => panic!("{other:?}"),
        };
        assert_eq!(word as u32, 0xC211_92EE);
        let book_names: Vec<_> = layer_groups(&out, "LAYER_COLOR_BOOK").unwrap().into_iter().map(|(name, entries)| (name, entries[0].value.as_string().map(str::to_string))).collect();
        assert_eq!(book_names[2], ("COLORNAME".to_string(), Some("DIC COLOR GUIDE(R)$DIC 7".to_string())), "{version:?}");
        assert!(layer_groups(&out, "PLAIN").is_none(), "{version:?}: an index layer gets no record");
        // the colour the layer record states is the nearest index
        let aci = match layer_groups(&out, "LAYER_TRUE_COLOR").unwrap()[0].1[0].value {
            acadrust::objects::XRecordValue::Int16(v) => v,
            ref other => panic!("{other:?}"),
        };
        assert_eq!(aci, 150, "{version:?}");
    }
    // R2004 holds the colour in the layer
    for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
        let out = write_and_read(&source, version);
        assert_eq!(layer(&out, "LAYER_TRUE_COLOR").color, Color::Rgb { r: 17, g: 146, b: 238 }, "{version:?}");
        assert!(layer_groups(&out, "LAYER_TRUE_COLOR").is_none(), "{version:?}");
    }
}

/// An edit after the read wins over the record; a drawing read from R2000 and written at R2004 keeps the colour and states no record.
#[test]
fn a_layer_colour_record_is_edited_and_dropped_with_the_version() {
    let out = write_and_read(&layer_document(), DxfVersion::AC1015);
    let mut edited = out.clone();
    for layer in edited.layers.iter_mut() {
        if layer.name.eq_ignore_ascii_case("LAYER_TRUE_COLOR") {
            layer.color = Color::Rgb { r: 1, g: 2, b: 3 };
        }
        if layer.name.eq_ignore_ascii_case("PLAIN") {
            layer.color = Color::Rgb { r: 9, g: 8, b: 7 };
        }
    }
    let again = write_and_read(&edited, DxfVersion::AC1015);
    assert_eq!(layer(&again, "LAYER_TRUE_COLOR").color, Color::Rgb { r: 1, g: 2, b: 3 });
    assert_eq!(layer(&again, "PLAIN").color, Color::Rgb { r: 9, g: 8, b: 7 });
    let up = write_and_read(&out, DxfVersion::AC1018);
    assert_eq!(layer(&up, "LAYER_COLOR_BOOK").book_name.as_deref(), Some("DIC COLOR GUIDE(R)"));
    assert!(layer_groups(&up, "LAYER_COLOR_BOOK").is_none());
}

/// AutoCAD's DXF of R2000 states the index of a true-colour layer only, with no 420 or 430 and no `COLORACI` record
/// (sample_AC1015_ascii.dxf: Layer_true_color `62 150`, Layer_color_book `62 31`); its DXF of R2004 states 420 and 430
/// (sample_AC1018_ascii.dxf). R14 mirrors R2000.
#[test]
fn a_dxf_holds_the_layer_colour_as_autocads_dxf_of_the_version_does() {
    let write_dxf = |document: &CadDocument, version: DxfVersion| {
        let mut document = document.clone();
        document.version = version;
        acadrust::DxfWriter::new(&document).write_to_vec().expect("DXF write")
    };
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let bytes = write_dxf(&layer_document(), version);
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(!text.contains("COLORACI") && !text.contains("COLORTRUE") && !text.contains("DIC COLOR GUIDE(R)$DIC 7"), "{version:?}");
        let read = acadrust::DxfReader::from_reader(Cursor::new(bytes)).expect("DXF reader").read().expect("DXF read");
        assert_eq!(layer(&read, "LAYER_TRUE_COLOR").color, Color::Index(150), "{version:?}: the nearest index");
        assert!(layer_groups(&read, "LAYER_TRUE_COLOR").is_none(), "{version:?}");
        let book = layer(&read, "LAYER_COLOR_BOOK");
        assert_eq!((book.color, book.book_name.as_deref(), book.color_name.as_deref()), (Color::Index(31), None, None), "{version:?}");
        assert_eq!(layer(&read, "PLAIN").color, Color::Index(3), "{version:?}");
    }
    let bytes = write_dxf(&layer_document(), DxfVersion::AC1018);
    let read = acadrust::DxfReader::from_reader(Cursor::new(bytes)).expect("DXF reader").read().expect("DXF read");
    assert_eq!(layer(&read, "LAYER_TRUE_COLOR").color, Color::Rgb { r: 17, g: 146, b: 238 });
    assert_eq!(layer(&read, "LAYER_COLOR_BOOK").book_name.as_deref(), Some("DIC COLOR GUIDE(R)"));
    assert_eq!(layer(&read, "LAYER_COLOR_BOOK").color_name.as_deref(), Some("DIC 7"));
    // a drawing read from an R2000 DWG (colour restored from the record) is written to a DXF without the record
    let dwg = write_and_read(&layer_document(), DxfVersion::AC1015);
    assert!(layer_groups(&dwg, "LAYER_TRUE_COLOR").is_some());
    for (version, book) in [(DxfVersion::AC1015, false), (DxfVersion::AC1018, true)] {
        let text = String::from_utf8_lossy(&write_dxf(&dwg, version)).into_owned();
        assert!(!text.contains("COLORACI") && !text.contains("COLORTRUE"), "{version:?}");
        assert_eq!(text.contains("DIC COLOR GUIDE(R)$DIC 7"), book, "{version:?}");
    }
}

/// R14 has no plot flag and no line weight in a layer record: AutoCAD's R14 file keeps `{PLOTBIT 280 0}` of a layer that does not
/// plot and `{LINEWEIGHT 280 <index>}` of a layer with a line weight (sample_AC1014.dwg: 2 layers each; the 15 others have none).
#[test]
fn a_layer_keeps_its_plot_flag_and_line_weight_in_an_r14_file() {
    let mut document = CadDocument::new();
    let mut no_plot = Layer::new("NOPLOT");
    no_plot.is_plottable = false;
    document.layers.add(no_plot).unwrap();
    let mut heavy = Layer::new("HEAVY");
    heavy.line_weight = acadrust::types::LineWeight::Value(35);
    document.layers.add(heavy).unwrap();
    document.layers.add(Layer::new("PLAIN")).unwrap();
    let out = write_and_read(&document, DxfVersion::AC1014);
    assert!(!layer(&out, "NOPLOT").is_plottable);
    assert_eq!(layer(&out, "HEAVY").line_weight, acadrust::types::LineWeight::Value(35));
    assert!(layer(&out, "PLAIN").is_plottable && layer(&out, "PLAIN").line_weight == Layer::new("X").line_weight);
    assert!(out.xrecord(layer(&out, "NOPLOT").handle, "ACAD_XREC_ROUNDTRIP").is_none(), "the record is out of the model");
    let no_plot = layer_groups(&out, "NOPLOT").expect("the record");
    assert_eq!(no_plot.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["PLOTBIT"]);
    assert_eq!((no_plot[0].1[0].code, no_plot[0].1[0].value.as_string()), (280, None));
    let heavy = layer_groups(&out, "HEAVY").expect("the record");
    assert_eq!(heavy.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["LINEWEIGHT"]);
    assert!(matches!(heavy[0].1[0].value, acadrust::objects::XRecordValue::Byte(9)), "35 is DWG index 9: {:?}", heavy[0].1[0].value);
    assert!(layer_groups(&out, "PLAIN").is_none());
    // R2000 holds both in the layer record
    let out = write_and_read(&document, DxfVersion::AC1015);
    assert!(!layer(&out, "NOPLOT").is_plottable);
    assert_eq!(layer(&out, "HEAVY").line_weight, acadrust::types::LineWeight::Value(35));
    assert!(layer_groups(&out, "NOPLOT").is_none() && layer_groups(&out, "HEAVY").is_none());
}

// ---- the legacy rows' data types of a table style (R14 to R2004) ---------------------------------------------------------------

/// A drawing with a table style that has modern cell styles and a typed data row (data type 4, format `%lu2`).
fn table_style_document() -> (CadDocument, Handle) {
    let mut document = CadDocument::new();
    let styles = root_entry(&document, "ACAD_TABLESTYLE");
    let mut style = acadrust::objects::TableStyle::new("TYPED");
    style.handle = document.allocate_handle();
    style.owner_handle = styles;
    let mut cell = acadrust::objects::NamedTableCellStyle::default();
    cell.id = 1;
    cell.name = "_TITLE".into();
    style.modern_overrides = vec![(1, cell)];
    style.data_row_style.data_type = 4;
    style.data_row_style.format_string = "%lu2".into();
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::TableStyle(style));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&styles) {
        entries.add_entry("TYPED", handle);
    }
    (document, handle)
}

fn table_style(document: &CadDocument, handle: Handle) -> &acadrust::objects::TableStyle {
    match document.objects.get(&handle) {
        Some(ObjectType::TableStyle(style)) => style,
        _ => panic!("no table style {handle:?}"),
    }
}

/// AutoCAD's R14 to R2004 files keep the data types of a table style's rows in `ACAD_XREC_ROUNDTRIP {ACAD_ROUNDTRIP_PRE2007_TABLESTYLE
/// 90 .., 91 .., 1 .., ..}` (sample_AC1014.dwg to sample_AC1018.dwg, the style #87). The reader applies the group and takes it out
/// of the model when it is the one the writer states; the view and the writers put it back (under the file's handle; an edit after
/// the read wins).
#[test]
fn a_table_style_group_of_the_rows_data_types_is_taken_out_of_the_model() {
    let (source, handle) = table_style_document();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&source, version);
        let style = table_style(&out, handle);
        assert_eq!((style.data_row_style.data_type, style.data_row_style.format_string.as_str()), (4, "%lu2"), "{version:?}");
        assert!(out.xrecord(handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}: the record is out of the model");
        let held = out.with_round_trip_records();
        let record = held.xrecord(handle, "ACAD_XREC_ROUNDTRIP").unwrap_or_else(|| panic!("{version:?}: the file held no record")).clone();
        assert_eq!(record.entries.first().and_then(|entry| entry.value.as_string()), Some("ACAD_ROUNDTRIP_PRE2007_TABLESTYLE"), "{version:?}");
        let notes = dump(&out);
        let consumed: Vec<u64> = notes["note/consumed_records"].as_array().expect("the note").iter().filter_map(|v| v.as_u64()).collect();
        assert!(consumed.contains(&record.handle.value()), "{version:?}: {consumed:?}");
        // written again at the version: the file's record under its handle
        let again = write_and_read(&out, version).with_round_trip_records();
        assert_eq!(again.xrecord(handle, "ACAD_XREC_ROUNDTRIP").map(|record| (record.handle, record.entries.clone())), Some((record.handle, record.entries.clone())), "{version:?}");
        // an edit after the read wins
        let mut edited = out.clone();
        if let Some(ObjectType::TableStyle(style)) = edited.objects.get_mut(&handle) {
            style.data_row_style.format_string = "%lu4".into();
        }
        assert_eq!(table_style(&write_and_read(&edited, version), handle).data_row_style.format_string, "%lu4", "{version:?}");
        // a DWG of R2010 or later states the data types in the TABLESTYLE record: the file holds no record and the style's extension
        // dictionary stays empty (sample_AC1024.dwg to sample_AC1032.dwg, the style #87)
        for to in [DxfVersion::AC1024, DxfVersion::AC1032] {
            let up = write_and_read(&out, to).with_round_trip_records();
            assert!(up.xrecord(handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?} -> {to:?}");
            let dictionary = up.extension_dictionary_handle(handle).unwrap_or_else(|| panic!("{version:?} -> {to:?}: the dictionary"));
            assert!(matches!(up.objects.get(&dictionary), Some(ObjectType::Dictionary(entries)) if entries.entries.is_empty()), "{version:?} -> {to:?}");
        }
    }
}

/// AutoCAD's R14 file shows the colour, the plot flag and the line weight of a layer each alone in its record (sample_AC1014.dwg:
/// `EXTNAMES` and then one of them); no sample gives the order of a record that holds two, so an R14 file states none for a layer
/// that needs two (it reads back with the index, plotting and the default weight), and a layer that needs one keeps it. An edit
/// after the read wins.
#[test]
fn an_r14_layer_that_needs_two_kinds_of_record_states_none() {
    let mut document = CadDocument::new();
    let mut both = Layer::new("BOTH");
    both.color = Color::Rgb { r: 17, g: 146, b: 238 };
    both.is_plottable = false;
    document.layers.add(both).unwrap();
    let mut plot_weight = Layer::new("PLOT_WEIGHT");
    plot_weight.is_plottable = false;
    plot_weight.line_weight = acadrust::types::LineWeight::Value(35);
    document.layers.add(plot_weight).unwrap();
    let mut one = Layer::new("ONE");
    one.is_plottable = false;
    document.layers.add(one).unwrap();
    let out = write_and_read(&document, DxfVersion::AC1014);
    assert!(layer_groups(&out, "BOTH").is_none() && layer_groups(&out, "PLOT_WEIGHT").is_none());
    assert_eq!(layer(&out, "BOTH").color, Color::Index(150));
    assert!(layer(&out, "BOTH").is_plottable && layer(&out, "PLOT_WEIGHT").is_plottable);
    assert_eq!(layer(&out, "PLOT_WEIGHT").line_weight, Layer::new("X").line_weight);
    assert!(!layer(&out, "ONE").is_plottable, "a layer that needs one record keeps it");
    assert_eq!(layer_groups(&out, "ONE").expect("the record").iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["PLOTBIT"]);
    // an edit after the read wins: the layer plots again and gets a line weight
    let mut edited = out.clone();
    for layer in edited.layers.iter_mut() {
        if layer.name.eq_ignore_ascii_case("ONE") {
            layer.is_plottable = true;
            layer.line_weight = acadrust::types::LineWeight::Value(50);
        }
    }
    let again = write_and_read(&edited, DxfVersion::AC1014);
    assert!(layer(&again, "ONE").is_plottable);
    assert_eq!(layer(&again, "ONE").line_weight, acadrust::types::LineWeight::Value(50));
    assert_eq!(layer_groups(&again, "ONE").expect("the record").iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["LINEWEIGHT"]);
    // R2000 holds the plot flag and the line weight in the layer record: the colour alone needs a record
    let out = write_and_read(&document, DxfVersion::AC1015);
    assert_eq!(layer(&out, "BOTH").color, Color::Rgb { r: 17, g: 146, b: 238 });
    assert!(!layer(&out, "BOTH").is_plottable && !layer(&out, "PLOT_WEIGHT").is_plottable);
    assert_eq!(layer(&out, "PLOT_WEIGHT").line_weight, acadrust::types::LineWeight::Value(35));
}
