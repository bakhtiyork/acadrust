// SPDX-License-Identifier: MPL-2.0
//! R5b-8 (gate G5-RR, cross-version cells): defects found by writing a drawing at another version and reading it back.
//! Synthetic documents only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing
//! (sample_AC1014.dwg to sample_AC1032.dwg and their DXF twins; the C# dumps under Tests/CadCoreTests/Fixtures/dumps).

use std::io::Cursor;

use acadrust::entities::{EntityType, MText, MTextColumnData};
use acadrust::objects::{ObjectType, VisualStyleProperty, VisualStylePropertyValue};
use acadrust::tables::BlockRecord;
use acadrust::types::{DxfVersion, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn dxf_source(document: &CadDocument) -> CadDocument {
    DxfReader::from_reader(Cursor::new(DxfWriter::new(document).write_to_vec().expect("DXF write")))
        .expect("DXF reader")
        .read()
        .expect("DXF read")
}

fn entry_of(document: &CadDocument, dictionary: &str) -> acadrust::types::Handle {
    match document.objects.get(&document.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.get(dictionary).unwrap_or_else(|| panic!("no {dictionary} in the root dictionary")),
        _ => panic!("no root dictionary"),
    }
}

/// A drawing read from an R2018 DWG with one visual style (58 properties) whose property 40 (after the 28th) differs from the default.
fn document_with_visual_style() -> (CadDocument, acadrust::types::Handle) {
    let mut document = CadDocument::new();
    let styles = entry_of(&document, "ACAD_VISUALSTYLE");
    let mut style = acadrust::objects::VisualStyle::new();
    style.handle = document.allocate_handle();
    style.owner = styles;
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::VisualStyle(style));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&styles) {
        entries.add_entry("MYSTYLE", handle);
    }
    let mut source = write_and_read(&document, DxfVersion::AC1032);
    let handle = source
        .objects
        .iter()
        .find_map(|(handle, object)| matches!(object, ObjectType::VisualStyle(_)).then_some(*handle))
        .expect("the visual style");
    let Some(ObjectType::VisualStyle(style)) = source.objects.get_mut(&handle) else { unreachable!() };
    assert_eq!(style.properties.len(), 58);
    style.properties[40] = VisualStyleProperty { value: VisualStylePropertyValue::Long(7), enabled: 0 };
    (source, handle)
}

fn visual_style(document: &CadDocument) -> &acadrust::objects::VisualStyle {
    document
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::VisualStyle(style) if style.properties.len() >= 28 && style.description.is_empty() => Some(style),
            _ => None,
        })
        .expect("the visual style")
}

/// AutoCAD's R2010 files keep the properties of a visual style after the 28th in its round-trip record
/// (sample_AC1024.dwg: 126 entries on each of the 24 styles, `RTVSPost2010Prop28` .. with the flags `RTVSPost2010PropOp28` ..,
/// and not the 63 flags `RTVSPropertyOp0` .. of the R2004 form); the R2010 object states 28. Before R5b-8 the writer stated no
/// record at R2010 and the reader restored no style of 28 properties from one, so the 30 properties were lost in an R2010 file.
#[test]
fn a_visual_style_of_r2018_keeps_its_late_properties_in_an_r2010_file() {
    let (source, _) = document_with_visual_style();
    let out = write_and_read(&source, DxfVersion::AC1024);
    let style = visual_style(&out);
    assert_eq!(style.properties.len(), 58, "restored from the record");
    assert_eq!(style.properties[40], VisualStyleProperty { value: VisualStylePropertyValue::Long(7), enabled: 0 });
    // the record is in AutoCAD's R2010 form: values and flags from the 28th property on, no flags of the first 63 ids
    let record = out
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::XRecord(record) if record.entries.iter().any(|e| e.value.as_string() == Some("RTVSPost2010Prop40")) => Some(record),
            _ => None,
        })
        .expect("the round-trip record");
    let names: Vec<_> = record.entries.iter().filter(|e| e.code == 102).filter_map(|e| e.value.as_string()).collect();
    assert!(names.contains(&"RTVSPost2010Prop28") && names.contains(&"RTVSPost2010PropOp40"), "{names:?}");
    assert!(!names.iter().any(|name| name.starts_with("RTVSPropertyOp")), "R2010 states no flags of the first 63 ids: {names:?}");
    // R2013 states every property in the style and has no record
    let up = write_and_read(&out, DxfVersion::AC1027);
    assert_eq!(visual_style(&up).properties[40], style.properties[40]);
    assert!(!up.objects.values().any(|object| matches!(object, ObjectType::XRecord(record) if record.entries.iter().any(|e| e.value.as_string() == Some("RTVSPost2010Prop40")))));
}

/// The record of an R2010 file as AutoCAD writes it for a style that states only the flags (no value after the 28th) leaves the
/// style at its 28 properties; the writer does not add a record to a style of 28 properties that has none.
#[test]
fn a_visual_style_of_28_properties_gets_no_record_at_r2010() {
    let (mut source, handle) = document_with_visual_style();
    if let Some(ObjectType::VisualStyle(style)) = source.objects.get_mut(&handle) {
        style.properties.truncate(28);
    }
    let out = write_and_read(&source, DxfVersion::AC1024);
    assert_eq!(visual_style(&out).properties.len(), 28);
    assert!(!out.objects.values().any(|object| matches!(object, ObjectType::XRecord(record) if record.entries.iter().any(|e| e.value.as_string().is_some_and(|text| text.starts_with("RTVS"))))));
    // no record of any kind is made for the style: as many records as the same drawing has written at a version that makes none
    let xrecords = |document: &CadDocument| document.objects.values().filter(|object| matches!(object, ObjectType::XRecord(_))).count();
    assert_eq!(xrecords(&out), xrecords(&write_and_read(&source, DxfVersion::AC1032)));
}

fn document_with_columns() -> CadDocument {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = MText::with_value("columns", Vector3::ZERO);
    text.column_data = MTextColumnData {
        column_type: 2,
        column_count: 1,
        flow_reversed: false,
        auto_height: false,
        width: 15.5,
        gutter: 110.25,
        heights: vec![-2.5],
    };
    document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    document
}

fn columns_of(document: &CadDocument) -> (MTextColumnData, usize) {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::MText(text) => Some((text.column_data.clone(), text.common.extended_data.records().len())),
            _ => None,
        })
        .expect("the MTEXT")
}

/// A drawing before R2018 states the columns of an MTEXT in the block `ACAD_MTEXT_COLUMN_INFO` of its `ACAD` data (sample_AC1015.dwg
/// to sample_AC1027.dwg: the four MTEXT that sample_AC1032.dwg states as dynamic columns, with the height 0 where R2018 states
/// the negative height of the laid-out text). The reader keeps the columns in the entity and not in a record of its extended data.
#[test]
fn the_columns_of_an_mtext_survive_a_file_before_r2018() {
    let source = document_with_columns();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027] {
        let (columns, records) = columns_of(&write_and_read(&source, version));
        assert_eq!((columns.column_type, columns.column_count, columns.width, columns.gutter), (2, 1, 15.5, 110.25), "{version:?}");
        assert_eq!(columns.heights, vec![0.0], "{version:?}: the negative height of R2018 is 0 in the older file");
        assert_eq!(records, 0, "{version:?}: the block is not left in the extended data");
    }
}

/// A block that is a column of a pre-R2018 drawing with positive height keeps it (AutoCAD keeps a height of its own).
#[test]
fn a_positive_column_height_is_kept() {
    let mut source = document_with_columns();
    for entity in source.entities_mut() {
        if let EntityType::MText(text) = entity {
            text.column_data.heights = vec![23.85];
        }
    }
    let (columns, _) = columns_of(&write_and_read(&source, DxfVersion::AC1027));
    assert_eq!(columns.heights, vec![23.85]);
}

/// R13 and R14 state the description and the preview image of a block in records of its extended data (sample_AC1014.dwg:
/// CONTENTBLOCKDESCRIPTION, CONTENTBLOCKICON), R2000 and later in the block record. A drawing of another version written as R14 lost
/// both (the writer stated the records only for a block whose records the reader had taken out of an R14 file).
#[test]
fn the_description_and_preview_of_a_block_survive_an_r14_file() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut block = BlockRecord::new("my-block");
    block.handle = document.allocate_handle();
    block.description = "a block with a description".into();
    block.preview_data = (0..300u32).map(|i| (i % 251) as u8).collect();
    let preview = block.preview_data.clone();
    document.block_records.add(block).ok();
    let out = write_and_read(&document, DxfVersion::AC1014);
    let block = out.block_records.iter().find(|record| record.name == "my-block").expect("the block");
    assert_eq!(block.description, "a block with a description");
    assert_eq!(block.preview_data, preview);
}

/// A class that a DXF states live stays live in every DWG version it is written at (AutoCAD's DWG of the drawing states none of
/// its classes a zombie, whatever the version and the count: sample_AC1018.dwg to sample_AC1032.dwg), including a class whose
/// stated count the written file does not reach (CELLSTYLEMAP in the R2010 and later files); and a DXF's list of classes is kept
/// whole, in its order, at every version (the 50 classes of AutoCAD's six files of one drawing are one list), where a version
/// before R2013 got the short profile list and lost the classes the DXF stated without a record. Both were so for the version of the
/// DXF and R2013 and later only.
#[test]
fn the_classes_a_dxf_states_stay_live_and_whole_in_other_versions() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.classes.clear();
    document.classes.add_or_update(acadrust::classes::DxfClass::new("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault"));
    let mut custom = acadrust::classes::DxfClass::new("ACME_CUSTOM_OBJECT", "AcmeCustomObject");
    custom.application_name = "ObjectDBX Classes".into();
    document.classes.add_or_update(custom);
    let mut unused = acadrust::classes::DxfClass::new("EXACXREFPANELOBJECT", "ExAcXREFPanelObject");
    unused.application_name = "ObjectDBX Classes".into();
    document.classes.add_or_update(unused);
    let mut counted = acadrust::classes::DxfClass::new("CELLSTYLEMAP", "AcDbCellStyleMap");
    counted.instance_count = 3;
    document.classes.add_or_update(counted);
    let source = dxf_source(&document);
    for name in ["ACME_CUSTOM_OBJECT", "EXACXREFPANELOBJECT", "CELLSTYLEMAP"] {
        assert!(!source.classes.get_by_name(name).unwrap().was_zombie, "the DXF states {name} live");
    }
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027] {
        let out = write_and_read(&source, version);
        let names: Vec<_> = out.classes.iter().map(|class| class.dxf_name.as_str()).collect();
        let positions: Vec<_> = ["ACME_CUSTOM_OBJECT", "EXACXREFPANELOBJECT", "CELLSTYLEMAP"].iter().map(|name| names.iter().position(|n| n == name)).collect();
        assert!(positions.iter().all(Option::is_some), "{version:?}: the classes of the DXF are kept: {names:?}");
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]), "{version:?}: in the order of the DXF: {names:?}");
        for name in ["ACME_CUSTOM_OBJECT", "EXACXREFPANELOBJECT", "CELLSTYLEMAP"] {
            assert!(!out.classes.get_by_name(name).unwrap().was_zombie, "{version:?}: {name}");
        }
    }
}

// ── a table of a DWG before R2010 ──────────────────────────────────────────────────────────────────────────────────────

use acadrust::entities::table::{CellValue, CellValueType, Table};
use acadrust::objects::{Dictionary, XRecord};
use acadrust::types::Handle;

/// A drawing of R2004 with a table whose legacy cells hold texts and whose extension dictionary links the round-trip TABLECONTENT
/// AutoCAD keeps with it (the typed cells of the original R2010 table).
fn legacy_table_drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let mut legacy = Table::new(Vector3::ZERO, 1, 2);
    legacy.set_cell_text(0, 0, "0.500000");
    legacy.set_cell_text(0, 1, "60");
    // the legacy record's own words: the value flags of the entity and the edge flags of a cell (an R2010 record has neither)
    legacy.value_flags = 22;
    legacy.rows[0].cells[0].edge_flags = 9;
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

/// A table read from a DWG before R2010 holds the typed cells of the TABLECONTENT object AutoCAD keeps with it (an R2010 or later
/// file of the same table has them in the entity: sample_AC1018.dwg against sample_AC1024.dwg), so a drawing written at R2004 and
/// read again is the drawing: before R5b-8 the reader kept the flat cells, and the cross-version comparison of 41 cells found 1,350
/// rows in the tables alone. A file written from it at the version it was read from has the legacy cells again, unchanged.
#[test]
fn a_table_of_a_dwg_before_r2010_is_completed_from_its_round_trip_content() {
    let (document, handle) = legacy_table_drawing();
    let source = write_and_read(&document, DxfVersion::AC1018);
    let cells = &table_of(&source, handle).rows[0].cells;
    assert_eq!(cells[0].contents[0].value.value_type, CellValueType::Double);
    assert_eq!(cells[0].contents[0].value.numeric_value, 0.5);
    assert_eq!(cells[1].contents[0].value_format, "%au0");
    // the words only the legacy record has read as an R2010 record reads them (AutoCAD's R2010 file of the table: 0)
    assert_eq!((table_of(&source, handle).value_flags, cells[0].edge_flags), (0, 0));
    // written again at R2004, the file states the legacy cells it was read with: a second round trip changes nothing
    let again = write_and_read(&source, DxfVersion::AC1018);
    assert_eq!(table_of(&again, handle), table_of(&source, handle));
    // a cell edited after the file was read is no longer the object's: the edited text is kept
    let mut edited = source.clone();
    if let Some(EntityType::Table(table)) = edited.get_entity_mut(handle) {
        table.set_cell_text(0, 0, "7");
    }
    let after = write_and_read(&edited, DxfVersion::AC1018);
    assert_eq!(table_of(&after, handle).rows[0].cells[0].text_value(), "7");
}

// ── the ACadSharp view of what the readers took out of the records ────────────────────────────────────────────────────────

#[cfg(feature = "acadsharp-compat")]
mod view {
    use super::*;
    use acadrust::compat::acadsharp::dump_document;

    fn props(document: &CadDocument, kind: &str) -> serde_json::Value {
        dump_document(document, "", "new")["objects"].as_array().unwrap().iter().find(|object| object["type"] == kind).unwrap_or_else(|| panic!("no {kind}"))["props"].clone()
    }

    /// The oracle reads the columns of a DWG before R2018 as the block of the entity's extended data and no columns; the native model
    /// holds the columns the reader took out of the block (R5b-8), so the view projects the entity as the oracle reads it.
    #[test]
    fn the_view_of_a_dwg_before_r2018_shows_the_column_block_and_no_columns() {
        let read = write_and_read(&document_with_columns(), DxfVersion::AC1027);
        assert_eq!(columns_of(&read).0.column_type, 2, "native");
        let view = props(&read, "MText");
        assert_eq!(view["ColumnData"]["ColumnType"], 0);
        assert_eq!(view["HasColumns"], false);
        let records = view["ExtendedData"].to_string();
        assert!(records.contains("ACAD_MTEXT_COLUMN_INFO_BEGIN") && records.contains("ACAD_MTEXT_COLUMN_INFO_END"), "{records}");
    }

    /// The oracle reads the flat cells of a table in a DWG before R2010 (the value flags of the entity record among them); the
    /// native model holds the typed cells of the round-trip objects and an R2010 record's words (R5b-8).
    #[test]
    fn the_view_of_a_table_in_a_dwg_before_r2010_is_the_legacy_entity() {
        let (document, handle) = legacy_table_drawing();
        let source = write_and_read(&document, DxfVersion::AC1018);
        assert_eq!(table_of(&source, handle).value_flags, 0, "native");
        assert_eq!(props(&source, "TableEntity")["ValueFlag"], 22);
    }
}

/// An R2010 round-trip record that states the flags only (what AutoCAD's own R2010 files hold for a style of 28 properties) leaves
/// the style at its 28 properties when the file is read: only a record that also states values extends the style.
#[test]
fn a_record_of_flags_alone_leaves_a_visual_style_of_28_properties() {
    let (mut source, handle) = document_with_visual_style();
    if let Some(ObjectType::VisualStyle(style)) = source.objects.get_mut(&handle) {
        style.properties.truncate(28);
    }
    let record_handle = source.ensure_xrecord(handle, "ACAD_XREC_ROUNDTRIP");
    if let Some(ObjectType::XRecord(record)) = source.objects.get_mut(&record_handle) {
        for index in 0..63 {
            record.add_string(102, &format!("RTVSPropertyOp{index}"));
            record.add_int16(70, 1);
        }
    }
    let out = write_and_read(&source, DxfVersion::AC1024);
    assert_eq!(visual_style(&out).properties.len(), 28);
}
