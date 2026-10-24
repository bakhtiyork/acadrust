// SPDX-License-Identifier: MPL-2.0
//! R5b-5 (gate G5): the round-trip extended data of AutoCAD's own down-saves (`RTTcAl`, `RTLwPs`, `RTMaterial`). The layout
//! is the one of sample_AC1014.dwg / sample_AC1015.dwg / sample_AC1018.dwg (the C# dumps under
//! Tests/CadCoreTests/Fixtures/dumps); the numbers below are the ones of entities of those files. Synthetic documents only.
#![cfg(feature = "acadsharp-compat")]

use std::io::Cursor;

use acadrust::compat::acadsharp::dump_reader_document;
use acadrust::entities::{Circle, EntityType, Line, Point};
use acadrust::objects::{BookColor, Material, ObjectType};
use acadrust::types::{Color, DxfVersion, Handle, LineWeight, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use serde_json::json;
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn write(document: &CadDocument, version: DxfVersion) -> Vec<u8> {
    let mut document = document.clone();
    document.version = version;
    DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"))
}

fn read(bytes: Vec<u8>) -> CadDocument {
    DwgReader::from_stream(Cursor::new(bytes)).read().expect("DWG read")
}

/// Whether `text` occurs in the bit stream of `bytes` at any bit offset (an object record is bit-packed, so the extended
/// data of an entity is not byte-aligned).
fn contains(bytes: &[u8], text: &str) -> bool {
    let bits = |data: &[u8]| -> Vec<u8> { data.iter().flat_map(|byte| (0..8).rev().map(move |bit| (byte >> bit) & 1)).collect() };
    let (haystack, needle) = (bits(bytes), bits(text.as_bytes()));
    (0..=haystack.len().saturating_sub(needle.len())).any(|at| haystack[at..at + needle.len()] == needle[..])
}

/// A drawing with the three values: a line of true colour (245, 10, 147) and line weight 0.13 (AutoCAD's R14 file of its
/// drawing states `-32538 / -1024128365` and `131` for them), a line of ByBlock line weight, a circle with an own material
/// and a point whose material is ByBlock.
struct Drawing {
    document: CadDocument,
    colored: Handle,
    by_block_weight: Handle,
    circle: Handle,
    point: Handle,
    material: Handle,
}

fn drawing() -> Drawing {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let materials = document.header.acad_material_dict_handle;
    let add_material = |document: &mut CadDocument, name: &str| {
        let mut material = Material::new();
        material.name = name.into();
        material.handle = document.allocate_handle();
        material.owner = materials;
        let handle = material.handle;
        document.objects.insert(handle, ObjectType::Material(material));
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&materials) {
            entries.add_entry(name, handle);
        }
        handle
    };
    add_material(&mut document, "ByBlock");
    let material = add_material(&mut document, "Aluminum Cast");

    let mut colored = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    colored.common.color = Color::from_rgb(245, 10, 147);
    colored.common.line_weight = LineWeight::Value(13);
    let colored = document.add_entity(EntityType::Line(colored)).unwrap();

    let mut by_block_weight = Line::from_points(Vector3::ZERO, Vector3::UNIT_Y);
    by_block_weight.common.line_weight = LineWeight::ByBlock;
    let by_block_weight = document.add_entity(EntityType::Line(by_block_weight)).unwrap();

    let mut circle = Circle::new();
    circle.common.material_flags = 0b11;
    circle.common.material_handle = Some(material);
    let circle = document.add_entity(EntityType::Circle(circle)).unwrap();

    let mut point = Point::new();
    point.common.material_flags = 0b01;
    let point = document.add_entity(EntityType::Point(point)).unwrap();

    Drawing { document, colored, by_block_weight, circle, point, material }
}

fn properties(document: &CadDocument, handle: Handle) -> std::sync::Arc<acadrust::entities::EntityProperties> {
    document.get_entity(handle).expect("entity").common().properties.clone()
}

#[test]
fn down_saves_state_the_records_autocad_states_and_the_read_restores_the_values() {
    let drawing = drawing();
    for (version, true_color, line_weight, material) in [
        (DxfVersion::AC1014, true, true, true),
        (DxfVersion::AC1015, true, false, true),
        (DxfVersion::AC1018, false, false, true),
        (DxfVersion::AC1021, false, false, false),
    ] {
        let bytes = write(&drawing.document, version);
        assert_eq!(contains(&bytes, "RTTcAl"), true_color, "{version:?}: RTTcAl");
        assert_eq!(contains(&bytes, "RTLwPs"), line_weight, "{version:?}: RTLwPs");
        assert_eq!(contains(&bytes, "RTMaterial"), material, "{version:?}: RTMaterial");

        let read = read(bytes);
        let colored = properties(&read, drawing.colored);
        assert_eq!(colored.color, Color::from_rgb(245, 10, 147), "{version:?}");
        assert_eq!(colored.line_weight, LineWeight::Value(13), "{version:?}");
        assert_eq!(properties(&read, drawing.by_block_weight).line_weight, LineWeight::ByBlock, "{version:?}");
        let circle = properties(&read, drawing.circle);
        assert_eq!((circle.material_flags, circle.material_handle), (0b11, Some(drawing.material)), "{version:?}");
        let point = properties(&read, drawing.point);
        assert_eq!((point.material_flags, point.material_handle), (0b01, None), "{version:?}");
        // The records are not data of the drawing: the read removed them.
        for handle in [drawing.colored, drawing.by_block_weight, drawing.circle, drawing.point] {
            assert!(properties(&read, handle).extended_data.is_empty(), "{version:?}: {handle:?} keeps a record");
            assert!(properties(&read, handle).extended_data.raw_dwg_eed.is_empty(), "{version:?}: {handle:?} keeps a block");
        }
    }
}

#[test]
fn a_document_without_such_values_gets_no_record() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.color = Color::Index(3);
    line.common.line_weight = LineWeight::ByLayer;
    document.add_entity(EntityType::Line(line)).unwrap();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let bytes = write(&document, version);
        for name in ["RTTcAl", "RTLwPs", "RTMaterial"] {
            assert!(!contains(&bytes, name), "{version:?}: {name}");
        }
    }
}

#[test]
fn other_acad_data_of_the_entity_stays_behind_the_records() {
    let mut drawing = drawing();
    let mut record = ExtendedDataRecord::new("ACAD");
    record.add_value(XDataValue::String("PLUGINDATA".into()));
    record.add_value(XDataValue::Integer16(5));
    drawing
        .document
        .get_entity_mut(drawing.colored)
        .unwrap()
        .common_mut()
        .extended_data
        .add_record(record);
    let read = read(write(&drawing.document, DxfVersion::AC1014));
    let colored = properties(&read, drawing.colored);
    assert_eq!(colored.color, Color::from_rgb(245, 10, 147));
    let record = colored.extended_data.get_record("ACAD").expect("the plugin's data is kept");
    assert_eq!(record.values, vec![XDataValue::String("PLUGINDATA".into()), XDataValue::Integer16(5)]);
    // Written again, the record is not doubled.
    let again = read_again(&read, DxfVersion::AC1014);
    let colored = properties(&again, drawing.colored);
    assert_eq!(colored.extended_data.get_record("ACAD").unwrap().values.len(), 2);
}

fn read_again(document: &CadDocument, version: DxfVersion) -> CadDocument {
    read(write(document, version))
}

#[test]
fn an_old_drawing_written_to_a_later_version_keeps_no_record() {
    let drawing = drawing();
    let old = read(write(&drawing.document, DxfVersion::AC1014));
    let up = read(write(&old, DxfVersion::AC1032));
    let colored = properties(&up, drawing.colored);
    assert_eq!(colored.color, Color::from_rgb(245, 10, 147));
    assert_eq!(colored.line_weight, LineWeight::Value(13));
    assert!(colored.extended_data.is_empty());
    let circle = properties(&up, drawing.circle);
    assert_eq!((circle.material_flags, circle.material_handle), (0b11, Some(drawing.material)));
    let point = properties(&up, drawing.point);
    assert_eq!((point.material_flags, point.material_handle), (0b01, None));
}

/// The ACadSharp view of such a drawing: the entity record holds the nearest index and no line weight or material, and
/// the records are extended data (sample_AC1014.dwg through ACadSharp, #29A: index 230, line weight ByLayer).
#[test]
fn the_acadsharp_view_of_an_old_drawing_shows_the_records_and_the_nearest_index() {
    let drawing = drawing();
    let old = read(write(&drawing.document, DxfVersion::AC1014));
    let dump = dump_reader_document(&old, "old.dwg", "dwg");
    let entity = |handle: Handle| {
        dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|object| object["handle"] == acadrust::compat::acadsharp::handle(handle))
            .unwrap()["props"]
            .clone()
    };
    let colored = entity(drawing.colored);
    assert_eq!(colored["Color"]["Index"], 230);
    assert_eq!(colored["Color"]["IsTrueColor"], false);
    assert_eq!(colored["LineWeight"], -1);
    let records: Vec<_> = colored["ExtendedData"][0][1]["Records"].as_array().unwrap().iter().map(|r| r["RawValue"].clone()).collect();
    assert_eq!(records[0], "RTLwPs");
    assert_eq!(records[1], 131);
    assert_eq!(records[2], "RTTcAl");
    assert_eq!(records[3], -32538);
    assert_eq!(records[4], -1024128365);
    assert_eq!(entity(drawing.circle)["Material"], serde_json::Value::Null);
    assert_eq!(entity(drawing.circle)["ExtendedData"][0][1]["Records"][1]["RawValue"], drawing.material.value());
}

/// A colour-book colour: the record names it `"<book>$<name>"` and the colour value is the RGB of the colour-book object.
#[test]
fn a_named_colour_goes_into_the_record_and_back() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut book = BookColor::new();
    book.handle = document.allocate_handle();
    book.owner = document.header.named_objects_dict_handle;
    book.color = Color::from_rgb(226, 144, 0);
    book.book_name = "RAL CLASSIC".into();
    book.color_name = "RAL 1006".into();
    let book_handle = book.handle;
    document.objects.insert(book_handle, ObjectType::BookColor(book));
    let root = document.header.named_objects_dict_handle;
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&root) {
        entries.add_entry("RAL CLASSIC$RAL 1006", book_handle);
    }
    let mut circle = Circle::new();
    circle.common.color = Color::Index(42);
    circle.common.color_book_handle = Some(book_handle);
    let circle = document.add_entity(EntityType::Circle(circle)).unwrap();
    let read = read(write(&document, DxfVersion::AC1015));
    let restored = properties(&read, circle);
    // A DWG before R2004 holds a colour-book object with an index only (no names): the read gives the colour and its name, the
    // way a DXF states a named colour (420 and 430).
    assert_eq!(restored.color, Color::from_rgb(226, 144, 0));
    assert_eq!(restored.color_name.as_deref(), Some("RAL CLASSIC$RAL 1006"));
    assert!(restored.extended_data.is_empty());
    // Written again it states the same record.
    assert_eq!(read_again(&read, DxfVersion::AC1015).get_entity(circle).unwrap().common().color_name.as_deref(), Some("RAL CLASSIC$RAL 1006"));
    let dump = dump_reader_document(&read, "old.dwg", "dwg");
    let records: Vec<_> = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["handle"] == acadrust::compat::acadsharp::handle(circle))
        .unwrap()["props"]["ExtendedData"][0][1]["Records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["RawValue"].clone())
        .collect();
    // 0xC000 (colour book) | 42; the packed colour 0xC2E29000.
    assert_eq!(records[1], (0xC02Au16 as i16));
    assert_eq!(records[2], 0xC2E2_9000u32 as i32);
    assert_eq!(records[3], "RAL CLASSIC$RAL 1006");
}

fn mtext_drawing() -> (CadDocument, Handle) {
    use acadrust::entities::{LineSpacingStyle, MText};
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = MText::new();
    text.value = "7.0711".into();
    text.line_spacing_style = LineSpacingStyle::Exactly;
    text.line_spacing_factor = 1.5;
    // The MTEXT #9DA of AutoCAD's drawing: a true colour fill (54, 117, 66), scale 1.25, no transparency.
    text.background_fill_flags = 1;
    text.background_scale = 1.25;
    text.background_color = Color::from_rgb(54, 117, 66);
    text.background_transparency = 0;
    let handle = document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    (document, handle)
}

fn mtext(document: &CadDocument, handle: Handle) -> acadrust::entities::MText {
    match document.get_entity(handle) {
        Some(EntityType::MText(text)) => (**text).clone(),
        _ => panic!("no MTEXT"),
    }
}

/// AutoCAD's R2000 file of its drawing states, for the MTEXT with a fill, `MTEXTBEGIN 73 1 44 1 74 0 MTEXTEND` and `ACAD_MTEXT_BBRT 91 1 46
/// 1.25 64 -1036618430 65 "" 66 "" 442 0 ACAD_MTEXT_BERT`; the native model holds line spacing and fill.
#[test]
fn an_mtext_of_a_version_before_r2004_states_line_spacing_and_fill_in_its_data() {
    let (document, handle) = mtext_drawing();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let bytes = write(&document, version);
        assert!(contains(&bytes, "MTEXTBEGIN") && contains(&bytes, "ACAD_MTEXT_BBRT"), "{version:?}");
        let read = read(bytes);
        let text = mtext(&read, handle);
        assert_eq!(text.background_fill_flags, 1, "{version:?}");
        assert_eq!(text.background_scale, 1.25);
        assert_eq!(text.background_color, Color::from_rgb(54, 117, 66));
        // The record states the spacing; R14 has no field of its own for it.
        assert_eq!(text.line_spacing_style, acadrust::entities::LineSpacingStyle::Exactly, "{version:?}");
        assert_eq!(text.line_spacing_factor, 1.5, "{version:?}");
        assert!(text.common.extended_data.is_empty(), "{version:?}: the blocks are not data of the drawing");
        // The ACadSharp view reads them as extended data and no fill.
        let dump = dump_reader_document(&read, "old.dwg", "dwg");
        let props = dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|object| object["handle"] == acadrust::compat::acadsharp::handle(handle))
            .unwrap()["props"]
            .clone();
        let records: Vec<_> = props["ExtendedData"][0][1]["Records"].as_array().unwrap().iter().map(|r| r["RawValue"].clone()).collect();
        let at = records.iter().position(|r| r == "MTEXTBEGIN").expect("MTEXTBEGIN");
        let expected = [json!(73), json!(2), json!(44), json!(1.5), json!(74), json!(0)];
        assert_eq!(records[at + 1..at + 7], expected, "{version:?}");
        let bg = records.iter().position(|r| r == "ACAD_MTEXT_BBRT").expect("BBRT");
        assert_eq!(records[bg + 1..bg + 4], [json!(91), json!(1), json!(46)]);
        assert_eq!(records[bg + 5..bg + 7], [json!(64), json!(-1036618430)], "{version:?}");
        assert_eq!(records.last().unwrap(), "ACAD_MTEXT_BERT");
    }
    // R2004 holds both natively.
    let bytes = write(&document, DxfVersion::AC1018);
    assert!(!contains(&bytes, "MTEXTBEGIN") && !contains(&bytes, "ACAD_MTEXT_BBRT"));
    let text = mtext(&read(bytes), handle);
    assert_eq!((text.background_fill_flags, text.line_spacing_factor), (1, 1.5));
}

#[test]
fn an_mtext_without_the_block_in_its_source_is_written_without_it() {
    let (document, handle) = mtext_drawing();
    let mut old = read(write(&document, DxfVersion::AC1014));
    // As AutoCAD's R14 file states none for three dimension texts.
    match old.get_entity_mut(handle) {
        Some(EntityType::MText(text)) => text.dwg_without_spacing_block = true,
        _ => panic!("no MTEXT"),
    }
    let again = read(write(&old, DxfVersion::AC1014));
    assert!(mtext(&again, handle).dwg_without_spacing_block);
    // Written for a later version it is data of the drawing no more.
    let up = read(write(&old, DxfVersion::AC1032));
    assert!(!mtext(&up, handle).dwg_without_spacing_block);
}
