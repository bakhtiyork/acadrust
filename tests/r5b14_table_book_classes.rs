// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-14 (gate G5-RR, TABLE down-saves, colour books): what the fork's DWG of a DXF of R2010 or later and of a pre-R2004 colour-book
//! drawing loses or states differently from its native model. Synthetic documents only; each test fails with its fix reverted.
//! Evidence: AutoCAD's own files of one drawing (sample_AC1014.dwg to sample_AC1032.dwg and their DXF twins; the C# dumps under
//! Tests/CadCoreTests/Fixtures/dumps).

use std::io::Cursor;

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::table::{CellValue, CellValueType, Table};
use acadrust::entities::EntityType;
use acadrust::objects::{BookColor, Dictionary, ObjectType, XRecord};
use acadrust::types::{Color, DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn dxf_source(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    DxfReader::from_reader(Cursor::new(DxfWriter::new(&document).write_to_vec().expect("DXF write")))
        .expect("DXF reader")
        .read()
        .expect("DXF read")
}

fn props(document: &CadDocument, kind: &str) -> serde_json::Value {
    dump_document(document, "", "new")["objects"].as_array().unwrap().iter().find(|object| object["type"] == kind).unwrap_or_else(|| panic!("no {kind}"))["props"].clone()
}

// ── a table of a DXF of R2010 or later ─────────────────────────────────────────────────────────────────────────────────

/// A drawing with a table whose legacy words are AutoCAD's (value flags 22, the edge flags 9 of a cell: sample_AC1024_ascii.dxf) and
/// whose extension dictionary links the TABLECONTENT object with the typed cells.
fn table_drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1024);
    let mut legacy = Table::new(Vector3::ZERO, 1, 2);
    legacy.set_cell_text(0, 0, "0.500000");
    legacy.set_cell_text(0, 1, "60");
    legacy.value_flags = 22;
    legacy.rows[0].cells[0].edge_flags = 9;
    legacy.rows[0].cells[1].block_scale = 0.25;
    let mut content = legacy.clone();
    for (column, (number, format, unit)) in [(0.5, "", 0), (1.0471975511965976, "%au0", 2)].into_iter().enumerate() {
        let item = &mut content.rows[0].cells[column].contents[0];
        item.value = CellValue::new();
        item.value.value_type = CellValueType::Double;
        item.value.raw_type_code = 2;
        item.value.numeric_value = number;
        item.format_value_data_type = 2;
        item.format_value_unit_type = unit;
        item.value_format = format.to_string();
        item.format_has_data = true;
    }
    content.common.handle = document.allocate_handle();
    let content_handle = content.common.handle;
    document.objects.insert(content_handle, ObjectType::TableContent(content));
    let mut record = XRecord::new();
    record.handle = document.allocate_handle();
    record.add_string(102, "ACAD_ROUNDTRIP_2008_TABLE_ENTITY");
    record.add_handle(360, content_handle);
    record.add_int16(70, 2);
    record.add_int32(90, 0);
    let record_handle = record.handle;
    let mut dictionary = Dictionary::new();
    dictionary.handle = document.allocate_handle();
    dictionary.add_entry("ACAD_XREC_ROUNDTRIP", record_handle);
    let dictionary_handle = dictionary.handle;
    document.objects.insert(record_handle, ObjectType::XRecord(record));
    document.objects.insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    legacy.common.xdictionary_handle = Some(dictionary_handle);
    let handle = document.add_entity(EntityType::Table(Box::new(legacy))).unwrap();
    (document, handle)
}

fn table_of(document: &CadDocument, handle: Handle) -> Table {
    match document.get_entity(handle) {
        Some(EntityType::Table(table)) => (**table).clone(),
        _ => panic!("no table"),
    }
}

/// A DXF of R2010 or later states the legacy words of the table (the value flags 22, the edge flags of the cells, the block scale)
/// next to the TABLECONTENT object; the table holds what an R2010 DWG record holds (AutoCAD's DWG of the table reads 0, no flags, scale
/// 1: sample_AC1024.dwg), and the words stay in the legacy view the ACadSharp view and the writers use.
#[test]
fn a_table_of_a_dxf_of_r2010_holds_an_r2010_record_and_keeps_the_legacy_words() {
    let (document, handle) = table_drawing();
    let source = dxf_source(&document, DxfVersion::AC1024);
    let table = table_of(&source, handle);
    assert_eq!(table.rows[0].cells[0].contents[0].value.value_type, CellValueType::Double, "completed from the content object");
    assert_eq!((table.value_flags, table.rows[0].cells[0].edge_flags, table.rows[0].cells[1].block_scale), (0, 0, 1.0));
    assert_eq!(props(&source, "TableEntity")["ValueFlag"], 22, "the view of the file is the legacy entity");
    // a DXF of R2010 written again states the legacy words (G4)
    let again = dxf_source(&source, DxfVersion::AC1024);
    assert_eq!(props(&again, "TableEntity")["ValueFlag"], 22);
    assert_eq!(table_of(&again, handle).rows, table.rows);
}

/// Written before R2010 (the flat record) the table states the words its DXF stated, as AutoCAD's R2000 and R2004 files do
/// (sample_AC1015_ascii.dxf, sample_AC1018_ascii.dxf: value flags 22 for both tables), so the file read again is the table that was
/// written: before R5b-14 the record had 0 and an R2010 DXF's table read back with other words than the source's.
#[test]
fn a_table_of_a_dxf_of_r2010_states_its_legacy_words_in_a_dwg_before_r2010() {
    let (document, handle) = table_drawing();
    let source = dxf_source(&document, DxfVersion::AC1024);
    for version in [DxfVersion::AC1018, DxfVersion::AC1015, DxfVersion::AC1014] {
        let down = write_and_read(&source, version);
        assert_eq!(props(&down, "TableEntity")["ValueFlag"], 22, "{version:?}: the record states the value flags");
        let (read, was) = (table_of(&down, handle), table_of(&source, handle));
        let words = |table: &Table| {
            let cells = &table.rows[0].cells;
            (
                table.value_flags,
                table.override_flag,
                cells.iter().map(|cell| (cell.edge_flags, cell.block_scale, cell.contents[0].value.value_type, cell.contents[0].value.numeric_value)).collect::<Vec<_>>(),
            )
        };
        assert_eq!(words(&read), words(&was), "{version:?}");
    }
    // written as R2010 again (an R2010 DWG record has no place for the words): nothing changes
    let same = write_and_read(&source, DxfVersion::AC1024);
    assert_eq!(table_of(&same, handle).rows[0].cells[0].contents[0].value.numeric_value, 0.5);
    assert_eq!(table_of(&same, handle).value_flags, 0);
}

/// A cell edited after the read is the edited one: the legacy cells are no longer the table's.
#[test]
fn an_edit_after_reading_a_dxf_of_r2010_wins_in_a_dwg_before_r2010() {
    let (document, handle) = table_drawing();
    let mut source = dxf_source(&document, DxfVersion::AC1024);
    if let Some(EntityType::Table(table)) = source.get_entity_mut(handle) {
        table.set_cell_text(0, 1, "7");
    }
    let down = write_and_read(&source, DxfVersion::AC1018);
    assert_eq!(table_of(&down, handle).rows[0].cells[1].text_value(), "7");
}

// ── a colour-book object of a drawing before R2004 ─────────────────────────────────────────────────────────────────────

fn book_drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut book = BookColor::new();
    book.handle = document.allocate_handle();
    book.book_name = "RAL CLASSIC".into();
    book.color_name = "RAL 1006".into();
    book.color = Color::from_rgb(226, 144, 0);
    let handle = book.handle;
    document.objects.insert(handle, ObjectType::BookColor(book));
    (document, handle)
}

fn book_of(document: &CadDocument, handle: Handle) -> BookColor {
    match document.objects.get(&handle) {
        Some(ObjectType::BookColor(book)) => book.clone(),
        _ => panic!("no colour book"),
    }
}

/// A DWG before R2004 holds the index of a colour-book object in its record and the colour and the names in the `DBColXD` block
/// (sample_AC1015.dwg #996: index 42, (226, 144, 0), "RAL CLASSIC", "RAL 1006"; AutoCAD's R2004 file has them in the record). The
/// native model has the colour and the names; the ACadSharp view shows the record (the index, no names); written for R2000 the
/// record states the index again and for R2004 and later the colour, and the colour of an object edited after the read wins.
#[test]
fn a_colour_book_object_read_from_an_r2000_dwg_has_its_colour_and_names() {
    let (document, handle) = book_drawing();
    let r2000 = write_and_read(&document, DxfVersion::AC1015);
    let book = book_of(&r2000, handle);
    assert_eq!(book.color, Color::from_rgb(226, 144, 0));
    assert_eq!((book.book_name.as_str(), book.color_name.as_str()), ("RAL CLASSIC", "RAL 1006"));
    assert_eq!(props(&r2000, "BookColor")["Color"]["Index"], 42, "the view shows the record");
    assert_eq!(props(&r2000, "BookColor")["BookName"], "", "the record has no names");
    // the file written again for R2000 is the file read; for R2004 the object states the colour
    let again = write_and_read(&r2000, DxfVersion::AC1015);
    assert_eq!(book_of(&again, handle).color, book.color);
    assert_eq!(props(&again, "BookColor")["Color"]["Index"], 42);
    let up = write_and_read(&r2000, DxfVersion::AC1018);
    assert_eq!((book_of(&up, handle).color, book_of(&up, handle).color_name), (book.color, book.color_name.clone()));
    assert_eq!(props(&up, "BookColor")["Color"]["B"], 0);
    assert_eq!(props(&up, "BookColor")["ColorName"], "RAL 1006");
    // a DXF of R2000 written from it is read as the same object, and states the record of the DWG of R2000: the index alone (the
    // colour and the names are in the block; AutoCAD's DXF of R2000 has no colour in the object: sample_AC1015_ascii.dxf)
    let dxf = dxf_source(&r2000, DxfVersion::AC1015);
    assert_eq!(book_of(&dxf, handle).color, book.color);
    let mut text_doc = r2000.clone();
    text_doc.version = DxfVersion::AC1015;
    let text = String::from_utf8_lossy(&DxfWriter::new(&text_doc).write_to_vec().unwrap()).replace('\r', "");
    let at = text.find("\n  0\nDBCOLOR\n").expect("a DBCOLOR record");
    let record = &text[at + 1..at + 1 + text[at + 8..].find("\n  0\n").unwrap()];
    assert!(record.contains("\n 62\n    42\n") && !record.contains("\n420\n") && !record.contains("\n430\n"), "{record}");
    // an edit after the read wins
    let mut edited = r2000.clone();
    if let Some(ObjectType::BookColor(book)) = edited.objects.get_mut(&handle) {
        book.color = Color::from_rgb(10, 20, 30);
    }
    assert_eq!(book_of(&write_and_read(&edited, DxfVersion::AC1018), handle).color, Color::from_rgb(10, 20, 30));
}
