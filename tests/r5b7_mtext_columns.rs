// SPDX-License-Identifier: MPL-2.0
//! R5b-7 (gate G5): the columns of an MTEXT in a drawing written to a version before R2018. R2018 states them in the MTEXT
//! record; AutoCAD's files from R2000 to R2013 (sample_AC1015.dwg to sample_AC1027.dwg and their DXF twins) state them in the
//! `ACAD_MTEXT_COLUMN_INFO` block of the `ACAD` extended data, with the height 0 where sample_AC1032.dwg states the negative
//! height of the laid-out text. Synthetic documents only.

use std::io::Cursor;

use acadrust::entities::{EntityType, MText, MTextColumnData};
use acadrust::types::{DxfVersion, Handle};
use acadrust::xdata::XDataValue;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut text = MText::new();
    text.value = "this is a Mtext\\Pwith multiple lines in it".into();
    text.column_data = MTextColumnData {
        column_type: 2,
        column_count: 0,
        flow_reversed: false,
        auto_height: false,
        width: 15.741414244987482,
        gutter: 110.22316703811953,
        heights: vec![-2.9442928603910867],
    };
    let handle = document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    (document, handle)
}

fn column_record(document: &CadDocument, handle: Handle) -> Option<Vec<XDataValue>> {
    let record = document.get_entity(handle).unwrap().common().extended_data.get_record("ACAD")?.values.clone();
    let begin = record.iter().position(|value| matches!(value, XDataValue::String(text) if text == "ACAD_MTEXT_COLUMN_INFO_BEGIN"))?;
    let end = record.iter().position(|value| matches!(value, XDataValue::String(text) if text == "ACAD_MTEXT_COLUMN_INFO_END"))?;
    Some(record[begin..=end].to_vec())
}

#[test]
fn a_dwg_before_r2018_states_the_columns_in_the_record_as_autocad_does() {
    let (document, handle) = drawing();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1027] {
        let mut older = document.clone();
        older.version = version;
        let bytes = DwgWriter::write_to_vec(&older).unwrap_or_else(|error| panic!("{version:?}: {error}"));
        let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        // R5b-8: the reader takes the block into the entity (it held the columns as a record of the extended data before), so the
        // block is not data of the entity and the columns are native, with the height 0 the older file states.
        assert_eq!(column_record(&read, handle), None, "{version:?}");
        let Some(EntityType::MText(text)) = read.get_entity(handle) else { panic!("{version:?}: no MTEXT") };
        assert_eq!(
            text.column_data,
            MTextColumnData { column_type: 2, column_count: 1, flow_reversed: false, auto_height: false, width: 15.741414244987482, gutter: 110.22316703811953, heights: vec![0.0] },
            "{version:?}"
        );
        // ... and a file written from it states the block again, as the first one did
        assert!(read.get_entity(handle).is_some());
        let again = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&read).unwrap())).read().unwrap();
        let Some(EntityType::MText(text_again)) = again.get_entity(handle) else { panic!("{version:?}: no MTEXT") };
        assert_eq!(text_again.column_data, text.column_data, "{version:?}: written again");
    }
    // R2018 states them in the MTEXT record, with the height it was given, and no record
    let bytes = DwgWriter::write_to_vec(&document).unwrap();
    let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    assert_eq!(column_record(&read, handle), None);
    let Some(EntityType::MText(text)) = read.get_entity(handle) else { panic!("no MTEXT") };
    assert_eq!((text.column_data.column_type, text.column_data.heights.clone()), (2, vec![-2.9442928603910867]));
}

#[test]
fn a_dxf_before_r2018_states_the_columns_in_the_record_as_autocad_does() {
    let (document, handle) = drawing();
    for (version, spacing) in [(DxfVersion::AC1015, true), (DxfVersion::AC1018, false), (DxfVersion::AC1027, false)] {
        let mut older = document.clone();
        older.version = version;
        let bytes = DxfWriter::new(&older).write_to_vec().unwrap();
        let read = DxfReader::from_reader(Cursor::new(bytes.clone())).unwrap().read().unwrap();
        // R5b-8: the DXF reader takes the block into the entity's columns as the DWG reader does (it stayed a record before)
        assert_eq!(column_record(&read, handle), None, "{version:?}");
        let Some(EntityType::MText(column_text)) = read.get_entity(handle) else { panic!("{version:?}: no MTEXT") };
        assert_eq!(column_text.column_data.heights, vec![0.0], "{version:?}");
        assert_eq!((column_text.column_data.column_type, column_text.column_data.column_count), (2, 1), "{version:?}");
        let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
        let column = text.find("ACAD_MTEXT_COLUMN_INFO_BEGIN").expect("record");
        assert_eq!(text.contains("MTEXTBEGIN"), spacing, "{version:?}");
        if spacing {
            // sample_AC1015_ascii.dxf: the column block first, then the line spacing
            assert!(column < text.find("MTEXTBEGIN").unwrap(), "{version:?}");
        }
    }
    let bytes = DxfWriter::new(&document).write_to_vec().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("ACAD_MTEXT_COLUMN_INFO"));
}

#[test]
fn an_mtext_without_columns_of_its_own_keeps_the_record_its_source_stated() {
    let (document, handle) = drawing();
    let mut r2013 = document.clone();
    r2013.version = DxfVersion::AC1027;
    let bytes = DwgWriter::write_to_vec(&r2013).unwrap();
    let mut read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    // R5b-8: the block was taken into the entity's columns when it was read (it stayed a record of the extended data before)
    assert_eq!(column_record(&read, handle), None);
    // written again at another version before R2018: the same record, stated from the columns
    for version in [DxfVersion::AC1024, DxfVersion::AC1015] {
        read.version = version;
        let again = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&read).unwrap())).read().unwrap();
        let Some(EntityType::MText(text)) = again.get_entity(handle) else { panic!("{version:?}: no MTEXT") };
        assert_eq!((text.column_data.column_type, text.column_data.heights.clone()), (2, vec![0.0]), "{version:?}: stated again, read again");
        assert_eq!(column_record(&again, handle), None, "{version:?}");
    }
}
