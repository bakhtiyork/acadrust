//! R5b-1 (gate G5): DWG writer losses found by the DWG round trips, one test per fix.
//!
//! Each test writes a synthetic document to DWG at every version the fix concerns and reads it back natively; no
//! external file is used.

use std::io::Cursor;

use acadrust::entities::underlay::{Underlay, UnderlayType};
use acadrust::entities::{Block, EntityType, Line, Table};
use acadrust::tables::BlockRecord;
use acadrust::objects::{ObjectType, UnderlayDefinition};
use acadrust::tables::Layer;
use acadrust::types::{DxfVersion, Handle, Transparency, Vector3};
use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, DwgReader, DwgWriter};

const MODERN: [DxfVersion; 4] = [
    DxfVersion::AC1018,
    DxfVersion::AC1024,
    DxfVersion::AC1027,
    DxfVersion::AC1032,
];
const ALL: [DxfVersion; 6] = [
    DxfVersion::AC1014,
    DxfVersion::AC1015,
    DxfVersion::AC1018,
    DxfVersion::AC1024,
    DxfVersion::AC1027,
    DxfVersion::AC1032,
];

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document)
        .unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn record_of(application: &str, values: Vec<XDataValue>) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(application);
    record.values = values;
    record
}

fn application_order(data: &ExtendedData) -> Vec<String> {
    data.records()
        .iter()
        .map(|record| record.application_name.clone())
        .collect()
}

/// A layer that carries a description (modelled, so the writer merges it into the retained block) between two
/// applications of its own keeps the three blocks in the order they were read in.
#[test]
fn extended_data_blocks_keep_their_order_when_a_modelled_block_is_merged() {
    for version in MODERN {
        let mut document = CadDocument::with_version(version);
        for name in ["FIRST_APP", "LAST_APP"] {
            document
                .app_ids
                .add(acadrust::tables::AppId::new(name))
                .unwrap();
        }
        let mut layer = Layer::new("L");
        layer.description = "described".into();
        layer.handle = document.allocate_handle();
        let handle = layer.handle;
        document.layers.add(layer).unwrap();
        let mut data = ExtendedData::default();
        data.add_record(record_of("FIRST_APP", vec![XDataValue::Integer16(1)]));
        data.add_record(record_of(
            "AcAecLayerStandard",
            vec![XDataValue::String(String::new()), XDataValue::String("described".into())],
        ));
        data.add_record(record_of("LAST_APP", vec![XDataValue::Integer16(3)]));
        document.object_xdata.insert(handle, data);
        // The second write has the blocks of the first one as raw blocks of a DWG file.
        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        for (label, document) in [("first", &first), ("second", &second)] {
            let layer = document.layers.get("L").unwrap();
            let data = document.object_xdata.get(&layer.handle).expect("layer extended data");
            let order = application_order(data);
            let position = |name: &str| order.iter().position(|app| app == name).unwrap_or(usize::MAX);
            assert!(
                position("FIRST_APP") < position("AcAecLayerStandard")
                    && position("AcAecLayerStandard") < position("LAST_APP"),
                "{version:?} {label} write: extended data order {order:?}"
            );
            assert_eq!(layer.description, "described", "{version:?} {label} write");
        }
    }
}

/// A packed zero in a layer's `AcCmTransparency` block is the opaque default: it stays zero and does not turn into
/// the explicit opaque value 0x020000FF.
#[test]
fn layer_transparency_block_keeps_a_packed_zero() {
    for version in MODERN {
        let mut document = CadDocument::with_version(version);
        let mut layer = Layer::new("OPAQUE");
        layer.handle = document.allocate_handle();
        let handle = layer.handle;
        document.layers.add(layer).unwrap();
        let mut data = ExtendedData::default();
        data.add_record(record_of("AcCmTransparency", vec![XDataValue::Integer32(0)]));
        document.object_xdata.insert(handle, data);
        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        for (label, document) in [("first", &first), ("second", &second)] {
            let layer = document.layers.get("OPAQUE").unwrap();
            assert_eq!(layer.transparency, Transparency::OPAQUE, "{version:?} {label}");
            let packed = document
                .object_xdata
                .get(&layer.handle)
                .and_then(|data| data.get_record("AcCmTransparency"))
                .and_then(|record| {
                    record.values.iter().find_map(|value| match value {
                        XDataValue::Integer32(packed) => Some(*packed),
                        _ => None,
                    })
                });
            assert_eq!(packed, Some(0), "{version:?} {label}: packed layer transparency");
        }
    }
}

/// A changed layer transparency is still written (the retained zero no longer describes it).
#[test]
fn layer_transparency_change_is_written() {
    for version in MODERN {
        let mut document = CadDocument::with_version(version);
        let mut layer = Layer::new("HALF");
        layer.transparency = Transparency::T_50;
        layer.handle = document.allocate_handle();
        let handle = layer.handle;
        document.layers.add(layer).unwrap();
        let mut data = ExtendedData::default();
        data.add_record(record_of("AcCmTransparency", vec![XDataValue::Integer32(0)]));
        document.object_xdata.insert(handle, data);
        let read = write_and_read(&document, version);
        assert_eq!(read.layers.get("HALF").unwrap().transparency, Transparency::T_50, "{version:?}");
    }
}

fn line_document(version: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    document
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(10.0, 20.0, 0.0),
            Vector3::new(30.0, 40.0, 0.0),
        )))
        .unwrap();
    document
}

/// Extents the header holds are the drawing's own and are written as they are, however far the entities lie from
/// them (AutoCAD keeps EXTMIN/EXTMAX until the next zoom).
#[test]
fn header_extents_are_written_as_held() {
    for version in ALL {
        let mut document = line_document(version);
        document.header.model_space_extents_min = Vector3::new(-5.0, -6.0, 0.0);
        document.header.model_space_extents_max = Vector3::new(7.0, 8.0, 0.0);
        let read = write_and_read(&document, version);
        assert_eq!(read.header.model_space_extents_min, Vector3::new(-5.0, -6.0, 0.0), "{version:?}");
        assert_eq!(read.header.model_space_extents_max, Vector3::new(7.0, 8.0, 0.0), "{version:?}");
    }
}

/// A header with no extents (the 1e20 / -1e20 defaults) gets the extents of the model space entities.
#[test]
fn unset_header_extents_are_filled_from_the_entities() {
    for version in ALL {
        let document = line_document(version);
        assert!(document.header.model_space_extents_min.x > document.header.model_space_extents_max.x);
        let read = write_and_read(&document, version);
        assert_eq!(read.header.model_space_extents_min, Vector3::new(10.0, 20.0, 0.0), "{version:?}");
        assert_eq!(read.header.model_space_extents_max, Vector3::new(30.0, 40.0, 0.0), "{version:?}");
    }
}

/// The native-dump `reactors/<handle>` record of an object: its reactor handles, or empty.
fn reactors_of(document: &CadDocument, handle: Handle) -> Vec<u64> {
    let mut dump = Vec::new();
    acadrust::native_dump::write_native_dump(document, &mut dump).unwrap();
    let key = format!("\"k\":\"reactors/{:016X}\"", handle.value());
    String::from_utf8(dump)
        .unwrap()
        .lines()
        .find(|line| line.contains(&key))
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            value["v"]
                .as_array()
                .expect("reactors array")
                .iter()
                .map(|reactor| reactor.as_u64().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

/// An underlay definition read from DWG keeps all its reactors (its owning dictionary too) when the underlay that
/// references it makes the writer add the underlay to the list.
#[test]
fn underlay_definition_keeps_its_reactors_on_a_second_write() {
    for version in ALL {
        let mut document = CadDocument::with_version(version);
        let dictionary = document.header.named_objects_dict_handle;
        let mut definition = UnderlayDefinition::new(UnderlayType::Pdf);
        definition.handle = document.allocate_handle();
        definition.owner_handle = dictionary;
        definition.reactors = vec![dictionary];
        definition.file_path = "sheet.pdf".into();
        let definition_handle = definition.handle;
        document
            .objects
            .insert(definition_handle, ObjectType::UnderlayDefinition(definition));
        let mut underlay = Underlay::new(UnderlayType::Pdf);
        underlay.definition_handle = definition_handle;
        document.add_entity(EntityType::Underlay(underlay)).unwrap();

        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        let before = reactors_of(&first, definition_handle);
        let after = reactors_of(&second, definition_handle);
        assert!(before.contains(&dictionary.value()), "{version:?}: first write {before:?}");
        assert_eq!(before, after, "{version:?}: reactors changed by a second write");
    }
}

/// A table row, column or cell whose source stated a style type but no style data keeps the type: it was written
/// as 0, which another reader takes for a different (cell) style.
#[test]
fn table_style_types_without_data_survive() {
    for version in [DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut table = Table::new(Vector3::ZERO, 2, 2);
        table.columns[0].source_style_type = Some(3);
        table.columns[1].source_style_type = Some(3);
        table.rows[0].source_style_type = Some(2);
        table.rows[1].source_style_type = Some(2);
        table.rows[0].cells[1].source_style_type = Some(1);
        table.base_style_type = Some(4);
        let mut document = CadDocument::with_version(version);
        document.add_entity(EntityType::Table(Box::new(table))).unwrap();
        let read = write_and_read(&document, version);
        let table = read
            .entities()
            .find_map(|entity| match entity {
                EntityType::Table(table) => Some((**table).clone()),
                _ => None,
            })
            .expect("table");
        assert_eq!(table.base_style_type, Some(4), "{version:?}: table");
        assert_eq!(table.columns[0].source_style_type, Some(3), "{version:?}: column 0");
        assert_eq!(table.columns[1].source_style_type, Some(3), "{version:?}: column 1");
        assert_eq!(table.rows[0].source_style_type, Some(2), "{version:?}: row 0");
        assert_eq!(table.rows[1].source_style_type, Some(2), "{version:?}: row 1");
        assert_eq!(table.rows[0].cells[1].source_style_type, Some(1), "{version:?}: cell");
    }
}

/// HANDSEED is the first free handle. A document read from a file keeps the seed its file stated: the reader moves a
/// default application ID that sits on the handle of a record of the file to a fresh handle, which advances the
/// native seed, and writing that seed would grow the file's with every round trip.
#[test]
fn handle_seed_does_not_grow_with_round_trips() {
    for version in ALL {
        let mut document = line_document(version);
        // The file has no AcadAnnotative application and a layer on the handle the reader's default one takes.
        let annotative = document.app_ids.get("AcadAnnotative").expect("default application").handle;
        document.app_ids.remove("AcadAnnotative");
        let mut layer = Layer::new("ON_DEFAULT_HANDLE");
        layer.handle = annotative;
        document.layers.add(layer).unwrap();
        document.header.handle_seed = 5000;
        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        let third = write_and_read(&second, version);
        assert_eq!(
            first.header.source_handle_seed,
            Some(5000),
            "{version:?}: the file states the seed it was given"
        );
        for (label, read) in [("second", &second), ("third", &third)] {
            assert_eq!(
                read.header.source_handle_seed,
                Some(5000),
                "{version:?}: seed stated by the {label} file"
            );
        }
    }
}

/// An anonymous block's BLOCK marker states the number the file had ("*U7"), while the table record has the bare
/// "*U" and the native name is AutoCAD's numbering on reopening: the marker keeps the stated number.
#[test]
fn anonymous_block_marker_keeps_the_number_of_the_source() {
    for version in ALL {
        let mut document = CadDocument::with_version(version);
        let mut record = BlockRecord::new("*U1");
        record.handle = document.allocate_handle();
        record.block_entity_handle = document.allocate_handle();
        record.block_end_handle = document.allocate_handle();
        let marker_handle = record.block_entity_handle;
        let mut marker = Block::new("*U1", Vector3::ZERO);
        marker.common.handle = marker_handle;
        marker.common.owner_handle = record.handle;
        marker.source_name = Some("*U7".into());
        document.block_records.add(record).unwrap();
        document.add_entity(EntityType::Block(marker)).unwrap();
        let read = write_and_read(&document, version);
        let stated: Vec<_> = read
            .block_records
            .iter()
            .filter(|record| record.name.starts_with("*U"))
            .filter_map(|record| match read.get_entity(record.block_entity_handle) {
                Some(EntityType::Block(marker)) => Some((marker.name.clone(), marker.source_name.clone())),
                _ => None,
            })
            .collect();
        assert!(
            stated.iter().any(|(_, source)| source.as_deref() == Some("*U7")),
            "{version:?}: markers {stated:?}"
        );
    }
}

/// A dynamic block definition the file stated anonymous ("*U", flag 1) with its public name in the extended data is
/// written the way it was stated: reading the result restores the name from the extended data again, and the
/// record is still stated anonymous.
#[test]
fn dynamic_definition_stated_anonymous_stays_anonymous() {
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        document
            .app_ids
            .add(acadrust::tables::AppId::new("AcDbDynamicBlockTrueName"))
            .unwrap();
        let mut record = BlockRecord::new("my-dynamic-block");
        record.handle = document.allocate_handle();
        record.block_entity_handle = document.allocate_handle();
        record.block_end_handle = document.allocate_handle();
        record.source_flags = 1;
        let handle = record.handle;
        document.block_records.add(record).unwrap();
        let mut data = ExtendedData::default();
        data.add_record(record_of(
            "AcDbDynamicBlockTrueName",
            vec![XDataValue::String("my-dynamic-block".into())],
        ));
        document.object_xdata.insert(handle, data);
        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        for (label, read) in [("first", &first), ("second", &second)] {
            let record = read
                .block_records
                .get("my-dynamic-block")
                .unwrap_or_else(|| panic!("{version:?} {label}: the public name is gone"));
            assert_eq!(record.source_flags & 1, 1, "{version:?} {label}: stated anonymous");
        }
    }
}

/// The records of a fixed class (LAYOUT, ACDBPLACEHOLDER, ...) are written under fixed type codes; the class still
/// states how many there are.
#[test]
fn fixed_classes_state_the_number_of_their_objects() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let document = CadDocument::with_version(version);
        let layouts = document
            .objects
            .values()
            .filter(|object| matches!(object, ObjectType::Layout(_)))
            .count() as i32;
        assert!(layouts > 0, "the new document has layouts");
        let read = write_and_read(&document, version);
        let class = read
            .classes
            .get_by_name("LAYOUT")
            .unwrap_or_else(|| panic!("{version:?}: no LAYOUT class"));
        assert_eq!(class.instance_count, layouts, "{version:?}: LAYOUT instances");
    }
}

/// The "referenced" bit (64 of the DXF group 70) of LAYER, STYLE, LTYPE and APPID records is stated before R2007 and
/// written from the source flags.
#[test]
fn referenced_bit_of_table_records_survives_before_r2007() {
    use acadrust::tables::{AppId, LineType, TextStyle};
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let mut document = CadDocument::with_version(version);
        let mut layer = Layer::new("REFERENCED");
        layer.source_flags = 64;
        document.layers.add(layer).unwrap();
        let mut style = TextStyle::new("REFERENCED");
        style.source_flags = 64;
        document.text_styles.add(style).unwrap();
        let mut linetype = LineType::new("REFERENCED");
        linetype.source_flags = 64;
        document.line_types.add(linetype).unwrap();
        let mut app = AppId::new("REFERENCED");
        app.source_flags = 64;
        document.app_ids.add(app).unwrap();
        let read = write_and_read(&document, version);
        let second = write_and_read(&read, version);
        for (label, read) in [("first", &read), ("second", &second)] {
            assert_eq!(read.layers.get("REFERENCED").unwrap().source_flags & 64, 64, "{version:?} {label}: layer");
            assert_eq!(read.text_styles.get("REFERENCED").unwrap().source_flags & 64, 64, "{version:?} {label}: style");
            assert_eq!(read.line_types.get("REFERENCED").unwrap().source_flags & 64, 64, "{version:?} {label}: linetype");
            assert_eq!(read.app_ids.get("REFERENCED").unwrap().source_flags & 64, 64, "{version:?} {label}: application");
        }
    }
}

/// A document read from a file of the version it is written as keeps the style dictionaries the profile of older
/// versions would drop (TABLESTYLE before R2004, MLEADERSTYLE before R2007): the file had them.
#[test]
fn style_dictionaries_of_a_same_version_source_are_kept() {
    use acadrust::objects::{Dictionary, TableStyle};
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let root = document.header.named_objects_dict_handle;
    let dictionary_handle = document.allocate_handle();
    let style_handle = document.allocate_handle();
    let mut style = TableStyle::new("Standard");
    style.handle = style_handle;
    style.owner_handle = dictionary_handle;
    let mut dictionary = Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary.owner = root;
    dictionary.add_entry("Standard", style_handle);
    document.objects.insert(style_handle, ObjectType::TableStyle(style));
    document.objects.insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    let Some(ObjectType::Dictionary(root_dictionary)) = document.objects.get_mut(&root) else {
        panic!("no named object dictionary");
    };
    root_dictionary.add_entry("ACAD_TABLESTYLE", dictionary_handle);
    let mut read = write_and_read(&document, DxfVersion::AC1018);
    assert!(read.objects.contains_key(&style_handle), "the R2004 file has its table style");

    // The same document, as if read from an R2000 file that has the dictionaries (another application wrote it).
    read.version = DxfVersion::AC1015;
    read.dwg_source_version = Some(DxfVersion::AC1015);
    let same_version = write_and_read(&read, DxfVersion::AC1015);
    assert!(same_version.objects.contains_key(&style_handle), "a source of the same version keeps the style");

    // Written to R2000 from another version, the profile drops it.
    read.dwg_source_version = Some(DxfVersion::AC1018);
    let down_saved = write_and_read(&read, DxfVersion::AC1015);
    assert!(!down_saved.objects.contains_key(&style_handle), "a down-save drops the style");
}

/// The VIEWPORT grid behaviour bits (status bits 18..21) belong to R2007+; a document read from a file of an older
/// version that states them keeps them.
#[test]
fn viewport_grid_bits_of_a_same_version_source_are_kept() {
    use acadrust::entities::{GridFlags, Viewport};
    let version = DxfVersion::AC1015;
    let build = |source: Option<DxfVersion>| {
        let mut document = CadDocument::with_version(version);
        let mut view = Viewport::new();
        view.id = 2;
        view.width = 160.0;
        view.height = 100.0;
        view.grid_flags = GridFlags::from_bits(0b0011);
        document
            .add_entity_to_layout(EntityType::Viewport(Box::new(view)), "Layout1")
            .unwrap();
        document.dwg_source_version = source;
        document
    };
    let grid_of = |document: &CadDocument| {
        document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Viewport(view) if view.width == 160.0 => Some(view.grid_flags.to_bits()),
                _ => None,
            })
            .expect("viewport")
    };
    assert_eq!(grid_of(&write_and_read(&build(Some(version)), version)), 0b0011, "a same-version source keeps them");
    assert_eq!(grid_of(&write_and_read(&build(None), version)), 0, "a document of no file does not state them before R2007");
}

/// A block whose BLOCK marker the document does not hold (a document read from DXF, or built in memory) gets a
/// marker that states the record's full name, as AutoCAD's do ("*Paper_Space0", "*D3"); the record has the bare
/// name. With the bare name in every marker, a reader that names records after their markers finds two
/// "*Paper_Space" records.
#[test]
fn synthesized_block_markers_state_the_full_name() {
    for version in ALL {
        let mut document = CadDocument::with_version(version);
        document.add_layout("Second").unwrap();
        document.add_layout("Third").unwrap();
        let read = write_and_read(&document, version);
        let mut stated: Vec<(String, String)> = read
            .block_records
            .iter()
            .filter(|record| record.name.to_ascii_uppercase().starts_with("*PAPER_SPACE"))
            .map(|record| match read.get_entity(record.block_entity_handle) {
                Some(EntityType::Block(marker)) => (
                    record.name.clone(),
                    marker.source_name.clone().unwrap_or_else(|| marker.name.clone()),
                ),
                _ => panic!("{version:?}: {} has no marker", record.name),
            })
            .collect();
        stated.sort();
        assert!(stated.len() >= 3, "{version:?}: {stated:?}");
        for (record, marker) in &stated {
            assert_eq!(record, marker, "{version:?}: the marker states the record's name");
        }
    }
}
