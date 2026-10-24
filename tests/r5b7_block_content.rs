// SPDX-License-Identifier: MPL-2.0
//! R5b-7 (gate G5): the description and the preview image of a block in a drawing of R14. R2000 and later state them in the
//! block record; AutoCAD's R14 file (sample_AC1014.dwg) states them in the records `CONTENTBLOCKDESCRIPTION` and
//! `CONTENTBLOCKICON` of the block record's extended data (the image in chunks of 127 bytes, byte for byte the preview of
//! sample_AC1015.dwg). Synthetic documents only.
#![cfg(feature = "acadsharp-compat")]

use std::io::Cursor;

use acadrust::entities::{EntityType, Line};
use acadrust::tables::{AppId, BlockRecord};
use acadrust::types::{DxfVersion, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

const NAME: &str = "my-dynamic-block";

fn image() -> Vec<u8> {
    (0..4136u32).map(|i| (i % 251) as u8).collect()
}

/// A drawing as an R14 file holds a block: the description and the image are records of the block record.
fn drawing_of_r14() -> CadDocument {
    let mut doc = CadDocument::with_version(DxfVersion::AC1014);
    for name in ["CONTENTBLOCKDESCRIPTION", "CONTENTBLOCKICON"] {
        let mut app = AppId::new(name);
        app.handle = doc.allocate_handle();
        doc.app_ids.add(app).unwrap();
    }
    let mut record = BlockRecord::new(NAME);
    record.handle = doc.allocate_handle();
    record.block_entity_handle = doc.allocate_handle();
    record.block_end_handle = doc.allocate_handle();
    let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0));
    let handle = doc.add_entity(EntityType::Line(line)).unwrap();
    record.entity_handles.push(handle);
    let block = record.handle;
    doc.block_records.add(record).unwrap();

    let mut description = ExtendedDataRecord::new("CONTENTBLOCKDESCRIPTION");
    description.values = vec![XDataValue::Integer32(0), XDataValue::String("My dynamic block description.".into())];
    doc.set_object_xdata_record(block, description);
    let mut icon = ExtendedDataRecord::new("CONTENTBLOCKICON");
    icon.values = std::iter::once(XDataValue::Integer32(0))
        .chain(image().chunks(127).map(|chunk| XDataValue::BinaryData(chunk.to_vec())))
        .collect();
    doc.set_object_xdata_record(block, icon);
    doc
}

fn applications(doc: &CadDocument) -> Vec<String> {
    let handle = doc.block_records.get(NAME).unwrap().handle;
    doc.object_xdata(handle).map_or_else(Vec::new, |data| data.records().iter().map(|record| record.application_name.clone()).collect())
}

fn values_of(doc: &CadDocument) -> (String, Vec<u8>) {
    let block = doc.block_records.get(NAME).unwrap();
    (block.description.clone(), block.preview_data.clone())
}

#[test]
fn a_dwg_of_r14_reads_the_description_and_preview_as_values_and_states_them_again() {
    let bytes = DwgWriter::write_to_vec(&drawing_of_r14()).unwrap();
    let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    assert_eq!(values_of(&read), ("My dynamic block description.".to_string(), image()));
    // the records are what the format needs, not data of the drawing
    assert!(!applications(&read).iter().any(|name| name.starts_with("CONTENTBLOCK")), "{:?}", applications(&read));
    // written again at R14 they are the records again (the values survive another read); at R2000 the values are the record's
    let again = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&read).unwrap())).read().unwrap();
    assert_eq!(values_of(&again), values_of(&read));
    let mut r2000 = read.clone();
    r2000.version = DxfVersion::AC1015;
    let up = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&r2000).unwrap())).read().unwrap();
    assert_eq!(values_of(&up), values_of(&read));
    assert!(!applications(&up).iter().any(|name| name.starts_with("CONTENTBLOCK")));
}

#[test]
fn the_records_of_r14_are_in_the_dxf_of_r14_and_not_in_the_dxf_of_r2000() {
    let read = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&drawing_of_r14()).unwrap())).read().unwrap();
    for (version, records) in [(DxfVersion::AC1014, true), (DxfVersion::AC1015, false)] {
        let mut doc = read.clone();
        doc.version = version;
        let text = String::from_utf8(DxfWriter::new(&doc).write_to_vec().unwrap()).unwrap().replace("\r\n", "\n");
        // the record is the extended data of the application (the application table names it either way)
        assert_eq!(text.contains("1001\nCONTENTBLOCKICON\n"), records, "{version:?}");
        assert_eq!(text.contains("1001\nCONTENTBLOCKDESCRIPTION\n"), records, "{version:?}");
        // the record stands for the groups of R2000 (4 on the BLOCK, 310 on the BLOCK_RECORD): not both in a DXF of R14
        assert_eq!(text.matches("My dynamic block description.").count(), 1, "{version:?}");
        assert_eq!(text.contains("\n  4\nMy dynamic block description.\n"), !records, "{version:?}");
        assert_eq!(text.contains("\n310\n"), !records, "{version:?}");
        let again = DxfReader::from_reader(Cursor::new(text.into_bytes())).unwrap().read().unwrap();
        assert_eq!(values_of(&again).0, "My dynamic block description.", "{version:?}");
        assert_eq!(values_of(&again).1, image(), "{version:?}");
    }
}

/// ACadSharp reads the records of a drawing of R14 as extended data, not as the description and the preview: the ACadSharp view
/// shows them so whether the drawing is a DWG or a DXF of R14 (whose BLOCK and BLOCK_RECORD then have no group 4 or 310).
#[test]
fn the_acadsharp_view_of_a_drawing_of_r14_shows_the_records_and_not_the_values() {
    use acadrust::compat::acadsharp::{dump_reader_document, handle};
    let dwg = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&drawing_of_r14()).unwrap())).read().unwrap();
    let dxf = DxfReader::from_reader(Cursor::new(DxfWriter::new(&dwg).write_to_vec().unwrap())).unwrap().read().unwrap();
    for (doc, file, reader) in [(&dwg, "r14.dwg", "dwg"), (&dxf, "r14.dxf", "dxf")] {
        assert_eq!(values_of(doc), ("My dynamic block description.".to_string(), image()), "{reader}");
        let dump = dump_reader_document(doc, file, reader);
        let objects = dump["objects"].as_array().unwrap();
        let props = |h| objects.iter().find(|object| object["handle"] == handle(h)).map(|object| object["props"].clone());
        let record = doc.block_records.get(NAME).unwrap();
        let view = props(record.handle).expect("block record");
        assert!(view["Preview"].is_null(), "{reader}: {}", view["Preview"]);
        let names: Vec<String> = view["ExtendedData"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| objects.iter().find(|object| object["handle"] == entry[0]))
            .map(|app| app["props"]["Name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["CONTENTBLOCKDESCRIPTION", "CONTENTBLOCKICON"], "{reader}");
        let block = props(record.block_entity_handle).expect("block");
        assert!(block["Comments"].is_null(), "{reader}: {}", block["Comments"]);
    }
}
