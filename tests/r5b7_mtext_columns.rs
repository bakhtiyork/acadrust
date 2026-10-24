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

fn the_record() -> Vec<XDataValue> {
    vec![
        XDataValue::String("ACAD_MTEXT_COLUMN_INFO_BEGIN".into()),
        XDataValue::Integer16(75),
        XDataValue::Integer16(2),
        XDataValue::Integer16(79),
        XDataValue::Integer16(0),
        XDataValue::Integer16(76),
        XDataValue::Integer16(1),
        XDataValue::Integer16(78),
        XDataValue::Integer16(0),
        XDataValue::Integer16(48),
        XDataValue::Real(15.741414244987482),
        XDataValue::Integer16(49),
        XDataValue::Real(110.22316703811953),
        XDataValue::Integer16(50),
        XDataValue::Integer16(1),
        XDataValue::Real(0.0),
        XDataValue::String("ACAD_MTEXT_COLUMN_INFO_END".into()),
    ]
}

#[test]
fn a_dwg_before_r2018_states_the_columns_in_the_record_as_autocad_does() {
    let (document, handle) = drawing();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1027] {
        let mut older = document.clone();
        older.version = version;
        let bytes = DwgWriter::write_to_vec(&older).unwrap_or_else(|error| panic!("{version:?}: {error}"));
        let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        assert_eq!(column_record(&read, handle), Some(the_record()), "{version:?}");
        let Some(EntityType::MText(text)) = read.get_entity(handle) else { panic!("{version:?}: no MTEXT") };
        assert_eq!(text.column_data.column_type, 0, "{version:?}: the columns are the record's");
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
        assert_eq!(column_record(&read, handle), Some(the_record()), "{version:?}");
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
    assert_eq!(column_record(&read, handle), Some(the_record()));
    // written again at another version before R2018: the record is the source's, as it was read
    for version in [DxfVersion::AC1024, DxfVersion::AC1015] {
        read.version = version;
        let again = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&read).unwrap())).read().unwrap();
        assert_eq!(column_record(&again, handle), Some(the_record()), "{version:?}");
    }
}
