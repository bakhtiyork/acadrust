// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-15 (gate G5-RR): the uncompressed DWG writer and the multileader round-trip records. Synthetic documents only; each test
//! fails with its fix reverted. Evidence: AutoCAD's own files of one drawing (sample_AC1014.dwg to sample_AC1032.dwg and their DXF
//! twins; the C# dumps under Tests/CadCoreTests/Fixtures/dumps).

use std::io::Cursor;

use acadrust::objects::{BookColor, ObjectType};
use acadrust::types::{Color, DxfVersion, Handle};
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn book_drawing(version: DxfVersion) -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(version);
    document.dwg_source_version = Some(version);
    let mut book = BookColor::new();
    book.handle = document.allocate_handle();
    book.book_name = "RAL CLASSIC".into();
    book.color_name = "RAL 1006".into();
    book.color = Color::from_rgb(226, 144, 0);
    let handle = book.handle;
    document.objects.insert(handle, ObjectType::BookColor(book));
    (document, handle)
}

/// `write_to_writer_no_lz77` writes the R2007 container only: it used to write it for a document of any version, so that a document set
/// to R2000 lost its colour-book object (the objects were prepared for R2000 and encoded for R2007). A document of another version is
/// refused, writes nothing, and one of R2007 is prepared as `write_to_writer` prepares it.
#[test]
fn the_uncompressed_writer_refuses_a_version_that_has_no_such_container() {
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let (document, _) = book_drawing(version);
        let mut output = Cursor::new(Vec::new());
        let error = DwgWriter::write_to_writer_no_lz77(&mut output, &document).expect_err("a container the version does not use");
        assert!(error.to_string().contains("R2007"), "{version:?}: {error}");
        assert!(output.into_inner().is_empty(), "{version:?}: nothing is written");
    }
    let (document, handle) = book_drawing(DxfVersion::AC1021);
    let mut output = Cursor::new(Vec::new());
    DwgWriter::write_to_writer_no_lz77(&mut output, &document).expect("R2007 is written uncompressed");
    let bytes = output.into_inner();
    assert_eq!(&bytes[..6], b"AC1021");
    let read = DwgReader::from_stream(Cursor::new(bytes)).read().expect("read");
    let Some(ObjectType::BookColor(book)) = read.objects.get(&handle) else { panic!("the colour book is lost") };
    assert_eq!((book.color, book.color_name.as_str()), (Color::from_rgb(226, 144, 0), "RAL 1006"));
    // the compressed writer of the same document gives the same objects
    let compressed = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&document).unwrap())).read().unwrap();
    assert_eq!(compressed.objects.len(), read.objects.len());
}

// ── a table of an R2010 or later DWG written before R2010 ──────────────────────────────────────────────────────────────

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::table::{CellValue, CellValueType, Table};
use acadrust::entities::EntityType;
use acadrust::types::Vector3;

fn props(document: &CadDocument, kind: &str) -> serde_json::Value {
    dump_document(document, "", "new")["objects"].as_array().unwrap().iter().find(|object| object["type"] == kind).unwrap_or_else(|| panic!("no {kind}"))["props"].clone()
}

fn r2010_table_drawing(typed: bool) -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::ZERO, 1, 2);
    table.set_cell_text(0, 0, "0.500000");
    table.set_cell_text(0, 1, "60");
    if typed {
        let item = &mut table.rows[0].cells[0].contents[0];
        item.value = CellValue::new();
        item.value.value_type = CellValueType::Double;
        item.value.raw_type_code = 2;
        item.value.numeric_value = 0.5;
        item.format_value_data_type = 2;
        item.value_format = "%lu2%pr2".to_string();
        item.format_has_data = true;
    }
    let handle = document.add_entity(EntityType::Table(Box::new(table))).unwrap();
    (document, handle)
}

fn table_of(document: &CadDocument, handle: Handle) -> Table {
    match document.get_entity(handle) {
        Some(EntityType::Table(table)) => (**table).clone(),
        _ => panic!("no table"),
    }
}

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

/// An R2010 DWG record of a table has no value flags. AutoCAD's files of the drawing before R2010 state 22 for both tables, and the
/// DXF twins of R2000 to R2018 too (sample_AC1015.dwg, sample_AC1018.dwg, sample_AC1015_ascii.dxf: `90 22`); the fork wrote 0 (the
/// sample_V rows of sample_AC1024.dwg -> AC1018 were hidden by the generated rule G5-RC-S245). The native table is the R2010 record
/// again after the read, the view shows the legacy word, and a flat table (plain text cells, no objects to complete it from) states what it holds.
#[test]
fn a_table_of_an_r2010_dwg_states_the_value_flags_of_autocads_files_before_r2010() {
    let (document, handle) = r2010_table_drawing(true);
    for version in [DxfVersion::AC1018, DxfVersion::AC1015, DxfVersion::AC1014] {
        let down = write_and_read(&document, version);
        assert_eq!(props(&down, "TableEntity")["ValueFlag"], 22, "{version:?}: the record states the value flags");
        let table = table_of(&down, handle);
        assert_eq!(table.value_flags, 0, "{version:?}: the native table is the R2010 record");
        assert_eq!(table.rows[0].cells[0].contents[0].value.value_type, CellValueType::Double, "{version:?}: completed from the objects");
        // written again at the version it was read from, the words stay; written as R2010, nothing of them is left
        assert_eq!(props(&write_and_read(&down, version), "TableEntity")["ValueFlag"], 22, "{version:?}: written again");
        assert_eq!(table_of(&write_and_read(&down, DxfVersion::AC1032), handle).value_flags, 0);
    }
    // a table read from an R2010 file that keeps the objects AutoCAD's own R2010 file keeps (sample_AC1024.dwg: TABLECONTENT for both
    // tables) has them already: it states the flags as well
    let r2010 = write_and_read(&write_and_read(&document, DxfVersion::AC1018), DxfVersion::AC1024);
    assert!(r2010.objects.values().any(|object| matches!(object, ObjectType::TableContent(_))), "the objects stay in the R2010 file");
    assert_eq!(table_of(&r2010, handle).value_flags, 0);
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        assert_eq!(props(&write_and_read(&r2010, version), "TableEntity")["ValueFlag"], 22, "{version:?}: the objects of the R2010 file");
    }
    // a table whose own value flags are not 0 keeps them
    let mut own = document.clone();
    if let Some(EntityType::Table(table)) = own.get_entity_mut(handle) {
        table.value_flags = 6;
    }
    assert_eq!(props(&write_and_read(&own, DxfVersion::AC1018), "TableEntity")["ValueFlag"], 6);
    // a flat table has no objects to complete it from: it reads back as it is written
    let (flat, flat_handle) = r2010_table_drawing(false);
    let down = write_and_read(&flat, DxfVersion::AC1018);
    assert_eq!(table_of(&down, flat_handle).value_flags, 0);
}

// ── the paper-space markers of an R14 file written for R2000 ──────────────────────────────────────────────────────────────

fn paper_space_names(document: &CadDocument) -> Vec<String> {
    let mut names: Vec<String> = document.block_records.iter().filter(|record| record.name.to_ascii_lowercase().starts_with("*paper_space")).map(|record| record.name.clone()).collect();
    names.sort();
    names
}

/// An R14 file states every name in upper case and AutoCAD's own keeps the drawing's in `EXTNAMES` (`*Paper_Space0`, as its R2000 and
/// later files state them: sample_AC1014.dwg and sample_AC1015.dwg). The BLOCK marker of a numbered paper space remembered the file's
/// spelling (`*PAPER_SPACE0`) and the writer for R2000 stated it again, so that the R2000 file was read back with the upper-case names.
#[test]
fn the_paper_space_names_of_an_r14_file_keep_their_case_in_an_r2000_file() {
    let mut document = CadDocument::with_version(DxfVersion::AC1014);
    document.add_layout("Layout2").expect("a layout");
    document.add_layout("Layout3").expect("a layout");
    let r14 = write_and_read(&document, DxfVersion::AC1014);
    assert_eq!(paper_space_names(&r14), ["*Paper_Space", "*Paper_Space0", "*Paper_Space1"], "the names AutoCAD keeps in EXTNAMES");
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        assert_eq!(paper_space_names(&write_and_read(&r14, version)), paper_space_names(&r14), "{version:?}");
    }
}

// ── a vertical multileader written before R2010 ────────────────────────────────────────────────────────────────────────

use acadrust::entities::multileader::{LeaderRoot, TextAttachmentDirectionType, TextAttachmentType};
use acadrust::entities::MultiLeader;
use acadrust::{DxfReader, DxfWriter};

fn vertical_drawing(vertical: bool) -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut mleader = MultiLeader::new();
    mleader.context.leader_roots = vec![LeaderRoot::new(0)];
    if vertical {
        mleader.text_attachment_direction = TextAttachmentDirectionType::Vertical;
        mleader.context.leader_roots[0].text_attachment_direction = TextAttachmentDirectionType::Vertical;
        mleader.text_left_attachment = TextAttachmentType::TopOfTopLine;
        mleader.text_right_attachment = TextAttachmentType::BottomOfTopLineUnderlineAll;
        mleader.context.text_left_attachment = TextAttachmentType::TopOfTopLine;
        mleader.context.text_right_attachment = TextAttachmentType::BottomOfTopLineUnderlineAll;
        mleader.dogleg_length = 8.0;
    }
    let handle = document.add_entity(EntityType::MultiLeader(Box::new(mleader))).unwrap();
    (document, handle)
}

fn mleader_words(document: &CadDocument, handle: Handle) -> (TextAttachmentDirectionType, TextAttachmentDirectionType, i16, i16, f64) {
    let Some(EntityType::MultiLeader(mleader)) = document.get_entity(handle) else { panic!("no multileader") };
    (
        mleader.text_attachment_direction,
        mleader.context.leader_roots[0].text_attachment_direction,
        mleader.text_left_attachment as i16,
        mleader.text_right_attachment as i16,
        mleader.dogleg_length,
    )
}

/// The attachment direction of the text is a field of the R2010 record. AutoCAD's files before R2010 keep it (and the attachments and the
/// dogleg length a vertical leader has) in `ROUNDTRIP_MLCONTEXT` and `ACAD_ROUNDTRIP_2010_MLEADER` of the multileader's extension
/// dictionary (sample_AC1014.dwg to sample_AC1021.dwg, the vertical multileader #B27): the writer states them, the reader restores them
/// and takes the records out of the model, and the DXF of R2000 to R2007 holds them as AutoCAD's does.
#[test]
fn a_vertical_multileader_keeps_its_direction_in_a_file_before_r2010() {
    let (document, handle) = vertical_drawing(true);
    let was = mleader_words(&document, handle);
    assert_eq!(was.0, TextAttachmentDirectionType::Vertical);
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021] {
        let down = write_and_read(&document, version);
        assert_eq!(mleader_words(&down, handle), was, "{version:?}");
        assert!(down.xrecord(handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}: the record is out of the model");
        let held = down.with_round_trip_records();
        let record = held.xrecord(handle, "ACAD_XREC_ROUNDTRIP").unwrap_or_else(|| panic!("{version:?}: no record"));
        let codes: Vec<i32> = record.entries.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, [102, 272, 174, 175, 91, 40, 102, 272, 173, 174, 41], "{version:?}: the groups of AutoCAD's records");
        // written again at the same version, and for R2010
        assert_eq!(mleader_words(&write_and_read(&down, version), handle), was, "{version:?} again");
        let up = write_and_read(&down, DxfVersion::AC1024);
        assert_eq!(mleader_words(&up, handle), was, "{version:?} up");
        assert!(up.with_round_trip_records().xrecord(handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}: R2010 states the direction itself");
    }
    // a DXF of R2000 and R2004 holds the records too
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let mut dxf = document.clone();
        dxf.version = version;
        let bytes = DxfWriter::new(&dxf).write_to_vec().expect("DXF write");
        let text = String::from_utf8_lossy(&bytes).replace('\r', "");
        assert!(text.contains("\nROUNDTRIP_MLCONTEXT\n") && text.contains("\nACAD_ROUNDTRIP_2010_MLEADER\n"), "{version:?}: the DXF holds the records");
        let read = DxfReader::from_reader(Cursor::new(bytes)).expect("DXF reader").read().expect("DXF read");
        assert_eq!(mleader_words(&read, handle), was, "{version:?}: DXF");
        assert!(read.xrecord(handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}: DXF record out of the model");
    }
    // a horizontal multileader has no record
    let (flat, flat_handle) = vertical_drawing(false);
    for version in [DxfVersion::AC1014, DxfVersion::AC1018] {
        let down = write_and_read(&flat, version);
        assert!(down.with_round_trip_records().xrecord(flat_handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}: AutoCAD writes none");
        assert_eq!(mleader_words(&down, flat_handle), mleader_words(&flat, flat_handle), "{version:?}");
    }
    // an edit after the read wins
    let mut down = write_and_read(&document, DxfVersion::AC1018);
    if let Some(EntityType::MultiLeader(mleader)) = down.get_entity_mut(handle) {
        mleader.text_attachment_direction = TextAttachmentDirectionType::Horizontal;
        mleader.context.leader_roots[0].text_attachment_direction = TextAttachmentDirectionType::Horizontal;
    }
    let edited = write_and_read(&down, DxfVersion::AC1018);
    assert_eq!(mleader_words(&edited, handle).0, TextAttachmentDirectionType::Horizontal);
    assert!(edited.with_round_trip_records().xrecord(handle, "ACAD_XREC_ROUNDTRIP").is_none());
}
