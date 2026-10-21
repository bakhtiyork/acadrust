// SPDX-License-Identifier: MPL-2.0
//! DXF writer fixes of job R5a-2 (gate G4: the fork's own round trip and the ACadSharp read of its output).
//! Each test writes a synthetic document (or one marked as read from a file) and checks the groups written or the
//! value read back; none uses an external file.
use crate::io::dxf::{DxfReader, DxfWriter};
use crate::objects::{
    DynamicBlockData, DynamicBlockObject, Material, MaterialDxfSource, ObjectType, Scale, SortEntitiesTable,
};
use crate::entities::{
    Dimension, DimensionLinear, EntityType, GradientColorEntry, Hatch, MultiLeader, Point, Polyline2D, Vertex2D, Viewport,
};
use crate::tables::*;
use crate::types::{Color, Vector3};
use crate::types::{DxfVersion, Handle};
use crate::xdata::{ExtendedData, ExtendedDataRecord};
use crate::CadDocument;
use std::io::Cursor;

fn write(doc: &CadDocument) -> String {
    String::from_utf8_lossy(&DxfWriter::new(doc).write_to_vec().expect("write")).into_owned()
}

fn read(text: &str) -> CadDocument {
    DxfReader::from_reader(Cursor::new(text.as_bytes().to_vec()))
        .expect("reader")
        .read()
        .expect("read")
}

/// The records `0 KIND ... ` of a text DXF, each up to the next group code 0.
fn records<'a>(text: &'a str, kind: &str) -> Vec<&'a str> {
    let marker = format!("\r\n  0\r\n{kind}\r\n");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find(&marker) {
        let start = from + start + 2;
        let body = start + marker.len() - 2;
        let end = text[body..].find("\r\n  0\r\n").map_or(text.len(), |end| body + end + 2);
        found.push(&text[start..end]);
        from = end - 2;
    }
    found
}

fn values(record: &str, code: i32) -> Vec<String> {
    let lines: Vec<&str> = record.trim_start_matches("\r\n").split("\r\n").collect();
    let wanted = format!("{code}");
    lines
        .chunks(2)
        .filter(|pair| pair.len() == 2 && pair[0].trim() == wanted)
        .map(|pair| pair[1].trim().to_string())
        .collect()
}

fn first(record: &str, code: i32) -> Option<String> {
    values(record, code).into_iter().next()
}

/// A minimal DXF: the header with the given extra lines, an empty CLASSES section with the given records and nothing else.
fn minimal_dxf(version: &str, header_extra: &str, classes: &str) -> String {
    format!(
        "  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\n{version}\r\n{header_extra}  0\r\nENDSEC\r\n\
         0\r\nSECTION\r\n  2\r\nCLASSES\r\n{classes}  0\r\nENDSEC\r\n  0\r\nEOF\r\n"
    )
}

fn class_record(dxf: &str, cpp: &str, app: &str, flags: i32, count: i32, entity: bool) -> String {
    format!(
        "  0\r\nCLASS\r\n  1\r\n{dxf}\r\n  2\r\n{cpp}\r\n  3\r\n{app}\r\n 90\r\n{flags}\r\n 91\r\n{count}\r\n280\r\n     0\r\n281\r\n     {}\r\n",
        i32::from(entity)
    )
}

fn class_names(text: &str) -> Vec<String> {
    records(text, "CLASS").iter().filter_map(|record| first(record, 1)).collect()
}

#[test]
fn classes_of_a_dxf_source_are_written_as_read_and_the_default_registry_is_not() {
    let classes = class_record("ACDBPLACEHOLDER", "AcDbPlaceHolder", "ObjectDBX Classes", 1153, 7, false)
        + &class_record("MYTHING", "AcDbMyThing", "MyApp", 0, 2, true);
    let doc = read(&minimal_dxf("AC1018", "", &classes));
    let text = write(&doc);
    assert_eq!(class_names(&text), vec!["ACDBPLACEHOLDER", "MYTHING"]);
    let placeholder = records(&text, "CLASS")[0];
    assert_eq!(first(placeholder, 3).as_deref(), Some("ObjectDBX Classes"));
    assert_eq!(first(placeholder, 90).as_deref(), Some("1153"), "proxy flags of the source");
    assert_eq!(first(placeholder, 91).as_deref(), Some("7"), "instance count of the source");
    // A document built in memory still writes its class registry.
    assert!(class_names(&write(&CadDocument::new())).len() > 50);
}

#[test]
fn a_class_the_source_lacked_is_added_for_a_record_added_after_reading() {
    let classes = class_record("ACDBPLACEHOLDER", "AcDbPlaceHolder", "ObjectDBX Classes", 0, 0, false);
    let mut doc = read(&minimal_dxf("AC1018", "  9\r\n$HANDSEED\r\n  5\r\n500\r\n", &classes));
    assert!(!class_names(&write(&doc)).contains(&"SCALE".to_string()));

    // An object created after the read has a handle at or above the source's handle seed and needs its class.
    let handle = doc.allocate_handle();
    let mut scale = Scale::new("1:1", 1.0, 1.0);
    scale.handle = handle;
    doc.objects.insert(handle, ObjectType::Scale(scale));
    assert!(class_names(&write(&doc)).contains(&"SCALE".to_string()));
}

#[test]
fn material_groups_the_source_left_out_stay_out_and_changed_ones_are_written() {
    let mut material = Material::new();
    material.handle = Handle::new(0x50);
    material.name = "Beech".to_string();
    material.diffuse_color.flag = 1;
    material.diffuse_color.rgb = Some(0x112233);
    let mut source = MaterialDxfSource::default();
    for code in [1, 71, 91] {
        source.record(code, false);
    }
    material.dxf_source = Some(Box::new(source));
    let mut doc = CadDocument::new();
    doc.objects.insert(material.handle, ObjectType::Material(material.clone()));
    let text = write(&doc);
    let record = records(&text, "MATERIAL")[0];
    assert_eq!(first(record, 71).as_deref(), Some("1"));
    assert_eq!(first(record, 91).as_deref(), Some("1122867"));
    for absent in [72, 73, 43, 94, 282, 90, 149] {
        assert!(first(record, absent).is_none(), "group {absent} was absent from the source and is the default");
    }

    // A value changed from the default is written even though the source lacked the group.
    material.channel_flags = 5;
    doc.objects.insert(material.handle, ObjectType::Material(material));
    let text = write(&doc);
    assert_eq!(first(records(&text, "MATERIAL")[0], 94).as_deref(), Some("5"));
}

#[test]
fn builtin_materials_without_a_dxf_source_are_minimal_but_a_changed_value_is_kept() {
    let mut doc = CadDocument::new();
    let mut by_block = Material::new();
    by_block.handle = Handle::new(0x51);
    by_block.name = "ByBlock".to_string();
    doc.objects.insert(by_block.handle, ObjectType::Material(by_block.clone()));
    let text = write(&doc);
    let record = records(&text, "MATERIAL")[0];
    assert_eq!(first(record, 72).as_deref(), Some("0"));
    assert_eq!(first(record, 94).as_deref(), Some("127"));
    assert!(first(record, 73).is_none() && first(record, 43).is_none());

    by_block.diffuse_map.transform[0] = 0.0208;
    doc.objects.insert(by_block.handle, ObjectType::Material(by_block));
    let text = write(&doc);
    let record = records(&text, "MATERIAL")[0];
    assert_eq!(values(record, 43).len(), 16, "a changed matrix is written whole");
    assert_eq!(first(record, 43).as_deref(), Some("0.0208"));
}

#[test]
fn optional_header_variables_follow_the_source_the_dwg_version_and_edits() {
    let doc = CadDocument::new();
    assert!(!write(&doc).contains("$ISOLINES"), "an unchanged variable of a built document stays out");

    let mut edited = CadDocument::new();
    edited.header.isolines = 8;
    edited.header.lens_length = 35.0;
    let text = write(&edited);
    assert!(text.contains("  9\r\n$ISOLINES\r\n 70\r\n     8\r\n"));
    assert!(text.contains("  9\r\n$LENSLENGTH\r\n 40\r\n35.0\r\n"));

    // A DXF source that stated a variable at its default keeps it, an empty text included.
    let mut from_dxf = CadDocument::new();
    from_dxf.header.dxf_source_variables = Some(["$ACADVER".to_string(), "$PROJECTNAME".to_string(), "$HYPERLINKBASE".to_string()].into());
    let text = write(&from_dxf);
    assert!(text.contains("  9\r\n$PROJECTNAME\r\n  1\r\n\r\n"));
    assert!(text.contains("  9\r\n$HYPERLINKBASE\r\n  1\r\n\r\n"));
    assert!(!text.contains("$LENSLENGTH"));

    // A DWG source states the variables its version stores.
    let mut from_dwg = CadDocument::new();
    from_dwg.dwg_source_version = Some(DxfVersion::AC1021);
    let text = write(&from_dwg);
    assert!(text.contains("  9\r\n$LENSLENGTH\r\n"), "R2007 stores it");
    assert!(text.contains("  9\r\n$PROJECTNAME\r\n"), "R2004 and later store it");
    from_dwg.dwg_source_version = Some(DxfVersion::AC1015);
    let text = write(&from_dwg);
    assert!(!text.contains("$LENSLENGTH"), "R2000 has no such variable");
}

#[test]
fn arrow_block_names_of_a_dwg_header_come_from_the_block_record_handle() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1018);
    let mut block = BlockRecord::new("_BoxBlank");
    block.handle = Handle::new(0x70);
    doc.block_records.add(block).unwrap();
    doc.header.dim_arrow_block_handle = Handle::new(0x70);
    let text = write(&doc);
    assert!(text.contains("  9\r\n$DIMBLK\r\n  1\r\n_BoxBlank\r\n"));
    assert!(!text.contains("$DIMBLK1"), "an arrow block that is not named stays out");
}

#[test]
fn dynamic_block_purge_preventer_is_written_with_its_version_only() {
    let mut object = DynamicBlockObject::new("ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION", "AcDbDynamicBlockPurgePreventer");
    object.handle = Handle::new(0x268);
    object.owner = Handle::new(0x25A);
    object.data = DynamicBlockData::Representation(crate::objects::BlockRepresentationData {
        flags: 1,
        block: Handle::NULL,
    });
    let mut doc = CadDocument::new();
    doc.objects.insert(object.handle, ObjectType::DynamicBlock(object));
    let text = write(&doc);
    let record = records(&text, "ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION")[0];
    assert_eq!(first(record, 100).as_deref(), Some("AcDbDynamicBlockPurgePreventer"));
    assert_eq!(first(record, 70).as_deref(), Some("1"));
    assert!(first(record, 340).is_none(), "AutoCAD's own record has no block pointer");
}

fn app_names(text: &str) -> Vec<String> {
    records(text, "APPID").iter().filter_map(|record| first(record, 2)).collect()
}

#[test]
fn default_application_ids_the_source_never_registered_are_not_written() {
    let mut doc = CadDocument::new();
    assert!(app_names(&write(&doc)).contains(&"AcCmTransparency".to_string()), "a built document writes them");

    doc.source_app_id_order = Some(vec!["ACAD".to_string()]);
    let names = app_names(&write(&doc));
    assert!(names.contains(&"ACAD".to_string()));
    for unused in ["AcCmTransparency", "AcAecLayerStandard", "AcadAnnotative"] {
        assert!(!names.contains(&unused.to_string()), "{unused} is a native default the source lacked");
    }

    // An application the data does use is still registered.
    let mut layer = Layer::new("L1");
    layer.description = "text".to_string();
    doc.layers.add(layer).unwrap();
    let names = app_names(&write(&doc));
    assert!(names.contains(&"AcAecLayerStandard".to_string()));
    assert!(!names.contains(&"AcCmTransparency".to_string()));
}

#[test]
fn handle_seed_of_a_read_document_stays_the_seed_the_source_stated() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x5000);
    assert!(write(&doc).contains("  9\r\n$HANDSEED\r\n  5\r\n5000\r\n"));
    // Twice the written file read and written again keeps the seed (no growth by the defaults the reader creates).
    let reread = read(&write(&doc));
    assert_eq!(reread.header.source_handle_seed, Some(0x5000));
    assert!(write(&reread).contains("  9\r\n$HANDSEED\r\n  5\r\n5000\r\n"));
}

#[test]
fn sort_entities_table_keeps_the_sort_handles_it_has() {
    let mut doc = CadDocument::new();
    let mut table = SortEntitiesTable::new();
    table.add_entry(Handle::new(0x100), Handle::new(0x300));
    table.add_entry(Handle::new(0x101), Handle::new(0x200));
    table.handle = Handle::new(0x77);
    doc.objects.insert(table.handle, ObjectType::SortEntitiesTable(table));
    let text = write(&doc);
    let record = records(&text, "SORTENTSTABLE")[0];
    assert_eq!(values(record, 331), vec!["100", "101"]);
    assert_eq!(&values(record, 5)[1..], &["300", "200"], "the entries keep their keys, in entry order");
}

#[test]
fn extended_data_of_unused_defaults_is_not_a_reason_to_register_them() {
    // Only a record that names the application keeps it registered.
    let mut doc = CadDocument::new();
    doc.source_app_id_order = Some(vec!["ACAD".to_string()]);
    let mut layer = Layer::new("L1");
    layer.handle = Handle::new(0x90);
    doc.layers.add(layer).unwrap();
    let mut data = ExtendedData::new();
    data.add_record(ExtendedDataRecord::new("AcCmTransparency"));
    doc.object_xdata.insert(Handle::new(0x90), data);
    assert!(app_names(&write(&doc)).contains(&"AcCmTransparency".to_string()));
}


#[test]
fn multileader_line_type_by_block_is_kept() {
    let mut doc = CadDocument::new();
    let by_block = doc.line_types.get("ByBlock").expect("ByBlock linetype").handle;
    let mut leader = MultiLeader::new();
    leader.line_type_handle = Some(by_block);
    doc.add_entity(EntityType::MultiLeader(Box::new(leader))).unwrap();
    let text = write(&doc);
    let record = records(&text, "MULTILEADER")[0];
    assert_eq!(first(record, 341), Some(format!("{:X}", by_block.value())));
}

#[test]
fn entity_material_handle_is_written_from_r2007_and_only_when_the_entity_has_one() {
    let mut doc = CadDocument::new();
    doc.version = DxfVersion::AC1024;
    let mut material = Material::new();
    material.handle = Handle::new(0x96);
    material.name = "Beech".to_string();
    doc.objects.insert(material.handle, ObjectType::Material(material));
    let mut with = Point::new();
    with.common.material_flags = 0b11;
    with.common.material_handle = Some(Handle::new(0x96));
    doc.add_entity(EntityType::Point(with)).unwrap();
    doc.add_entity(EntityType::Point(Point::new())).unwrap();
    let text = write(&doc);
    let points = records(&text, "POINT");
    assert_eq!(first(points[0], 347).as_deref(), Some("96"));
    assert!(first(points[1], 347).is_none());

    // R2004 has no such group.
    doc.version = DxfVersion::AC1018;
    let text = write(&doc);
    assert!(first(records(&text, "POINT")[0], 347).is_none());
}

#[test]
fn viewport_groups_beyond_the_geometry_follow_the_source_the_dwg_version_and_edits() {
    let mut doc = CadDocument::new();
    doc.version = DxfVersion::AC1024;
    let mut vp = Viewport::new();
    vp.style_sheet = "Standard".to_string();
    vp.ambient_color = Color::from_rgb(51, 51, 51);
    vp.brightness = 0.25;
    doc.add_entity(EntityType::Viewport(Box::new(vp))).unwrap();
    let plain = Viewport::new();
    doc.add_entity(EntityType::Viewport(Box::new(plain))).unwrap();
    let text = write(&doc);
    let viewports = records(&text, "VIEWPORT");
    assert_eq!(first(viewports[0], 1).as_deref(), Some("Standard"));
    assert_eq!(first(viewports[0], 421).as_deref(), Some("3355443"));
    assert_eq!(first(viewports[0], 141).as_deref(), Some("0.25"));
    for absent in [1, 63, 141, 282, 292] {
        assert!(first(viewports[1], absent).is_none(), "group {absent} of an unedited viewport without a source");
    }

    // A DWG record of R2007 or later states its lighting even at the native defaults.
    doc.dwg_source_version = Some(DxfVersion::AC1024);
    let text = write(&doc);
    let viewports = records(&text, "VIEWPORT");
    assert_eq!(first(viewports[1], 292).as_deref(), Some("1"));
    assert_eq!(first(viewports[1], 282).as_deref(), Some("1"));
    assert_eq!(first(viewports[1], 1).as_deref(), Some(""));
}

#[test]
fn block_action_connections_are_written_as_code_and_name_pairs() {
    let mut object = DynamicBlockObject::new("BLOCKMOVEACTION", "AcDbBlockMoveAction");
    object.handle = Handle::new(0x267);
    object.data = DynamicBlockData::MoveAction(crate::objects::BlockMoveAction {
        connections: [
            crate::objects::BlockConnection { code: 1, name: "XDelta".to_string() },
            crate::objects::BlockConnection { code: 1, name: "YDelta".to_string() },
        ],
        ..Default::default()
    });
    let mut doc = CadDocument::new();
    doc.objects.insert(object.handle, ObjectType::DynamicBlock(object));
    let text = write(&doc);
    let record = records(&text, "BLOCKMOVEACTION")[0];
    let order: Vec<&str> = record
        .trim_start_matches("\r\n")
        .split("\r\n")
        .collect::<Vec<_>>()
        .chunks(2)
        .filter(|pair| matches!(pair[0].trim(), "92" | "93" | "301" | "302"))
        .map(|pair| pair[0].trim())
        .collect();
    assert_eq!(order, vec!["92", "301", "93", "302"], "the reference reader names the connection it has just read");
}

#[test]
fn dimension_block_bit_and_line_spacing_follow_the_source_or_the_dwg_record() {
    let mut doc = CadDocument::new();
    doc.version = DxfVersion::AC1021;
    let mut dwg_record = DimensionLinear::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    dwg_record.base.block_name = "*D1".to_string();
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dwg_record)))).unwrap();
    let mut no_block = DimensionLinear::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    no_block.base.block_name = String::new();
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(no_block)))).unwrap();
    let text = write(&doc);
    let dims = records(&text, "DIMENSION");
    assert_eq!(first(dims[0], 70).as_deref(), Some("32"), "a record with a geometry block carries bit 32");
    assert_eq!(first(dims[1], 70).as_deref(), Some("0"));
    assert!(first(dims[0], 41).is_none(), "a spacing factor of 1 is not stated without a DWG source");

    doc.dwg_source_version = Some(DxfVersion::AC1021);
    let text = write(&doc);
    assert_eq!(first(records(&text, "DIMENSION")[0], 41).as_deref(), Some("1.0"));
}

#[test]
fn disabled_hatch_gradient_definition_is_kept_in_r2004_output() {
    let mut doc = CadDocument::new();
    doc.version = DxfVersion::AC1018;
    let mut hatch = Hatch::new();
    hatch.gradient_color.enabled = false;
    hatch.gradient_color.name = "LINEAR".to_string();
    hatch.gradient_color.colors.push(GradientColorEntry {
        value: 0.0,
        color: Color::Index(5),
        color_name: None,
        book_name: None,
    });
    doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
    let mut plain = Hatch::new();
    plain.gradient_color.enabled = false;
    doc.add_entity(EntityType::Hatch(Box::new(plain))).unwrap();
    let text = write(&doc);
    let hatches = records(&text, "HATCH");
    assert_eq!(first(hatches[0], 450).as_deref(), Some("0"));
    assert_eq!(first(hatches[0], 470).as_deref(), Some("LINEAR"));
    assert!(first(hatches[1], 450).is_none(), "a hatch with no gradient data writes none");

    doc.version = DxfVersion::AC1015;
    assert!(first(records(&write(&doc), "HATCH")[0], 450).is_none(), "the groups belong to R2004 and later");
}

#[test]
fn polyline_default_widths_the_source_stated_are_written_after_the_vertices_took_over() {
    let mut doc = CadDocument::new();
    let mut polyline = Polyline2D::new();
    polyline.source_widths = Some((0.2, 0.1));
    let mut vertex = Vertex2D::new(Vector3::ZERO);
    vertex.start_width = 0.2;
    vertex.end_width = 0.1;
    polyline.vertices.push(vertex);
    doc.add_entity(EntityType::Polyline2D(polyline)).unwrap();
    doc.version = DxfVersion::AC1012;
    let text = write(&doc);
    let record = records(&text, "POLYLINE")[0];
    assert_eq!(first(record, 40).as_deref(), Some("0.2"));
    assert_eq!(first(record, 41).as_deref(), Some("0.1"));
}

#[test]
fn table_cell_style_of_a_binary_file_writes_only_what_it_overrides() {
    use crate::entities::table::{CellStyle, CellStylePropertyFlags, TableCell};
    use crate::entities::Table;
    let build = |flags: CellStylePropertyFlags| {
        let mut doc = CadDocument::with_version(DxfVersion::AC1027);
        let mut table = Table::new(Vector3::ZERO, 1, 1);
        let mut cell = TableCell::text("c");
        let mut style = CellStyle::new();
        style.property_flags = flags;
        style.fill_enabled = true;
        style.background_color = Color::from_index(3);
        style.alignment = 5;
        cell.style = Some(style);
        // The binary reader marks the cells it fills with the style type it read.
        cell.source_style_type = Some(1);
        table.rows[0].cells[0] = cell;
        doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
        write(&doc)
    };
    let plain = build(CellStylePropertyFlags::NONE);
    let record = records(&plain, "ACAD_TABLE")[0];
    for absent in [63, 283, 170, 64, 140] {
        assert!(first(record, absent).is_none(), "group {absent} of a cell that overrides nothing");
    }
    let overridden = build(CellStylePropertyFlags::BACKGROUND_COLOR | CellStylePropertyFlags::ALIGNMENT);
    let record = records(&overridden, "ACAD_TABLE")[0];
    assert_eq!(first(record, 283).as_deref(), Some("0"));
    assert_eq!(first(record, 63).as_deref(), Some("3"));
    assert_eq!(first(record, 170).as_deref(), Some("5"));
    assert!(first(record, 64).is_none());
}

#[test]
fn extended_data_strings_lose_their_trailing_nul() {
    let mut point = Point::new();
    let mut record = ExtendedDataRecord::new("ACAD");
    record.values.push(crate::xdata::XDataValue::String("name\0".to_string()));
    point.common.extended_data.add_record(record);
    let mut doc = CadDocument::new();
    doc.add_entity(EntityType::Point(point)).unwrap();
    let text = write(&doc);
    let record = records(&text, "POINT")[0];
    assert_eq!(values(record, 1000), vec!["name".to_string()]);
}

#[test]
fn leader_box_size_is_not_stated_for_an_r2010_dwg_record() {
    use crate::entities::Leader;
    let mut doc = CadDocument::with_version(DxfVersion::AC1027);
    doc.add_entity(EntityType::Leader(Leader::new())).unwrap();
    let text = write(&doc);
    assert!(first(records(&text, "LEADER")[0], 40).is_some(), "a native leader states its box size");
    doc.dwg_source_version = Some(DxfVersion::AC1027);
    let text = write(&doc);
    let record = records(&text, "LEADER")[0];
    assert!(first(record, 40).is_none() && first(record, 41).is_none());
}

#[test]
fn layout_plot_view_name_is_stated_for_a_dwg_up_to_r2000() {
    use crate::objects::Layout;
    let layout_text = |source: Option<DxfVersion>| {
        let mut doc = CadDocument::new();
        let mut layout = Layout::new("Layout9");
        layout.handle = Handle::new(0x8F1);
        doc.objects.insert(layout.handle, ObjectType::Layout(layout));
        doc.dwg_source_version = source;
        let text = write(&doc);
        let record = records(&text, "LAYOUT")
            .into_iter()
            .find(|r| values(r, 1).iter().any(|name| name == "Layout9"))
            .map(str::to_string);
        record.expect("layout record")
    };
    assert_eq!(values(&layout_text(Some(DxfVersion::AC1015)), 6), vec![String::new()]);
    assert!(values(&layout_text(Some(DxfVersion::AC1027)), 6).is_empty());
    assert!(values(&layout_text(None), 6).is_empty());
}

#[test]
fn layer_description_record_of_the_source_is_kept_while_the_description_is_unchanged() {
    let build = |description: &str| {
        let mut doc = CadDocument::new();
        let mut layer = Layer::new("L1");
        layer.handle = Handle::new(0x90);
        layer.description = description.to_string();
        doc.layers.add(layer).unwrap();
        let mut data = ExtendedData::new();
        let mut record = ExtendedDataRecord::new("AcAecLayerStandard");
        record.values.push(crate::xdata::XDataValue::String(String::new()));
        data.add_record(record);
        doc.object_xdata.insert(Handle::new(0x90), data);
        let text = write(&doc);
        records(&text, "LAYER")
            .into_iter()
            .find(|r| values(r, 2).first().map(String::as_str) == Some("L1"))
            .map(str::to_string)
            .expect("layer record")
    };
    // The source record (one string, no description) stands for the empty description.
    assert_eq!(values(&build(""), 1000).len(), 1);
    // A changed description regenerates the two-string record.
    assert_eq!(values(&build("walls"), 1000), vec![String::new(), "walls".to_string()]);
}

#[test]
fn dimension_style_of_an_r14_dwg_has_by_block_line_weights() {
    use crate::io::dwg::{DwgReader, DwgWriter};
    // R13/R14 records hold no DIMLWD/DIMLWE: the reader supplies the ByBlock default, not a 0.00 mm weight.
    let mut doc = CadDocument::with_version(DxfVersion::AC1014);
    let mut style = DimStyle::new("Plain");
    style.handle = Handle::new(0x9A1);
    doc.dim_styles.add(style).unwrap();
    let bytes = DwgWriter::write_to_vec(&doc).expect("write dwg");
    let loaded = DwgReader::from_stream(Cursor::new(bytes)).read().expect("read dwg");
    let style = loaded.dim_styles.get("Plain").expect("dimension style");
    assert_eq!((style.dimlwd, style.dimlwe), (-2, -2));
}

#[test]
fn underlay_definition_reactors_come_from_the_dwg_side_map() {
    use crate::entities::underlay::{UnderlayDefinition, UnderlayType};
    let mut def = UnderlayDefinition::new(UnderlayType::Pdf);
    def.handle = Handle::new(0x6A0);
    def.owner_handle = Handle::new(0x6A1);
    let mut doc = CadDocument::new();
    doc.objects.insert(def.handle, ObjectType::UnderlayDefinition(def.clone()));
    // The reactor is an entity of the document, as the underlay reference is.
    let reactor = doc.add_entity(EntityType::Point(Point::new())).unwrap();
    doc.reactors_by_handle.insert(def.handle, vec![reactor]);
    let text = write(&doc);
    let record = records(&text, "PDFDEFINITION")[0];
    assert_eq!(values(record, 330), vec![format!("{:X}", reactor.value()), "6A1".to_string()]);
}
