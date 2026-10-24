// SPDX-License-Identifier: MPL-2.0
//! R5b-5 (gate G5): conversions found by comparing the fork's down- and up-saves with AutoCAD's own files of one drawing
//! (sample_AC1014.dwg to sample_AC1032.dwg, their C# dumps under Tests/CadCoreTests/Fixtures/dumps). Synthetic documents only.
#![cfg(feature = "acadsharp-compat")]

use std::io::Cursor;

use acadrust::entities::table::{CellStyle, CellValue, CellValueType, Table};
use acadrust::entities::{EntityType, Leader, MText};
use acadrust::objects::{Dictionary, ObjectType, XRecord};
use acadrust::types::{Color, DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn through_dwg(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap()
}

/// A drawing of R2004 with a table whose legacy cells hold texts and whose extension dictionary links the round-trip
/// TABLECONTENT AutoCAD keeps (the typed cells of the original R2010 table).
fn legacy_table_drawing() -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let mut legacy = Table::new(Vector3::ZERO, 1, 3);
    legacy.set_cell_text(0, 0, "0.500000");
    legacy.set_cell_text(0, 1, "60");
    legacy.set_cell_text(0, 2, "11/15/2024");
    let mut content = legacy.clone();
    for (column, (value_type, code, number, format, unit)) in
        [(CellValueType::Double, 2, 0.5, "", 0), (CellValueType::Double, 2, 1.0471975511965976, "%au0", 2), (CellValueType::Date, 8, 0.0, "", 0)]
            .into_iter()
            .enumerate()
    {
        let item = &mut content.rows[0].cells[column].contents[0];
        item.value = CellValue::new();
        item.value.value_type = value_type;
        item.value.raw_type_code = code;
        item.value.numeric_value = number;
        item.format_value_data_type = code;
        item.format_value_unit_type = unit;
        item.value_format = format.to_string();
        item.format_has_data = true;
        if code == 8 {
            // 2024-11-15 local midnight, as the object states it: 32-bit seconds since 1970 (here 23:00 UTC the day before).
            item.value.binary_value = vec![0xF0, 0x80, 0x36, 0x67, 0, 0, 0, 0];
            item.value.data_size = 8;
        }
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

fn table(document: &CadDocument, handle: Handle) -> Table {
    match document.get_entity(handle) {
        Some(EntityType::Table(table)) => (**table).clone(),
        _ => panic!("no table"),
    }
}

/// AutoCAD's up-conversion takes the typed cells from the round-trip TABLECONTENT of the legacy file (sample_AC1018.dwg to
/// sample_AC1024.dwg: the date, angle and currency cells), with the legacy cell's text as the display and AutoCAD's value flags.
#[test]
fn a_legacy_table_written_as_r2010_takes_its_typed_cells_from_the_round_trip_content() {
    let (document, handle) = legacy_table_drawing();
    let source = through_dwg(&document, DxfVersion::AC1018);
    assert_eq!(source.dwg_source_version, Some(DxfVersion::AC1018));
    let up = through_dwg(&source, DxfVersion::AC1024);
    let cells = &table(&up, handle).rows[0].cells;
    let value = |column: usize| cells[column].contents[0].clone();
    assert_eq!(value(0).value.value_type, CellValueType::Double);
    assert_eq!(value(0).value.numeric_value, 0.5);
    assert_eq!(value(0).format_value_data_type, 2);
    assert_eq!(value(0).value.formatted_value, "0.500000");
    assert_eq!(value(0).value.flags, 6, "a value without a format");
    assert_eq!(value(1).value_format, "%au0");
    assert_eq!(value(1).value.format, "%au0");
    assert_eq!(value(1).value.flags, 4, "a value with a format");
    assert_eq!(value(1).format_value_unit_type, 2);
    // The date is the SYSTEMTIME of the day: 2024-11-15, a Friday.
    assert_eq!(value(2).value.binary_value, [0xE8, 7, 0x0B, 0, 5, 0, 0x0F, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // A cell edited after the file was saved keeps the converted legacy cell.
    let mut edited = source.clone();
    if let Some(EntityType::Table(table)) = edited.get_entity_mut(handle) {
        table.set_cell_text(0, 0, "7");
    }
    let up = through_dwg(&edited, DxfVersion::AC1024);
    let cells = &table(&up, handle).rows[0].cells;
    assert_eq!(cells[0].contents[0].value.text, "7");
    assert_ne!(cells[0].contents[0].value.value_type, CellValueType::Double);
}

/// The round-trip record of a drawing before R2007 goes on with the markers and handles of the pre-R2007 table; they are
/// not the content object (sample_AC1018.dwg: `360 0xF7A ... 102 ACAD_ROUNDTRIP_PRE2007_TABLE ... 360 0xFEF`).
#[test]
fn the_pre_r2007_markers_of_the_round_trip_record_do_not_replace_the_content_handle() {
    let (mut document, handle) = legacy_table_drawing();
    let dictionary = document.extension_dictionary_handle(handle).unwrap();
    let record = match document.objects.get(&dictionary) {
        Some(ObjectType::Dictionary(dictionary)) => dictionary.get("ACAD_XREC_ROUNDTRIP").unwrap(),
        _ => panic!(),
    };
    if let Some(ObjectType::XRecord(record)) = document.objects.get_mut(&record) {
        record.add_string(102, "ACAD_ROUNDTRIP_PRE2007_TABLE");
        record.add_int32(90, 4);
        record.add_string(102, "ACAD_ROUNDTRIP_PRE2007_TABLECELL");
        record.add_handle(360, Handle::new(0x7777));
    }
    let source = through_dwg(&document, DxfVersion::AC1018);
    let up = through_dwg(&source, DxfVersion::AC1024);
    assert_eq!(table(&up, handle).rows[0].cells[1].contents[0].value_format, "%au0");
}

fn modern_table() -> Table {
    // The title row of sample_AC1032.dwg's table #528: a text height of its own (0.1425), a last column whose cells take
    // the right edge from the inside horizontal border.
    let mut table = Table::new(Vector3::ZERO, 2, 2);
    let mut row_style = CellStyle::new();
    row_style.override_flags = 0x80;
    row_style.text_height = 0.14249252057197062;
    row_style.margin_left = 0.06;
    table.rows[0].style = Some(row_style);
    table.rows[1].style = None;
    table
}

/// A legacy cell that overrides no text height states the text height its row states (sample_AC1015.dwg: 0.1425 for the cells
/// of the title row, whose R2010 row style states it).
#[test]
fn a_legacy_cell_takes_the_text_height_of_its_row_style() {
    use acadrust::entities::table::{CellStylePropertyFlags, TableCell};
    let table = modern_table();
    let mut cell = TableCell::new();
    let mut style = CellStyle::new();
    style.property_flags = CellStylePropertyFlags::BACKGROUND_COLOR;
    style.override_flags = 0x04;
    style.background_color = Color::Index(1);
    cell.style = Some(style);
    let converted = cell.legacy_layout_in(table.rows[0].style.as_ref(), false);
    let style = converted.style.as_ref().unwrap();
    assert_ne!(style.override_flags & 0x20, 0);
    assert_eq!(style.text_height, 0.14249252057197062);
    // A row without a text height of its own leaves the cell as it is.
    let plain = cell.legacy_layout_in(table.rows[1].style.as_ref(), false);
    assert_eq!(plain.style.as_ref().unwrap().override_flags & 0x20, 0);
}

/// The leader of a drawing from R2010 or later states no annotation extents; AutoCAD's older files give those of the MTEXT.
#[test]
fn a_leader_of_an_old_version_states_the_extents_of_its_mtext_annotation() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = MText::new();
    text.value = "Sample annotation".into();
    text.extents_width = 16.37400410337938;
    text.extents_height = 1.844821309997273;
    let annotation = document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    let mut leader = Leader::new();
    leader.annotation_handle = annotation;
    leader.vertices = vec![Vector3::ZERO, Vector3::new(5.0, 5.0, 0.0)];
    let leader_handle = document.add_entity(EntityType::Leader(leader)).unwrap();
    for (version, extents) in [(DxfVersion::AC1015, true), (DxfVersion::AC1018, true), (DxfVersion::AC1032, false)] {
        let read = through_dwg(&document, version);
        let Some(EntityType::Leader(leader)) = read.get_entity(leader_handle) else { panic!("no leader") };
        if extents {
            assert_eq!((leader.text_height, leader.text_width), (1.844821309997273, 16.37400410337938), "{version:?}");
        } else {
            // R2010 and later have no such fields: the default stays.
            assert_eq!(leader.text_width, 0.0, "{version:?}");
        }
    }
}

/// A DXF of R2000 states the nearest index for a true colour and no round-trip record (R5b-7, owner decision: AutoCAD's own
/// R2000 and R2004 DXF files hold none; sample_AC1015_ascii.dxf states colour 230 for the point #29A and nothing else); a DXF
/// that has the record (an older output of this library, another producer's) is still restored on read.
#[test]
fn a_dxf_of_r2000_states_the_nearest_index_and_no_round_trip_record() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut line = acadrust::entities::Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.color = Color::from_rgb(245, 10, 147);
    let handle = document.add_entity(EntityType::Line(line)).unwrap();
    document.version = DxfVersion::AC1015;
    let source = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&document).unwrap())).read().unwrap();
    assert_eq!(source.get_entity(handle).unwrap().common().color, Color::from_rgb(245, 10, 147));
    let text = String::from_utf8(DxfWriter::new(&source).write_to_vec().unwrap()).unwrap();
    assert!(!text.contains("RTTcAl"), "AutoCAD's DXF holds no record");
    assert!(!text.contains("\n420\n") && !text.contains("\r\n420\r\n"), "R2000 has no true colour group");
    let back = DxfReader::from_reader(Cursor::new(text.clone().into_bytes())).unwrap().read().unwrap();
    assert_eq!(back.get_entity(handle).unwrap().common().color, Color::Index(230));

    // A file that states the record (word 0x80E6: not an index, nearest index 230; value 0xC2F50A93) gets its value back.
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let entities = text.find(&format!("2{eol}ENTITIES")).expect("entities section");
    let end = entities + text[entities..].find("ENDSEC").expect("end of the entities");
    let code_line = text[..end].rfind('\n').expect("the line of group 0");
    let at = text[..code_line].rfind('\n').expect("the line before it") + 1;
    let record = ["1001", "ACAD", "1000", "RTTcAl", "1070", "-32538", "1071", "-1024128365", "1000", "$$NoName$$"].join(eol);
    let stated = format!("{}{record}{eol}{}", &text[..at], &text[at..]);
    let restored = DxfReader::from_reader(Cursor::new(stated.into_bytes())).unwrap().read().unwrap();
    let common = restored.get_entity(handle).unwrap().common();
    assert_eq!(common.color, Color::from_rgb(245, 10, 147));
    assert!(common.extended_data.get_record("ACAD").is_none());
}

/// A cell of the last column takes its right edge from the inside horizontal border (sample_AC1015.dwg: edge flags 11 for the
/// last-column cells of a merged block whose cells to the left have 9).
#[test]
fn the_last_column_takes_its_right_edge_from_the_inside_horizontal_border() {
    use acadrust::entities::table::{BorderPropertyFlags, CellEdgeFlags, CellStylePropertyFlags, TableCell};
    let mut style = CellStyle::new();
    style.override_flags = 0x01;
    style.property_flags = CellStylePropertyFlags::ALIGNMENT;
    style.applied_border_edges = CellEdgeFlags::TOP | CellEdgeFlags::RIGHT | CellEdgeFlags::LEFT;
    style.top_border.override_flags = BorderPropertyFlags::COLOR | BorderPropertyFlags::LINE_WEIGHT | BorderPropertyFlags::INVISIBILITY;
    style.left_border.override_flags = style.top_border.override_flags;
    let mut inside = style.top_border.clone();
    inside.line_weight = acadrust::types::LineWeight::Value(40);
    style.additional_borders.push((CellEdgeFlags::INSIDE_HORIZONTAL.bits() as u32, inside));
    let mut cell = TableCell::new();
    cell.style = Some(style);
    assert_eq!(cell.legacy_layout_in(None, false).edge_flags, 9);
    let last = cell.legacy_layout_in(None, true);
    assert_eq!(last.edge_flags, 11);
    assert_eq!(last.style.as_ref().unwrap().right_border.line_weight, acadrust::types::LineWeight::Value(40));
}

/// A style read from a DWG before R2010 has no attachment fields; AutoCAD's later files of it state 9 for both.
#[test]
fn a_multileader_style_of_an_old_dwg_is_written_with_the_attachments_autocad_states() {
    use acadrust::objects::{MultiLeaderStyle, TextAttachmentType};
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let root = document.header.named_objects_dict_handle;
    let mut style = MultiLeaderStyle::new("Probe");
    style.handle = document.allocate_handle();
    style.owner_handle = root;
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::MultiLeaderStyle(style));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&root) {
        entries.add_entry("Probe", handle);
    }
    let source = through_dwg(&document, DxfVersion::AC1018);
    let up = through_dwg(&source, DxfVersion::AC1032);
    let Some(ObjectType::MultiLeaderStyle(style)) = up.objects.get(&handle) else { panic!("style lost") };
    assert_eq!(style.text_top_attachment, TextAttachmentType::CenterOfText);
    assert_eq!(style.text_bottom_attachment, TextAttachmentType::CenterOfText);
}

/// A block cell of the R2010 layout states the scale of its block in its content format; the legacy cell states it in its own
/// field (sample_AC1015.dwg: BlockScale 0.00596 where the R2010 cell has a format scale of 0.00596).
#[test]
fn a_block_cell_written_before_r2010_states_the_scale_of_its_content() {
    use acadrust::entities::table::TableCell;
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut block_table = Table::new(Vector3::ZERO, 1, 1);
    let mut cell = TableCell::block(Handle::new(0x77));
    cell.contents[0].scale = 0.005963880485348642;
    block_table.rows[0].cells[0] = cell;
    let handle = document.add_entity(EntityType::Table(Box::new(block_table))).unwrap();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let read = through_dwg(&document, version);
        assert_eq!(table(&read, handle).rows[0].cells[0].block_scale, 0.005963880485348642, "{version:?}");
    }
}
