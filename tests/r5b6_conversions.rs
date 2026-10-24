// SPDX-License-Identifier: MPL-2.0
//! R5b-6 (gate G5): the extended data AutoCAD adds when it saves to an older version and consumes when it saves to a newer
//! one (attribute lock, dimension text fill and line types, multileader version, modeler revision, the visual style and
//! colour-book stamps), seen by an actual DWG and DXF write and read. The records are those of AutoCAD's own files
//! sample_AC1014.dwg to sample_AC1032.dwg (see `io::dwg::autocad_conversions`). Synthetic documents only.

use std::io::Cursor;

use acadrust::entities::table::{CellValue, CellValueType, Table, TableCustomData};
use acadrust::entities::{AttributeDefinition, EntityType, MultiLeader};
use acadrust::objects::{Dictionary, MultiLeaderStyle, ObjectType, XRecord};
use acadrust::tables::DimStyle;
use acadrust::types::{Color, DxfVersion, Handle};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn through_dwg(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap()
}

struct Drawing {
    document: CadDocument,
    attribute: Handle,
    leader: Handle,
}

/// A drawing as AutoCAD R2018 holds it: the lock and the fill are native, the version stamps are absent.
fn modern_drawing() -> Drawing {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut attribute = AttributeDefinition::new("TAG".into(), "Prompt".into(), "".into());
    attribute.lock_position = true;
    let attribute = document.add_entity(EntityType::AttributeDefinition(attribute)).unwrap();
    let leader = document.add_entity(EntityType::MultiLeader(Box::new(MultiLeader::new()))).unwrap();
    let mut style = DimStyle::new("Filled");
    style.handle = document.allocate_handle();
    style.dimtfill = 2;
    style.dimtfillclr_true_color = Some(Color::from_rgb(0x36, 0x75, 0x42));
    style.dimtfillclr = 100;
    document.dim_styles.add(style).ok();
    let mut mleader_style = MultiLeaderStyle::new("Styled");
    mleader_style.handle = document.allocate_handle();
    document.objects.insert(mleader_style.handle, ObjectType::MultiLeaderStyle(mleader_style));
    Drawing { document, attribute, leader }
}

/// The extended data of the object `handle` as ACadSharp reads the file: one list of (group code, value) per application.
fn view_records(document: &CadDocument, handle: Handle) -> Vec<Vec<(i64, serde_json::Value)>> {
    let dump = acadrust::compat::acadsharp::dump_reader_document(document, "r5b6.dwg", "dwg");
    let object = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["handle"] == acadrust::compat::acadsharp::handle(handle))
        .unwrap_or_else(|| panic!("no object {handle:?}"));
    object["props"]["ExtendedData"]
        .as_array()
        .map(|pairs| {
            pairs
                .iter()
                .map(|pair| {
                    pair[1]["Records"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|record| (record["Code"].as_i64().unwrap(), record["RawValue"].clone()))
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ints(values: &[i64]) -> Vec<(i64, serde_json::Value)> {
    values.iter().map(|value| (1070, serde_json::json!(value))).collect()
}

fn model_has_record(document: &CadDocument, handle: Handle, application: &str) -> bool {
    document
        .get_entity(handle)
        .is_some_and(|entity| entity.common().extended_data.get_record(application).is_some())
}

/// A down-save to R2004 states the records AutoCAD's R2004 file states and ACadSharp reads them; the model of the drawing read holds
/// the values instead of the records; the up-save to R2010 carries the values and none of the records (sample_AC1018.dwg and
/// sample_AC1024.dwg).
#[test]
fn a_dwg_down_save_states_the_records_and_an_up_save_consumes_them() {
    let drawing = modern_drawing();
    let down = through_dwg(&drawing.document, DxfVersion::AC1018);
    let style = down.dim_styles.get("Filled").unwrap().handle;
    // ACadSharp reads the records the file has
    assert!(view_records(&down, drawing.attribute).contains(&ints(&[0, 1])));
    assert!(view_records(&down, drawing.leader).contains(&ints(&[2])));
    let fill = view_records(&down, style).into_iter().find(|record| record.first() == Some(&(1070, serde_json::json!(376)))).expect("the text fill record");
    assert_eq!(fill[1], (1070, serde_json::json!(2)));
    assert_eq!(fill[2], (1070, serde_json::json!(377)));
    assert_eq!(fill[3], (1004, serde_json::json!("1800000001000000427536c2000000000000000000000000")));
    // the model holds what they state
    assert!(!model_has_record(&down, drawing.attribute, "AcDbAttr"));
    assert!(!model_has_record(&down, drawing.leader, "ACAD_MLEADERVER"));
    assert!(down.object_xdata(style).is_none_or(|data| data.get_record("ACAD_DSTYLE_DIMTEXT_FILL").is_none()));
    match down.get_entity(drawing.attribute) {
        Some(EntityType::AttributeDefinition(attribute)) => assert!(attribute.lock_position),
        _ => panic!("attribute definition"),
    }
    let filled = down.dim_styles.get("Filled").unwrap();
    assert_eq!((filled.dimtfill, filled.dimtfillclr_true_color), (2, Some(Color::from_rgb(0x36, 0x75, 0x42))));
    // written as R2004 again, the records are where they were
    let again = through_dwg(&down, DxfVersion::AC1018);
    assert!(view_records(&again, drawing.attribute).contains(&ints(&[0, 1])));
    assert!(view_records(&again, drawing.leader).contains(&ints(&[2])));
    // up to R2010: values stay, records go
    let up = through_dwg(&down, DxfVersion::AC1024);
    match up.get_entity(drawing.attribute) {
        Some(EntityType::AttributeDefinition(attribute)) => assert!(attribute.lock_position),
        _ => panic!("attribute definition"),
    }
    assert!(view_records(&up, drawing.attribute).is_empty());
    assert!(view_records(&up, drawing.leader).is_empty());
    assert!(!view_records(&up, up.dim_styles.get("Filled").unwrap().handle).iter().any(|record| record.first() == Some(&(1070, serde_json::json!(376)))));
    let filled = up.dim_styles.get("Filled").unwrap();
    assert_eq!((filled.dimtfill, filled.dimtfillclr_true_color), (2, Some(Color::from_rgb(0x36, 0x75, 0x42))));
    // R2007 holds the lock and the fill natively but still states the multileader version (AutoCAD's R2007 file of the drawing)
    let r2007 = through_dwg(&down, DxfVersion::AC1021);
    assert!(view_records(&r2007, drawing.attribute).is_empty());
    assert!(view_records(&r2007, drawing.leader).contains(&ints(&[2])));
}

/// The DXF writer does the same for R2000 and R2004 (sample_AC1015_ascii.dxf and sample_AC1018_ascii.dxf hold the records).
#[test]
fn a_dxf_down_save_states_the_records_and_reading_it_restores_the_values() {
    let drawing = modern_drawing();
    let mut document = drawing.document.clone();
    document.version = DxfVersion::AC1018;
    let text = String::from_utf8(DxfWriter::new(&document).write_to_vec().unwrap()).unwrap();
    assert!(text.contains("AcDbAttr") && text.contains("ACAD_DSTYLE_DIMTEXT_FILL"), "the records are in the text");
    let read = DxfReader::from_reader(Cursor::new(text.into_bytes())).unwrap().read().unwrap();
    assert_eq!(read.version, DxfVersion::AC1018);
    let attribute = read
        .entities()
        .find_map(|entity| match entity {
            EntityType::AttributeDefinition(attribute) => Some(attribute),
            _ => None,
        })
        .expect("attribute definition");
    assert!(attribute.lock_position, "the lock is restored from the record");
    assert!(attribute.common.extended_data.get_record("AcDbAttr").is_none(), "and the record is not data of the model");
    assert!(view_records(&read, attribute.common.handle).contains(&ints(&[0, 1])), "ACadSharp reads the record");
    let style = read.dim_styles.get("Filled").unwrap();
    assert_eq!(style.dimtfill, 2);
    assert_eq!(style.dimtfillclr_true_color, Some(Color::from_rgb(0x36, 0x75, 0x42)));
    // written as R2010, the records are gone and the values stay
    let mut up = read.clone();
    up.version = DxfVersion::AC1024;
    let text = String::from_utf8(DxfWriter::new(&up).write_to_vec().unwrap()).unwrap();
    let again = DxfReader::from_reader(Cursor::new(text.into_bytes())).unwrap().read().unwrap();
    let attribute = again
        .entities()
        .find_map(|entity| match entity {
            EntityType::AttributeDefinition(attribute) => Some(attribute),
            _ => None,
        })
        .expect("attribute definition");
    assert!(view_records(&again, attribute.common.handle).is_empty());
    assert!(!view_records(&again, again.dim_styles.get("Filled").unwrap().handle).iter().any(|record| record.first() == Some(&(1070, serde_json::json!(376)))));
    assert_eq!(again.dim_styles.get("Filled").unwrap().dimtfill, 2);
}

/// A drawing of R2004 with a one-row table whose legacy cells show a number, an angle and a date, and whose extension dictionary
/// links the round-trip content AutoCAD keeps: the typed values and, as the first custom data item of every cell, the checksum
/// AutoCAD stores of the displayed text (sample_AC1018.dwg: "Text" 1110, "0.500000" 1739).
fn typed_table_drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let texts = ["0.500000", "60", "11/15/2024"];
    let mut legacy = Table::new(acadrust::types::Vector3::ZERO, 1, 3);
    for (column, text) in texts.iter().enumerate() {
        legacy.set_cell_text(0, column, text);
    }
    let mut content = legacy.clone();
    for (column, (value_type, code, number, format, unit)) in
        [(CellValueType::Double, 2, 0.5, "", 0), (CellValueType::Double, 2, 1.0471975511965976, "%au0", 2), (CellValueType::Date, 8, 0.0, "", 0)]
            .into_iter()
            .enumerate()
    {
        let cell = &mut content.rows[0].cells[column];
        let item = &mut cell.contents[0];
        item.value = CellValue::new();
        item.value.value_type = value_type;
        item.value.raw_type_code = code;
        item.value.numeric_value = number;
        item.format_value_data_type = code;
        item.format_value_unit_type = unit;
        item.value_format = format.to_string();
        item.format_has_data = true;
        if code == 8 {
            item.value.binary_value = vec![0xF0, 0x80, 0x36, 0x67, 0, 0, 0, 0];
            item.value.data_size = 8;
        }
        let mut checksum = CellValue::new();
        checksum.value_type = CellValueType::Double;
        checksum.numeric_value =
            texts[column].encode_utf16().enumerate().map(|(index, unit)| (index as f64 + 1.0) * f64::from(unit)).sum();
        cell.custom_data_items.push(TableCustomData { name: "ACAD_ROUNDTRIP_2008_CELL_CHECKSUM".into(), value: checksum });
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

fn cells(document: &CadDocument, handle: Handle) -> Vec<acadrust::entities::table::TableCell> {
    match document.get_entity(handle) {
        Some(EntityType::Table(table)) => table.rows[0].cells.clone(),
        _ => panic!("no table"),
    }
}

/// The checksum of the displayed text tells AutoCAD whether a typed cell was edited after the file was saved: an edited date,
/// angle or number with a format keeps the edit; an unedited one takes its typed value from the content object.
#[test]
fn an_edited_typed_table_cell_keeps_its_edit_when_the_table_is_written_as_r2010() {
    let (document, handle) = typed_table_drawing();
    let source = through_dwg(&document, DxfVersion::AC1018);
    let up = through_dwg(&source, DxfVersion::AC1024);
    let kept = cells(&up, handle);
    assert_eq!(kept[2].contents[0].value.value_type, CellValueType::Date, "unedited: the typed date");
    assert_eq!(kept[1].contents[0].value_format, "%au0", "unedited: the typed angle");
    assert_eq!(kept[0].contents[0].value.numeric_value, 0.5, "unedited: the typed number");
    // a plain number edited into a text that no longer parses as a number is an edit too (the checksum tells it)
    for (column, shown) in [(0, "n/a"), (1, "90"), (2, "11/16/2024")] {
        let mut edited = source.clone();
        if let Some(EntityType::Table(table)) = edited.get_entity_mut(handle) {
            table.set_cell_text(0, column, shown);
        }
        let up = through_dwg(&edited, DxfVersion::AC1024);
        let cells = cells(&up, handle);
        assert_eq!(cells[column].contents[0].value.text, shown, "the edit of column {column} survives");
        assert_ne!(cells[column].contents[0].value.value_type, CellValueType::Date, "no stale date in column {column}");
        assert_ne!(cells[column].contents[0].value.numeric_value, 1.0471975511965976, "no stale angle in column {column}");
        assert_ne!(cells[column].contents[0].value.numeric_value, 0.5, "no stale number in column {column}");
    }
}

/// AutoCAD's R2004 file states the default gradient name `LINEAR` for a hatch whose gradient is off (the 8 hatches of
/// sample_AC1018.dwg); its R2000 file of the same hatches has no name to state (sample_AC1015.dwg: the colours only). A drawing read
/// from an R2000 file and written as R2004 states it.
#[test]
fn a_hatch_without_gradient_of_an_r2000_drawing_states_the_default_name_in_r2004() {
    use acadrust::entities::Hatch;
    let mut document = CadDocument::with_version(DxfVersion::AC1015);
    let mut hatch = Hatch::new();
    hatch.is_solid = false;
    let handle = document.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
    let source = through_dwg(&document, DxfVersion::AC1015);
    assert_eq!(source.dwg_source_version, Some(DxfVersion::AC1015));
    let up = through_dwg(&source, DxfVersion::AC1018);
    match up.get_entity(handle) {
        Some(EntityType::Hatch(hatch)) => {
            assert!(!hatch.gradient_color.enabled);
            assert_eq!(hatch.gradient_color.name, "LINEAR");
        }
        _ => panic!("hatch"),
    }
    // a drawing that is read from an R2004 file keeps what the file says
    let again = through_dwg(&up, DxfVersion::AC1024);
    match again.get_entity(handle) {
        Some(EntityType::Hatch(hatch)) => assert_eq!(hatch.gradient_color.name, "LINEAR"),
        _ => panic!("hatch"),
    }
}

fn tagged(tag: u8, text: &str) -> Vec<u8> {
    let mut out = vec![tag, text.len() as u8];
    out.extend(text.as_bytes());
    out
}

fn number(tag: u8, value: i32) -> Vec<u8> {
    let mut out = vec![tag];
    out.extend(value.to_le_bytes());
    out
}

/// The binary modeler data of AutoCAD's save versions as the three bodies of sample_AC1018.dwg to sample_AC1032.dwg show them
/// (header words, an `asmheader` record, one vertex, one coedge, the end record).
fn modeler_data(version: i32) -> Vec<u8> {
    let flags = [(20800, 0), (21200, 26), (21500, 24), (21800, 12), (22300, 4)].iter().find(|(v, _)| *v == version).unwrap().1;
    let mut out = if version == 22300 { b"ASM BinaryFile4".to_vec() } else { b"ACIS BinaryFile".to_vec() };
    for word in [version, 0, 2, flags] {
        out.extend(word.to_le_bytes());
    }
    let record = |parts: Vec<Vec<u8>>| {
        let mut out = parts.concat();
        out.push(0x11);
        out
    };
    let double = |value: f64| {
        let mut out = vec![0x06];
        out.extend(value.to_le_bytes());
        out
    };
    // the product, its version and the date, the tolerances and the `asmheader` record are one record
    out.extend(record(vec![
        tagged(0x07, "Autodesk AutoCAD"),
        tagged(0x07, "ASM 230.7.0.65535 NT"),
        tagged(0x07, "Tue Dec  2 10:27:20 2025"),
        double(25.4),
        double(1e-6),
        double(1e-10),
        tagged(0x0D, "asmheader"),
        number(0x0C, -1),
        number(0x04, -1),
        tagged(0x07, "230.7.0.65535"),
    ]));
    let mut vertex = vec![tagged(0x0D, "vertex"), number(0x0C, 74), number(0x04, -1), number(0x0C, -1), number(0x0C, 23)];
    if version >= 21200 {
        vertex.push(number(0x04, 1));
    }
    vertex.push(number(0x0C, 75));
    out.extend(record(vertex));
    let mut coedge = vec![tagged(0x0D, "coedge"), number(0x0C, 3), number(0x04, -1)];
    coedge.extend((4..9).map(|index| number(0x0C, index)));
    coedge.extend([vec![0x0B], number(0x0C, 9)]);
    if version >= 21800 {
        coedge.push(number(0x04, 0));
    }
    coedge.push(number(0x0C, 10));
    out.extend(record(coedge));
    out.extend([tagged(0x0E, "End"), tagged(0x0E, "of"), tagged(0x0E, if version >= 21800 { "ASM" } else { "ACIS" }), tagged(0x0D, "data")].concat());
    out
}

/// A drawing of R2018 written as R2004, R2007, R2010 or R2013 holds the modeler data in that release's save version (AutoCAD's own
/// files: header words, one integer less per vertex from R2007 on and per coedge from R2013 on).
#[test]
fn modeler_data_of_a_newer_release_takes_the_save_version_of_an_older_drawing() {
    use acadrust::entities::Solid3D;
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut solid = Solid3D::new();
    solid.acis_data.is_binary = true;
    solid.acis_data.sab_data = modeler_data(22300);
    let handle = document.add_entity(EntityType::Solid3D(solid)).unwrap();
    for (version, save) in [
        (DxfVersion::AC1018, 20800),
        (DxfVersion::AC1021, 21200),
        (DxfVersion::AC1024, 21500),
    ] {
        let read = through_dwg(&document, version);
        match read.get_entity(handle) {
            Some(EntityType::Solid3D(solid)) => {
                assert!(solid.acis_data.sab_data.starts_with(b"ACIS BinaryFile"), "{version:?}: the magic of the older releases");
                assert_eq!(solid.acis_data.sab_data, modeler_data(save), "{version:?}: the save version {save}");
            }
            _ => panic!("solid"),
        }
    }
    // The current release keeps the data as it is.
    let same = through_dwg(&document, DxfVersion::AC1032);
    match same.get_entity(handle) {
        Some(EntityType::Solid3D(solid)) => assert!(solid.acis_data.sab_data.starts_with(b"ASM BinaryFile4")),
        _ => panic!("solid"),
    }
}
