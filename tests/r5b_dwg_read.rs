// SPDX-License-Identifier: MPL-2.0
//! R5b-1 (gate G5): DWG reader and ACadSharp-view differences found by reading ACadSharp's own DWG files.
//!
//! ACadSharp writes a word of its object model as it is, also when the word is no valid value of the field (an
//! alignment 0, a line spacing style 0, an attachment point 0). The reader folds such a word into the default of
//! the typed field and keeps the word as provenance (never part of equality); the ACadSharp view shows the word.
#![cfg(feature = "acadsharp-compat")]

use std::io::Cursor;

use acadrust::compat::acadsharp::dump_reader_document;
use acadrust::entities::{Dimension, DimensionLinear, EntityType, MText};
use acadrust::objects::{GeoData, ObjectType, SourceWord, TableStyle};
use acadrust::types::{DxfVersion, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter};
use serde_json::Value;

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: {error}"))
}

/// Registers `object` under `key` in the named object dictionary.
fn register(document: &mut CadDocument, key: &str, handle: acadrust::types::Handle, object: ObjectType) {
    let root = document.header.named_objects_dict_handle;
    document.objects.insert(handle, object);
    let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&root) else {
        panic!("no named object dictionary");
    };
    dictionary.add_entry(key, handle);
}

fn projected(document: &CadDocument, type_name: &str) -> Value {
    dump_reader_document(document, "r5b.dwg", "dwg")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == type_name)
        .unwrap_or_else(|| panic!("no {type_name}"))["props"]
        .clone()
}

#[test]
fn mtext_line_spacing_style_word_is_shown_as_stated() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        document.add_entity(EntityType::MText(Box::new(MText::new()))).unwrap();
        let mut read = write_and_read(&document, version);
        assert_eq!(projected(&read, "MText")["LineSpacingStyle"], 1, "{version:?}: a valid style");
        for entity in read.entities_mut() {
            if let EntityType::MText(text) = entity {
                text.source_line_spacing_style = SourceWord(Some(0));
            }
        }
        assert_eq!(projected(&read, "MText")["LineSpacingStyle"], 0, "{version:?}: the stated word");
    }
}

#[test]
fn dimension_attachment_point_word_is_shown_as_stated() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let linear = DimensionLinear::new(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
        document
            .add_entity(EntityType::Dimension(Box::new(Dimension::Linear(linear))))
            .unwrap();
        let mut read = write_and_read(&document, version);
        assert_eq!(projected(&read, "DimensionLinear")["AttachmentPoint"], 5, "{version:?}: a valid point");
        for entity in read.entities_mut() {
            if let EntityType::Dimension(dimension) = entity {
                dimension.base_mut().source_attachment_point = SourceWord(Some(0));
            }
        }
        assert_eq!(projected(&read, "DimensionLinear")["AttachmentPoint"], 0, "{version:?}: the stated word");
    }
}

#[test]
fn legacy_row_style_alignment_word_is_shown_as_stated() {
    // The TABLESTYLE object starts with R2004 (a file of an older version keeps it only when it had it).
    for version in [DxfVersion::AC1018, DxfVersion::AC1021] {
        let mut document = CadDocument::with_version(version);
        let handle = document.allocate_handle();
        let mut style = TableStyle::new("Standard");
        style.handle = handle;
        style.owner_handle = document.header.named_objects_dict_handle;
        register(&mut document, "ACAD_TABLESTYLE", handle, ObjectType::TableStyle(style));
        let mut read = write_and_read(&document, version);
        let alignment = |document: &CadDocument| projected(document, "TableStyle")["DataCellStyle"]["CellAlignment"].clone();
        let before = alignment(&read);
        assert_ne!(before, 0, "{version:?}: a valid alignment");
        for object in read.objects.values_mut() {
            if let ObjectType::TableStyle(style) = object {
                style.data_row_style.source_alignment = SourceWord(Some(0));
            }
        }
        assert_eq!(alignment(&read), 0, "{version:?}: the stated word");
    }
}

/// ACadSharp's R2009 GEODATA layout has no scale estimation: its property keeps the class default (1), whatever the
/// civil trailer of an AutoCAD record states. Later layouts state it.
#[test]
fn geodata_scale_estimation_follows_the_layout_of_the_record() {
    for (layout, expected) in [(1, 1), (3, 2)] {
        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        let mut geodata = GeoData::default();
        geodata.handle = document.allocate_handle();
        geodata.owner = document.header.named_objects_dict_handle;
        geodata.version = layout;
        geodata.scale_estimation_method = 2;
        let handle = geodata.handle;
        register(&mut document, "ACAD_GEOGRAPHICDATA", handle, ObjectType::GeoData(geodata));
        let read = write_and_read(&document, DxfVersion::AC1032);
        assert_eq!(
            projected(&read, "GeoData")["ScaleEstimationMethod"],
            expected,
            "layout {layout}"
        );
    }
}

/// A file without the default applications (`AcadAnnotative`, `AcCmTransparency`, `AcAecLayerStandard`) does not
/// get them back in the APPID table when it is written again: the reader adds them to its document, the writer leaves
/// out those nothing uses.
#[test]
fn unused_default_application_ids_stay_out_of_the_appid_table() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        for name in ["AcadAnnotative", "AcCmTransparency", "AcAecLayerStandard"] {
            document.app_ids.remove(name);
        }
        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        let count = |document: &CadDocument| projected(document, "AppIdsTable")["Count"].clone();
        assert_eq!(count(&first), 1, "{version:?}: the first file has the ACAD application only");
        assert_eq!(count(&second), 1, "{version:?}: and so has the one written from the first read");
    }
}
