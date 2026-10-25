// SPDX-License-Identifier: MPL-2.0
//! Job R5a-5 (gate G4-RR, the remaining DWG-sourced losses): each test builds a synthetic document, writes it as
//! DWG and/or DXF, reads it back and checks the datum the fix is about. No external file is used.
use std::io::Cursor;

use acadrust::entities::{
    CellContent, CellStyle, CellType, EntityType, Point, Table, TableBreakRange, TableCell, TableCellContentType,
    Viewport,
};
use acadrust::objects::{
    ClassObject, ClassObjectData, DataObject, DataObjectData, DataTable, DataTableCellType, DataTableColumn,
    DataTableValue, Dictionary, DictionaryVariable, Material, ObjectType, Scale, TableGeometry, TableGeometryCell,
    XRecordEntry, XRecordValue,
};
use acadrust::types::{Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfVersion, DxfWriter};

fn dwg(doc: &CadDocument) -> CadDocument {
    let bytes = DwgWriter::write_to_vec(doc).expect("DWG write");
    DwgReader::from_stream(Cursor::new(bytes)).read().expect("DWG read")
}

fn dxf_text(doc: &CadDocument) -> String {
    String::from_utf8_lossy(&DxfWriter::new(doc).write_to_vec().expect("DXF write")).into_owned()
}

fn dxf(doc: &CadDocument) -> CadDocument {
    DxfReader::from_reader(Cursor::new(DxfWriter::new(doc).write_to_vec().expect("DXF write")))
        .expect("DXF reader")
        .read()
        .expect("DXF read")
}

fn dxf_binary(doc: &CadDocument) -> CadDocument {
    DxfReader::from_reader(Cursor::new(DxfWriter::new_binary(doc).write_to_vec().expect("binary DXF write")))
        .expect("DXF reader")
        .read()
        .expect("DXF read")
}

fn first_table(doc: &CadDocument) -> &Table {
    doc.entities()
        .find_map(|entity| match entity {
            EntityType::Table(table) => Some(&**table),
            _ => None,
        })
        .expect("a table")
}

fn built_in_material(doc: &mut CadDocument, name: &str) -> Handle {
    let mut material = Material::new();
    material.name = name.into();
    material.handle = doc.allocate_handle();
    let handle = material.handle;
    doc.objects.insert(handle, ObjectType::Material(material));
    handle
}

// ---------------------------------------------------------------------------------------------------------------
// POINT material flag (ByBlock)
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn the_by_block_material_flag_travels_as_group_347_and_comes_back_as_the_flag() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let by_block = built_in_material(&mut doc, "ByBlock");
    let own = built_in_material(&mut doc, "Brick");
    let mut flagged = Point::new();
    flagged.common.material_flags = 1;
    let flagged = doc.add_entity(EntityType::Point(flagged)).unwrap();
    let mut explicit = Point::new();
    explicit.common.material_flags = 0b11;
    explicit.common.material_handle = Some(own);
    let explicit = doc.add_entity(EntityType::Point(explicit)).unwrap();
    let plain = doc.add_entity(EntityType::Point(Point::new())).unwrap();

    let text = dxf_text(&doc);
    assert!(text.contains(&format!("347\r\n{:X}\r\n", by_block.value())), "the flag names the ByBlock material");

    for back in [dxf(&doc), dxf_binary(&doc)] {
        let common = |handle: Handle| back.get_entity(handle).expect("point").common().clone();
        assert_eq!((common(flagged).material_flags, common(flagged).material_handle), (1, None));
        assert_eq!((common(explicit).material_flags, common(explicit).material_handle), (0b11, Some(own)));
        assert_eq!((common(plain).material_flags, common(plain).material_handle), (0, None));
    }
}

// ---------------------------------------------------------------------------------------------------------------
// $ACADMAINTVER
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn a_dwg_of_r2004_to_r2013_states_its_maintenance_byte_as_acadmaintver() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1027] {
        // The writer states its own byte; the file of another release states another one at offset 0x0B.
        let mut bytes = DwgWriter::write_to_vec(&CadDocument::with_version(version)).expect("DWG write");
        bytes[0x0B] = 77;
        let read = DwgReader::from_stream(Cursor::new(bytes)).read().expect("DWG read");
        let byte = read.maintenance_version;
        assert_eq!(byte, 77, "{version:?}");
        assert_eq!(read.header.maintenance_version, i32::from(byte), "{version:?}: the header states the byte");
        // The DXF carries the same number, and reads back as the DWG read does.
        let text = dxf_text(&read);
        assert!(text.contains(&format!("$ACADMAINTVER\r\n 70\r\n{byte:>6}\r\n")), "{version:?}: {byte}");
        assert_eq!(dxf(&read).header.maintenance_version, i32::from(byte), "{version:?}");
    }
    // R2000: AutoCAD's byte (15 in sample_AC1015.dwg) is not the variable its DXF states (20); neither the header
    // nor the DXF takes the byte.
    let mut doc = CadDocument::with_version(DxfVersion::AC1015);
    doc.maintenance_version = 15;
    let read = dwg(&doc);
    assert_eq!(read.maintenance_version, 15, "the R2000 writer states the document's release");
    assert_eq!(read.header.maintenance_version, 0);
    assert!(dxf_text(&read).contains("$ACADMAINTVER\r\n 70\r\n     0\r\n"));
    assert_eq!(dxf(&read).header.maintenance_version, 0);
    // R2018 and later: the header variable is a DXF one, and a DWG states no such number.
    let read = dwg(&CadDocument::with_version(DxfVersion::AC1032));
    assert_eq!(read.header.maintenance_version, 0);
}

// ---------------------------------------------------------------------------------------------------------------
// DATATABLE with a hard-owner column
// ---------------------------------------------------------------------------------------------------------------

fn data_table_with_owned_column(owner: Handle) -> ObjectType {
    let mut object = ClassObject::new(ClassObjectData::DataTable(DataTable {
        flags: 2,
        name: String::new(),
        row_count: 2,
        columns: vec![
            DataTableColumn {
                value_type: DataTableCellType::Double as i32,
                name: String::new(),
                rows: vec![
                    DataTableValue { real: 7783.0, ..Default::default() },
                    DataTableValue { real: 0.5, ..Default::default() },
                ],
            },
            DataTableColumn {
                value_type: DataTableCellType::HardOwnerId as i32,
                name: String::new(),
                rows: vec![
                    DataTableValue { handle: Handle::new(0x1440), ..Default::default() },
                    DataTableValue { handle: Handle::new(0x1441), ..Default::default() },
                ],
            },
        ],
    }));
    object.owner = owner;
    ObjectType::ClassObject(object)
}

fn data_table_of(doc: &CadDocument) -> &DataTable {
    doc.objects
        .values()
        .find_map(|object| match object {
            ObjectType::ClassObject(ClassObject { data: ClassObjectData::DataTable(table), .. }) => Some(table),
            _ => None,
        })
        .expect("a DATATABLE")
}

#[test]
fn a_data_table_column_of_hard_owned_handles_survives_dwg_and_both_dxf_encodings() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        let handle = doc.allocate_handle();
        let owner = doc.header.named_objects_dict_handle;
        let mut object = data_table_with_owned_column(owner);
        if let ObjectType::ClassObject(object) = &mut object {
            object.handle = handle;
        }
        doc.objects.insert(handle, object);
        let expected = data_table_of(&doc).clone();
        assert_eq!(data_table_of(&dwg(&doc)), &expected, "{version:?}: DWG");
        assert_eq!(data_table_of(&dxf(&doc)), &expected, "{version:?}: DXF");
        assert_eq!(data_table_of(&dxf_binary(&doc)), &expected, "{version:?}: binary DXF");
        assert!(dxf_text(&doc).contains("360\r\n1440\r\n"), "the owned handle is group 360");
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Extended data, layouts, dimension styles and viewports of a pre-R2000 DWG
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn application_names_of_extended_data_are_compared_without_regard_to_case() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1014);
    let mut layer = acadrust::tables::Layer::new("DESCRIBED");
    layer.handle = doc.allocate_handle();
    layer.description = "This layer has its own description".into();
    let handle = layer.handle;
    doc.layers.add(layer).unwrap();
    // A DWG before R2000 keeps application names in upper case.
    let mut data = acadrust::xdata::ExtendedData::default();
    let mut record = acadrust::xdata::ExtendedDataRecord::new("ACAECLAYERSTANDARD");
    record.values.push(acadrust::xdata::XDataValue::String(String::new()));
    record.values.push(acadrust::xdata::XDataValue::String("This layer has its own description".into()));
    data.add_record(record);
    assert!(data.get_record("AcAecLayerStandard").is_some());
    doc.object_xdata.insert(handle, data);
    let back = dxf(&doc);
    assert_eq!(
        back.layers.get("DESCRIBED").expect("layer").description,
        "This layer has its own description"
    );
}

#[test]
fn layout_dictionary_keys_that_differ_from_the_layout_names_only_in_case_are_written_as_they_are() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let dictionary = doc.header.acad_layout_dict_handle;
    let mut keys = Vec::new();
    if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&dictionary) {
        for (key, _) in dictionary.entries.iter_mut() {
            *key = key.to_uppercase();
            keys.push(key.clone());
        }
    }
    assert!(!keys.is_empty());
    let back = dxf(&doc);
    let read = match back.objects.get(&back.header.acad_layout_dict_handle) {
        Some(ObjectType::Dictionary(dictionary)) => dictionary,
        _ => panic!("layout dictionary"),
    };
    let mut read_keys: Vec<_> = read.entries.iter().map(|(key, _)| key.clone()).collect();
    read_keys.sort();
    keys.sort();
    assert_eq!(read_keys, keys, "the keys are not rewritten with the layout names");
}

#[test]
fn the_by_block_colour_of_an_r14_leader_travels_as_group_77() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1014);
    let mut leader = acadrust::entities::Leader::new();
    leader.add_vertex(Vector3::new(0.0, 0.0, 0.0));
    leader.add_vertex(Vector3::new(10.0, 10.0, 0.0));
    leader.byblock_color = 5;
    doc.add_entity(EntityType::Leader(leader)).unwrap();
    let colours = |doc: &CadDocument| {
        doc.entities()
            .find_map(|entity| match entity {
                EntityType::Leader(leader) => Some((leader.byblock_color, leader.override_color)),
                _ => None,
            })
            .expect("leader")
    };
    let read = dwg(&doc);
    assert_eq!(colours(&read), (5, acadrust::types::Color::Index(5)), "the R14 record states group 77");
    assert!(dxf_text(&read).contains(" 77\r\n     5\r\n"));
    assert_eq!(colours(&dxf(&read)), colours(&read));
}

#[test]
fn a_dimension_style_record_before_r2007_keeps_the_model_defaults_of_the_fields_it_lacks() {
    let doc = CadDocument::with_version(DxfVersion::AC1018);
    let read = dwg(&doc);
    let style = read.dim_styles.get("Standard").expect("Standard");
    assert_eq!(style.dimfxl, 1.0);
    assert_eq!(style.dimjogang, std::f64::consts::FRAC_PI_4);
    let back = dxf(&read);
    let again = back.dim_styles.get("Standard").expect("Standard");
    assert_eq!((again.dimfxl, again.dimjogang), (style.dimfxl, style.dimjogang));
}

#[test]
fn an_r14_viewport_has_the_world_axes_and_gets_its_view_parameters_back_from_the_dxf() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1014);
    let mut viewport = Viewport::new();
    viewport.width = 10.0;
    viewport.height = 5.0;
    viewport.view_height = 20.0;
    viewport.ucs_at_origin = true;
    let handle = doc.add_entity(EntityType::Viewport(Box::new(viewport))).unwrap();
    let read = dwg(&doc);
    let EntityType::Viewport(from_dwg) = read.get_entity(handle).expect("viewport") else { panic!() };
    assert_eq!(from_dwg.ucs_x_axis, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(from_dwg.ucs_y_axis, Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(from_dwg.custom_scale, 0.25);
    // R14 states the flag as bit 2 of the UCS icon word of its MVIEW data, which a reader puts into the status word (the icon at the
    // origin, as AutoCAD's own R2000 file of its R14 drawing states it: sample_AC1014.dwg and sample_AC1015.dwg).
    assert!(from_dwg.status.ucs_icon_at_origin);
    assert!(!from_dwg.ucs_at_origin);
    let back = dxf(&read);
    let EntityType::Viewport(from_dxf) = back.get_entity(handle).expect("viewport") else { panic!() };
    assert_eq!(from_dxf.custom_scale, from_dwg.custom_scale);
    assert_eq!(from_dxf.ucs_at_origin, from_dwg.ucs_at_origin);
    assert_eq!(from_dxf.status.ucs_icon_at_origin, from_dwg.status.ucs_icon_at_origin);
    assert_eq!(from_dxf.view_height, from_dwg.view_height);
    assert_eq!((from_dxf.ucs_x_axis, from_dxf.ucs_y_axis), (from_dwg.ucs_x_axis, from_dwg.ucs_y_axis));
}

// ---------------------------------------------------------------------------------------------------------------
// The current annotation scale
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn the_dxf_reader_takes_the_current_annotation_scale_from_the_variable_dictionary() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let scale_handle = doc.allocate_handle();
    let mut scale = Scale::new("1:100", 1.0, 100.0);
    scale.handle = scale_handle;
    doc.objects.insert(scale_handle, ObjectType::Scale(scale));
    let variable_handle = doc.allocate_handle();
    let mut variable = DictionaryVariable::new("CANNOSCALE", "1:100");
    variable.handle = variable_handle;
    doc.objects.insert(variable_handle, ObjectType::DictionaryVariable(variable));
    let dictionary_handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary.add_entry("CANNOSCALE", variable_handle);
    doc.objects.insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    let back = dxf(&doc);
    assert_eq!(back.header.current_annotation_scale, "1:100");
    assert!((back.header.annotation_scale_value - 0.01).abs() < 1e-12);
}

// ---------------------------------------------------------------------------------------------------------------
// ACAD_TABLE
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn an_r2010_cell_holding_a_block_has_the_block_cell_type_in_a_dwg_read() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let block = doc.allocate_handle();
    let mut table = Table::new(Vector3::ZERO, 1, 2);
    table.rows[0].cells[0] = TableCell::text("text");
    let mut cell = TableCell::new();
    cell.contents.push(CellContent::block(block));
    table.rows[0].cells[1] = cell;
    let handle = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let read = dwg(&doc);
    let EntityType::Table(table) = read.get_entity(handle).expect("table") else { panic!() };
    assert_eq!(table.rows[0].cells[0].cell_type, CellType::Text);
    assert_eq!(table.rows[0].cells[1].cell_type, CellType::Block);
    assert_eq!(table.rows[0].cells[1].contents[0].content_type, TableCellContentType::Block);
}

fn legacy_table_with_override_word(word: i32) -> Table {
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    let mut cell = TableCell::text("a");
    let mut style = CellStyle::new();
    style.legacy_override_bits = true;
    style.override_flags = word;
    style.text_height = 4.0;
    cell.style = Some(style);
    table.rows[0].cells[0] = cell;
    table
}

#[test]
fn the_override_word_of_a_legacy_cell_is_group_91_from_r2007_and_a_16_bit_group_177_before() {
    // R2007 and later: the whole word, bit 18 (no group of its own) included, on the style.
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    doc.add_entity(EntityType::Table(Box::new(legacy_table_with_override_word(0x40020)))).unwrap();
    let text = dxf_text(&doc);
    assert!(text.contains("91\r\n262176\r\n"));
    let back = dxf(&doc);
    let style = first_table(&back).rows[0].cells[0].style.as_ref().expect("style");
    assert_eq!(style.override_flags, 0x40020);
    assert!(style.legacy_override_bits);

    // A word with bits above 15 only still has a style; before R2007 the group holds 16 bits.
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    doc.add_entity(EntityType::Table(Box::new(legacy_table_with_override_word(0x40000)))).unwrap();
    let back = dxf(&doc);
    let style = first_table(&back).rows[0].cells[0].style.as_ref().expect("a style that states bit 18 only");
    assert_eq!(style.override_flags, 0x40000);

    let mut doc = CadDocument::with_version(DxfVersion::AC1018);
    doc.add_entity(EntityType::Table(Box::new(legacy_table_with_override_word(0x40020)))).unwrap();
    assert!(dxf_text(&doc).contains("177\r\n    32\r\n"));
    let back = dxf(&doc);
    assert_eq!(first_table(&back).rows[0].cells[0].style.as_ref().expect("style").override_flags, 0x20);

    // The 16-bit group is read as unsigned: bit 15 is not a sign.
    let mut doc = CadDocument::with_version(DxfVersion::AC1018);
    doc.add_entity(EntityType::Table(Box::new(legacy_table_with_override_word(0x8020)))).unwrap();
    let back = dxf(&doc);
    assert_eq!(first_table(&back).rows[0].cells[0].style.as_ref().expect("style").override_flags, 0x8020);
}

#[test]
fn a_legacy_block_cell_keeps_its_own_value_as_a_second_item_and_the_block_pointer_in_the_cell() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    let block = doc.allocate_handle();
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    let mut cell = TableCell::new();
    cell.cell_type = CellType::Block;
    cell.contents.push(CellContent::block(block));
    let mut value = CellContent::text("");
    value.content_type = TableCellContentType::Value;
    cell.contents.push(value);
    table.rows[0].cells[0] = cell;
    doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let back = dxf(&doc);
    let cell = &first_table(&back).rows[0].cells[0];
    assert_eq!(cell.cell_type, CellType::Block);
    assert_eq!(cell.contents.len(), 2, "the pointer and the value");
    assert_eq!(cell.contents[0].content_type, TableCellContentType::Block);
    assert_eq!(cell.contents[1].content_type, TableCellContentType::Value);
    assert_eq!(cell.value_handle, Some(block), "a pre-R2010 cell keeps the pointer in the cell too");
}

#[test]
fn a_typed_cell_value_does_not_keep_group_91_as_a_second_number() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    let mut cell = TableCell::new();
    let mut content = CellContent::new();
    content.content_type = TableCellContentType::Value;
    content.value = acadrust::entities::CellValue::integer(5);
    cell.contents.push(content);
    table.rows[0].cells[0] = cell;
    doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let back = dxf(&doc);
    let value = &first_table(&back).rows[0].cells[0].contents[0].value;
    assert_eq!(value.numeric_value, 5.0);
    assert_eq!(value.dxf_long, None);
}

/// A table of the R2010+ kind with its TABLECONTENT and TABLEGEOMETRY objects and the round-trip record that links
/// them, as an R2010+ DWG holds them.
fn document_with_table_carriers() -> (CadDocument, Handle, Table) {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::new(1.0, 2.0, 0.0), 2, 2);
    for (row, text) in [["a", "b"], ["c", "d"]].iter().enumerate() {
        for (column, text) in text.iter().enumerate() {
            table.rows[row].cells[column] = TableCell::text(text);
            let mut style = CellStyle::new();
            style.alignment = 1 + (row * 2 + column) as i32;
            style.value_data_type = 512;
            table.rows[row].cells[column].style = Some(style);
        }
    }
    table.rows[1].cells[1].tooltip = "tip".into();
    // The object states what is not geometry; the geometry object states the geometry.
    let content = table.clone();
    for row in &mut table.rows {
        for cell in &mut row.cells {
            cell.geometry_data_flag = 3;
            cell.geometry_width_with_gap = 12.5;
            cell.geometry_height_with_gap = 4.5;
            let geometry = acadrust::entities::CellContentGeometry {
                distance_to_top_left: Vector3::new(1.0, -1.0, 0.0),
                distance_to_center: Vector3::new(6.0, -2.0, 0.0),
                width: 11.0,
                height: 3.5,
                outer_width: 12.0,
                outer_height: 4.0,
                flags: 0,
            };
            cell.geometry_flags = 1;
            cell.geometries = vec![geometry.clone()];
            cell.geometry = Some(geometry.clone());
            cell.contents[0].geometry = Some(geometry);
        }
    }
    table.break_ranges = vec![TableBreakRange { position: Vector3::ZERO, start_row: 0, end_row: 1 }];
    let expected = table.clone();

    let table_handle = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let content_handle = doc.allocate_handle();
    let mut content = content;
    content.common.handle = content_handle;
    doc.objects.insert(content_handle, ObjectType::TableContent(content));
    let geometry_handle = doc.allocate_handle();
    let cells = expected
        .rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .map(|cell| TableGeometryCell {
            geometry_data_flag: cell.geometry_data_flag,
            width_with_gap: cell.geometry_width_with_gap,
            height_with_gap: cell.geometry_height_with_gap,
            table_geometry: Handle::NULL,
            geometry: cell.geometries.clone(),
        })
        .collect();
    let mut geometry = DataObject::new(DataObjectData::TableGeometry(TableGeometry { rows: 2, columns: 2, cells }));
    geometry.handle = geometry_handle;
    doc.objects.insert(geometry_handle, ObjectType::DataObject(geometry));
    let record = doc.ensure_xrecord(table_handle, "ACAD_XREC_ROUNDTRIP");
    if let Some(ObjectType::XRecord(record)) = doc.objects.get_mut(&record) {
        let entry = |code: i32, value: XRecordValue| XRecordEntry { code, value };
        record.entries = vec![
            entry(102, XRecordValue::String("ACAD_ROUNDTRIP_2008_TABLE_ENTITY".into())),
            entry(360, XRecordValue::Handle(content_handle)),
            entry(70, XRecordValue::Int16(2)),
            entry(90, XRecordValue::Int32(1)),
            entry(10, XRecordValue::Point3D(0.0, 0.0, 0.0)),
            entry(90, XRecordValue::Int32(0)),
            entry(90, XRecordValue::Int32(1)),
            entry(361, XRecordValue::Handle(geometry_handle)),
        ];
    }
    (doc, table_handle, expected)
}

#[test]
fn a_dxf_table_is_completed_from_its_content_and_geometry_objects_as_a_dwg_record_holds_it() {
    let (doc, handle, expected) = document_with_table_carriers();
    for back in [dxf(&doc), dxf_binary(&doc)] {
        let EntityType::Table(read) = back.get_entity(handle).expect("table") else { panic!() };
        for (row, expected_row) in read.rows.iter().zip(&expected.rows) {
            for (cell, expected_cell) in row.cells.iter().zip(&expected_row.cells) {
                assert_eq!(cell.style, expected_cell.style, "the cell style comes from the content object");
                assert_eq!(cell.tooltip, expected_cell.tooltip);
                assert_eq!(cell.geometry_data_flag, expected_cell.geometry_data_flag);
                assert_eq!(cell.geometry_width_with_gap, expected_cell.geometry_width_with_gap);
                assert_eq!(cell.geometry_height_with_gap, expected_cell.geometry_height_with_gap);
                assert_eq!(cell.geometries, expected_cell.geometries);
                assert_eq!(cell.geometry, expected_cell.geometry);
                assert_eq!(cell.geometry_flags, expected_cell.geometry_flags);
                assert_eq!(cell.contents[0].geometry, expected_cell.contents[0].geometry);
            }
        }
        assert_eq!(read.break_ranges, expected.break_ranges, "the break range comes from the round-trip record");
        // The objects stay, as in a DWG.
        assert!(back.objects.values().any(|object| matches!(object, ObjectType::TableContent(_))));
    }
}

#[test]
fn a_dxf_table_whose_grid_differs_from_its_content_object_is_left_as_read() {
    let (mut doc, handle, _) = document_with_table_carriers();
    for object in doc.objects.values_mut() {
        if let ObjectType::TableContent(content) = object {
            content.rows.pop();
        }
    }
    let back = dxf(&doc);
    let EntityType::Table(read) = back.get_entity(handle).expect("table") else { panic!() };
    assert_eq!(read.rows.len(), 2);
    assert_eq!(read.rows[0].cells[0].geometry_data_flag, 0, "nothing is taken from a content object of another grid");
}

#[test]
fn a_cell_with_several_items_in_the_content_object_keeps_the_legacy_item() {
    let (mut doc, handle, _) = document_with_table_carriers();
    for object in doc.objects.values_mut() {
        if let ObjectType::TableContent(content) = object {
            let item = content.rows[0].cells[0].contents[0].clone();
            content.rows[0].cells[0].contents.push(item);
        }
    }
    let back = dxf(&doc);
    let EntityType::Table(read) = back.get_entity(handle).expect("table") else { panic!() };
    let cell = &read.rows[0].cells[0];
    assert_eq!(cell.contents.len(), 1, "the entity states one effective item");
    // The geometry object lists every item, so the entity's cached geometry of such a cell is not taken from it.
    assert_eq!(cell.geometry_data_flag, 0);
    // The content format of the effective item is the cell's own.
    assert_eq!(cell.contents[0].alignment, cell.style.as_ref().expect("style").alignment);
    assert!(cell.contents[0].format_has_data);
}
