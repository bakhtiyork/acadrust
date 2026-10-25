// SPDX-License-Identifier: MPL-2.0
//! Fixes of job R5a-3 (gate G4: the native round trip). Each test writes a synthetic document (or one marked as read
//! from a DWG) and checks the groups written or the value read back; none uses an external file.
use crate::entities::{
    AttributeEntity, EntityType, Hatch, HatchPatternLine, MText, MultiLeader, Solid3D, Spline, Viewport,
};
use crate::io::dxf::{DxfReader, DxfWriter};
use crate::objects::{Material, ObjectType};
use crate::tables::*;
use crate::types::{DxfVersion, Handle, Vector3};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use crate::CadDocument;
use std::io::Cursor;

fn write(doc: &CadDocument) -> String {
    String::from_utf8_lossy(&DxfWriter::new(doc).write_to_vec().expect("write")).into_owned()
}

fn write_binary(doc: &CadDocument) -> Vec<u8> {
    DxfWriter::new_binary(doc).write_to_vec().expect("write binary")
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
        .map(|pair| pair[1].to_string())
        .collect()
}

fn first(record: &str, code: i32) -> Option<String> {
    values(record, code).into_iter().next().map(|v| v.trim().to_string())
}

fn builtin_material(name: &str) -> Material {
    let mut material = Material::new();
    material.handle = Handle::new(0x60);
    material.name = name.to_string();
    material
}

#[test]
fn builtin_material_channel_flags_are_written_only_where_the_source_version_states_them() {
    // R2007+ DWG: ByLayer carries channel flags, written as in the minimal form.
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    doc.dwg_source_version = Some(DxfVersion::AC1021);
    let material = builtin_material("ByLayer");
    doc.objects.insert(material.handle, ObjectType::Material(material.clone()));
    let text = write(&doc);
    assert_eq!(first(records(&text, "MATERIAL")[0], 94).as_deref(), Some("127"));

    // R2004 DWG: the record has no channel flags (nor translucence, reflectivity, illumination model, mode);
    // the native defaults are not written for it.
    let mut old = CadDocument::with_version(DxfVersion::AC1018);
    old.dwg_source_version = Some(DxfVersion::AC1018);
    old.objects.insert(material.handle, ObjectType::Material(material.clone()));
    let text = write(&old);
    let record = records(&text, "MATERIAL")[0];
    for absent in [94, 93, 148, 149, 468, 282] {
        assert!(first(record, absent).is_none(), "group {absent} is not in an R2004 DWG material");
    }

    // A value that was changed is not in the material of a DXF below R2007 (AutoCAD's own DXF of R2000 and R2004 states none: the
    // round-trip record `ACAD_XREC_ROUNDTRIP {MATERIAL ..}` of the material carries it, R5b-9), and is written for R2007 and later.
    let mut changed = material;
    changed.channel_flags = 5;
    old.objects.insert(changed.handle, ObjectType::Material(changed));
    let text = write(&old);
    assert!(first(records(&text, "MATERIAL")[0], 94).is_none());
    assert!(text.contains("MATERIAL\r\n"), "the round-trip record states the material's values");
    old.version = DxfVersion::AC1021;
    let text = write(&old);
    assert_eq!(first(records(&text, "MATERIAL")[0], 94).as_deref(), Some("5"));
    old.version = DxfVersion::AC1018;

    // A document built in memory keeps writing the BricsCAD form with its channel flags.
    let mut memory = CadDocument::new();
    memory.objects.insert(Handle::new(0x61), ObjectType::Material({
        let mut m = builtin_material("ByBlock");
        m.handle = Handle::new(0x61);
        m
    }));
    assert_eq!(first(records(&write(&memory), "MATERIAL")[0], 94).as_deref(), Some("127"));
}

#[test]
fn viewport_grid_major_is_not_written_for_a_dwg_that_has_no_such_field() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1018);
    doc.dwg_source_version = Some(DxfVersion::AC1015);
    let mut viewport = Viewport::new();
    viewport.grid_major = 5;
    doc.add_entity(EntityType::Viewport(Box::new(viewport.clone()))).unwrap();
    let text = write(&doc);
    assert!(first(records(&text, "VIEWPORT")[0], 61).is_none(), "an R2000 DWG viewport has no grid major");

    // A value that was changed is written; so is the default of an R2007+ DWG and of a document built in memory.
    viewport.grid_major = 8;
    let mut edited = CadDocument::with_version(DxfVersion::AC1018);
    edited.dwg_source_version = Some(DxfVersion::AC1015);
    edited.add_entity(EntityType::Viewport(Box::new(viewport.clone()))).unwrap();
    assert_eq!(first(records(&write(&edited), "VIEWPORT")[0], 61).as_deref(), Some("8"));
    viewport.grid_major = 5;
    let mut modern = CadDocument::with_version(DxfVersion::AC1021);
    modern.dwg_source_version = Some(DxfVersion::AC1021);
    modern.add_entity(EntityType::Viewport(Box::new(viewport.clone()))).unwrap();
    assert_eq!(first(records(&write(&modern), "VIEWPORT")[0], 61).as_deref(), Some("5"));
    let mut memory = CadDocument::with_version(DxfVersion::AC1018);
    memory.add_entity(EntityType::Viewport(Box::new(viewport))).unwrap();
    assert_eq!(first(records(&write(&memory), "VIEWPORT")[0], 61).as_deref(), Some("5"));
}

fn spline_with_tolerances(scenario: Option<i32>, knot: f64, control: f64) -> Spline {
    let mut spline = Spline::from_control_points(
        3,
        vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 2.0, 0.0),
            Vector3::new(2.0, 2.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ],
    );
    spline.dwg_scenario = scenario;
    spline.knot_tolerance = knot;
    spline.control_tolerance = control;
    spline
}

#[test]
fn spline_tolerances_a_dwg_fit_record_never_stored_are_not_invented() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1018);
    doc.add_entity(EntityType::Spline(spline_with_tolerances(Some(2), 0.0, 0.0))).unwrap();
    let text = write(&doc);
    let record = records(&text, "SPLINE")[0];
    assert!(first(record, 42).is_none() && first(record, 43).is_none(), "no groups 42/43: {record}");
    // Read back, the native model has what it had (the DXF reader applies no 1e-7 of its own).
    let back = read(&text);
    let EntityType::Spline(spline) = back.entities().find(|e| matches!(e, EntityType::Spline(_))).unwrap() else { unreachable!() };
    assert_eq!((spline.knot_tolerance, spline.control_tolerance), (0.0, 0.0));

    // Stored tolerances (any scenario) and a tolerance that was set are written as they are.
    let mut stored = CadDocument::with_version(DxfVersion::AC1018);
    stored.add_entity(EntityType::Spline(spline_with_tolerances(Some(1), 0.0, 0.0))).unwrap();
    stored.add_entity(EntityType::Spline(spline_with_tolerances(Some(2), 2e-7, 0.0))).unwrap();
    let text = write(&stored);
    let splines = records(&text, "SPLINE");
    assert_eq!(first(splines[0], 42).as_deref(), Some("0.0"));
    assert_eq!(first(splines[1], 42).unwrap().parse::<f64>().unwrap(), 2e-7);
}

#[test]
fn xdata_strings_of_a_dwg_lose_the_utf16_terminator_in_the_reader_not_the_writer() {
    // A wide EED string is counted with its NUL terminator (as AutoCAD stores it).
    let mut payload = vec![0u8];
    let units: Vec<u16> = "ACAD".encode_utf16().chain(std::iter::once(0)).collect();
    payload.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for unit in &units {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    let values = crate::io::dwg::eed_codec::decode_values(&payload, true, |_| None).unwrap();
    assert_eq!(values, vec![XDataValue::String("ACAD".to_string())], "the terminator is not part of the text");
    // ... and the encoder writes it back, so the bytes round-trip.
    let again = crate::io::dwg::eed_codec::encode_values_with_encoding(
        true,
        &values,
        encoding_rs::WINDOWS_1252.into(),
        30,
        |_| 0,
    );
    assert_eq!(again, payload);

    // The DXF writer writes the string as it is (it no longer trims anything).
    let mut doc = CadDocument::new();
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("ACAD");
    record.values.push(XDataValue::String("ACAD".to_string()));
    record.values.push(XDataValue::LayerName("0".to_string()));
    data.add_record(record);
    doc.object_xdata.insert(Handle::new(0x70), data);
    let mut block = BlockRecord::new("B");
    block.handle = Handle::new(0x70);
    doc.block_records.add(block).unwrap();
    let text = write(&doc);
    assert!(text.contains("1000\r\nACAD\r\n"));
    assert!(!text.contains('\0'));
}

fn multiline_attribute(version: DxfVersion, value: &str) -> CadDocument {
    let mut doc = CadDocument::with_version(version);
    let mut attrib = AttributeEntity::new("TAG".to_string(), value.to_string());
    attrib.is_multiline = true;
    attrib.embedded_mtext = Some(Box::new(MText::with_value("first\\Psecond", Vector3::ZERO)));
    let mut insert = crate::entities::Insert::new("B", Vector3::ZERO);
    insert.attributes.push(attrib);
    doc.add_entity(EntityType::Insert(insert)).unwrap();
    doc
}

#[test]
fn a_multiline_attribute_writes_group_1_empty_and_keeps_its_text_in_the_embedded_mtext() {
    let text = write(&multiline_attribute(DxfVersion::AC1032, ""));
    let attrib = records(&text, "ATTRIB")[0];
    assert_eq!(values(attrib, 1)[0], "", "AutoCAD writes the value empty: {attrib}");
    // The embedded MTEXT keeps the text (second group 1 of the record).
    assert_eq!(values(attrib, 1)[1], "first\\Psecond");

    // A value of its own is written as it is.
    let text = write(&multiline_attribute(DxfVersion::AC1032, "own"));
    assert_eq!(values(records(&text, "ATTRIB")[0], 1)[0], "own");
}

fn mtext_document(value: &str) -> CadDocument {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    doc.add_entity(EntityType::MText(Box::new(MText::with_value(value, Vector3::ZERO)))).unwrap();
    doc
}

#[test]
fn an_mtext_line_feed_is_written_as_a_caret_escape_not_as_a_paragraph_break() {
    let doc = mtext_document("one\ntwo\\Pthree\r\nfour");
    let text = write(&doc);
    let mtext = records(&text, "MTEXT")[0];
    assert_eq!(first(mtext, 1).as_deref(), Some("one^Jtwo\\Pthree^M^Jfour"));
    // Read back, the line feed is a line feed again and the paragraph break stays a paragraph break.
    let back = read(&text);
    let EntityType::MText(mtext) = back.entities().find(|e| matches!(e, EntityType::MText(_))).unwrap() else { unreachable!() };
    assert_eq!(mtext.value, "one\ntwo\\Pthree\r\nfour");

    // A binary DXF string holds the line feed as it is.
    let bytes = write_binary(&doc);
    let needle = b"one\ntwo\\Pthree\r\nfour\0";
    assert!(bytes.windows(needle.len()).any(|w| w == needle));
    let back = DxfReader::from_reader(Cursor::new(bytes)).unwrap().read().unwrap();
    let EntityType::MText(mtext) = back.entities().find(|e| matches!(e, EntityType::MText(_))).unwrap() else { unreachable!() };
    assert_eq!(mtext.value, "one\ntwo\\Pthree\r\nfour");

    // Text without a line feed is written as before.
    let plain = write(&mtext_document("plain \\Ptext"));
    assert_eq!(first(records(&plain, "MTEXT")[0], 1).as_deref(), Some("plain \\Ptext"));
}

#[test]
fn layer_material_is_written_from_r2007() {
    let mut layer = Layer::new("Wood");
    layer.handle = Handle::new(0x80);
    layer.material = Handle::new(0x9A);
    let mut doc = CadDocument::with_version(DxfVersion::AC1024);
    doc.layers.add(layer.clone()).unwrap();
    let text = write(&doc);
    let wood = records(&text, "LAYER").into_iter().find(|r| first(r, 2).as_deref() == Some("Wood")).unwrap();
    assert_eq!(first(wood, 347).as_deref(), Some("9A"));
    assert_eq!(read(&text).layers.get("Wood").unwrap().material, Handle::new(0x9A));

    let mut old = CadDocument::with_version(DxfVersion::AC1018);
    old.layers.add(layer).unwrap();
    let text = write(&old);
    let wood = records(&text, "LAYER").into_iter().find(|r| first(r, 2).as_deref() == Some("Wood")).unwrap();
    assert!(first(wood, 347).is_none(), "an R2004 layer has no material group");
}

#[test]
fn mleader_word_break_false_survives_a_file_older_than_r2013() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1024);
    let mut leader = MultiLeader::new();
    leader.set_text_content("Note", Vector3::ZERO);
    leader.context.word_break = false;
    doc.add_entity(EntityType::MultiLeader(Box::new(leader))).unwrap();
    let text = write(&doc);
    assert_eq!(first(records(&text, "MULTILEADER")[0], 295).as_deref(), Some("0"));
    let back = read(&text);
    let EntityType::MultiLeader(leader) = back.entities().find(|e| matches!(e, EntityType::MultiLeader(_))).unwrap() else { unreachable!() };
    assert!(!leader.context.word_break);

    // The default is not written before R2013 and is in R2013+.
    let mut plain = CadDocument::with_version(DxfVersion::AC1024);
    let mut with_text = MultiLeader::new();
    with_text.set_text_content("Note", Vector3::ZERO);
    plain.add_entity(EntityType::MultiLeader(Box::new(with_text.clone()))).unwrap();
    assert!(first(records(&write(&plain), "MULTILEADER")[0], 295).is_none());
    let mut modern = CadDocument::with_version(DxfVersion::AC1027);
    modern.add_entity(EntityType::MultiLeader(Box::new(with_text))).unwrap();
    assert_eq!(first(records(&write(&modern), "MULTILEADER")[0], 295).as_deref(), Some("1"));
}

#[test]
fn modeler_entity_uid_is_written_and_read_back() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut solid = Solid3D::new();
    solid.uid = "{1a113328-eb6d-d44d-824d-78b33668f9e7}".to_string();
    doc.add_entity(EntityType::Solid3D(solid)).unwrap();
    doc.add_entity(EntityType::Solid3D(Solid3D::new())).unwrap();
    let text = write(&doc);
    let solids = records(&text, "3DSOLID");
    assert_eq!(first(solids[0], 2).as_deref(), Some("{1a113328-eb6d-d44d-824d-78b33668f9e7}"));
    assert_eq!(first(solids[1], 2).as_deref(), Some("{00000000-0000-0000-0000-000000000000}"), "the zero GUID for none");
    let back = read(&text);
    let uids: Vec<String> = back
        .entities()
        .filter_map(|e| match e {
            EntityType::Solid3D(s) => Some(s.uid.clone()),
            _ => None,
        })
        .collect();
    assert!(uids.contains(&"{1a113328-eb6d-d44d-824d-78b33668f9e7}".to_string()));
}

#[test]
fn angles_are_written_as_the_degrees_that_read_back_exactly() {
    // Radians that come from a DXF degree value read back as that very value after a write.
    for degrees in [201.8_f64, 89.99, 45.0, 359.9999, 0.1, 12.345678901234567] {
        let radians = degrees.to_radians();
        let mut doc = CadDocument::with_version(DxfVersion::AC1032);
        let mut hatch = Hatch::new();
        hatch.is_solid = false;
        let line = HatchPatternLine {
            angle: radians,
            base_point: crate::types::Vector2::new(0.0, 0.0),
            offset: crate::types::Vector2::new(0.0, 1.0),
            dash_lengths: Vec::new(),
        };
        hatch.pattern.lines.push(line);
        doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
        let back = read(&write(&doc));
        let EntityType::Hatch(back) = back.entities().find(|e| matches!(e, EntityType::Hatch(_))).unwrap() else { unreachable!() };
        assert_eq!(back.pattern.lines[0].angle, radians, "{degrees} degrees");
    }
}

#[test]
fn an_empty_dimension_postfix_and_the_default_are_both_stated() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut empty = DimStyle::new("Plain");
    empty.handle = Handle::new(0x90);
    empty.dimpost = String::new();
    doc.dim_styles.add(empty).unwrap();
    let mut default = DimStyle::new("Default");
    default.handle = Handle::new(0x91);
    doc.dim_styles.add(default).unwrap();
    let text = write(&doc);
    let styles = records(&text, "DIMSTYLE");
    let find = |name: &str| styles.iter().copied().find(|r| first(r, 2).as_deref() == Some(name)).unwrap();
    assert_eq!(values(find("Plain"), 3), vec![String::new()], "an empty DIMPOST is written");
    assert_eq!(values(find("Default"), 3), vec!["<>".to_string()], "`<>` is stated: an absent group reads as empty");
    let back = read(&text);
    assert_eq!(back.dim_styles.get("Plain").unwrap().dimpost, "");
    assert_eq!(back.dim_styles.get("Default").unwrap().dimpost, "<>");
}

fn table_with_custom_data(version: DxfVersion) -> CadDocument {
    use crate::entities::table::{CellValue, CellValueType, Table, TableCustomData};
    let mut doc = CadDocument::with_version(version);
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    let mut checksum = CellValue::new();
    checksum.value_type = CellValueType::Double;
    checksum.raw_type_code = 2;
    checksum.numeric_value = 7783.0;
    let mut note = CellValue::new();
    note.value_type = CellValueType::String;
    note.raw_type_code = 4;
    note.text = "second item".to_string();
    table.rows[0].cells[0].custom_data_items = vec![
        TableCustomData { name: "ACAD_ROUNDTRIP_2008_CELL_CHECKSUM".to_string(), value: checksum },
        TableCustomData { name: "NOTE".to_string(), value: note },
    ];
    table.common.handle = Handle::new(0xB0);
    table.common.owner_handle = doc.header.named_objects_dict_handle;
    doc.objects.insert(table.common.handle, ObjectType::TableContent(table));
    doc
}

fn content_table(doc: &CadDocument) -> &crate::entities::Table {
    doc.objects
        .values()
        .find_map(|object| match object {
            ObjectType::TableContent(table) => Some(table),
            _ => None,
        })
        .expect("table content")
}

#[test]
fn table_cell_values_of_an_r2004_file_end_where_the_next_item_begins() {
    // AutoCAD's R2004 files have no ACVALUE_END (304) and no format groups after a value; the next data item or
    // the end marker ends it. The reader must not run on into the following groups.
    let written = write(&table_with_custom_data(DxfVersion::AC1018));
    let text = written.replace("304\r\nACVALUE_END\r\n", "");
    assert!(!text.contains("ACVALUE_END") && text.len() < written.len());
    let back = read(&text);
    let table = content_table(&back);
    let items = &table.rows[0].cells[0].custom_data_items;
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!(items[0].name, "ACAD_ROUNDTRIP_2008_CELL_CHECKSUM");
    assert_eq!(items[0].value.numeric_value, 7783.0);
    assert!(items[0].value.text.is_empty(), "{:?}", items[0].value.text);
    assert_eq!(items[1].name, "NOTE");
    assert_eq!(items[1].value.text, "second item");

    // With ACVALUE_END (as written, and as R2007+ files have it) the values read the same.
    let back = read(&written);
    assert_eq!(content_table(&back).rows[0].cells[0].custom_data_items[1].value.text, "second item");
    let modern = write(&table_with_custom_data(DxfVersion::AC1032));
    assert!(modern.contains("ACVALUE_END"));
    let back = read(&modern);
    let table = content_table(&back);
    assert_eq!(table.rows[0].cells[0].custom_data_items.len(), 2);
    assert_eq!(table.rows[0].cells[0].custom_data_items[0].value.numeric_value, 7783.0);
}

#[test]
fn a_table_cell_point_has_a_size_group_only_when_the_source_had_one() {
    use crate::entities::table::{CellValue, CellValueType, Table, TableCustomData};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    let mut point = CellValue::new();
    point.value_type = CellValueType::Point3D;
    point.raw_type_code = 0x20;
    point.point_value = Vector3::new(1.0, 2.0, 3.0);
    table.rows[0].cells[0].custom_data_items = vec![TableCustomData { name: "P".to_string(), value: point.clone() }];
    let mut with_point = table.clone();
    with_point.common.handle = Handle::new(0xB1);
    doc.objects.insert(with_point.common.handle, ObjectType::TableContent(with_point));
    let text = write(&doc);
    // The custom data value sits between `301 DATAMAP_VALUE` and `304 ACVALUE_END`.
    let start = text.find("DATAMAP_VALUE").unwrap();
    let value = &text[start..start + text[start..].find("ACVALUE_END").unwrap()];
    assert!(!value.contains("\r\n 92\r\n"), "no size group without a source size: {value}");

    point.data_size = 24;
    table.rows[0].cells[0].custom_data_items = vec![TableCustomData { name: "P".to_string(), value: point }];
    let mut sized = CadDocument::with_version(DxfVersion::AC1032);
    table.common.handle = Handle::new(0xB2);
    sized.objects.insert(table.common.handle, ObjectType::TableContent(table));
    let text = write(&sized);
    let start = text.find("DATAMAP_VALUE").unwrap();
    let value = &text[start..start + text[start..].find("ACVALUE_END").unwrap()];
    assert!(value.contains("\r\n 92\r\n    24\r\n") || value.contains("\r\n 92\r\n24\r\n"), "{value}");
}

#[test]
fn a_visual_style_with_extended_properties_is_written_in_the_layout_that_holds_them() {
    use crate::objects::VisualStyle;
    let mut style = VisualStyle::new();
    style.handle = Handle::new(0xA0);
    style.description = "Extended".to_string();
    let core = style.core_properties().len();
    let mut properties = style.core_properties();
    properties.extend(style.extended_properties());
    assert!(properties.len() > core);
    style.properties = properties.clone();
    for version in [DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        doc.objects.insert(style.handle, ObjectType::VisualStyle(style.clone()));
        let back = read(&write(&doc));
        let ObjectType::VisualStyle(back) = back.objects.get(&style.handle).expect("style") else { panic!() };
        assert_eq!(back.properties.len(), properties.len(), "{version:?}");
    }
}

#[test]
fn the_current_material_header_variable_is_written_and_read_back() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    doc.header.current_material_handle = Handle::new(0x96);
    let text = write(&doc);
    assert!(text.contains("  9\r\n$CMATERIAL\r\n347\r\n96\r\n"));
    assert_eq!(read(&text).header.current_material_handle, Handle::new(0x96));
    // A null material is the default and is not written, nor is the variable before R2007.
    assert!(!write(&CadDocument::with_version(DxfVersion::AC1032)).contains("$CMATERIAL"));
    let mut old = CadDocument::with_version(DxfVersion::AC1018);
    old.header.current_material_handle = Handle::new(0x96);
    assert!(!write(&old).contains("$CMATERIAL"));
}

#[test]
fn an_unrotated_mtext_without_a_direction_does_not_gain_one() {
    let text = write(&mtext_document("plain"));
    let record = records(&text, "MTEXT")[0];
    assert!(first(record, 11).is_none(), "no X-axis direction: {record}");
    let back = read(&text);
    let EntityType::MText(mtext) = back.entities().find(|e| matches!(e, EntityType::MText(_))).unwrap() else { unreachable!() };
    assert_eq!((mtext.rotation, mtext.dwg_x_direction), (0.0, None));

    // A rotated text states its direction, and so does a text that has one.
    let mut rotated = mtext_document("turned");
    let mut directed = mtext_document("directed");
    for (doc, rotation, direction) in [
        (&mut rotated, 0.5, None),
        (&mut directed, 0.0, Some(Vector3::new(1.0, 0.0, 0.0))),
    ] {
        let mut mtext = MText::with_value("x", Vector3::ZERO);
        mtext.rotation = rotation;
        mtext.dwg_x_direction = direction;
        doc.entities.clear();
        doc.entity_index.clear();
        doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    }
    for doc in [&rotated, &directed] {
        let text = write(doc);
        assert!(first(records(&text, "MTEXT")[0], 11).is_some());
        let back = read(&text);
        let EntityType::MText(back) = back.entities().find(|e| matches!(e, EntityType::MText(_))).unwrap() else { unreachable!() };
        let EntityType::MText(source) = doc.entities().find(|e| matches!(e, EntityType::MText(_))).unwrap() else { unreachable!() };
        assert!((back.rotation - source.rotation).abs() < 1e-12);
        assert!(back.dwg_x_direction.is_some());
    }
}

#[test]
fn a_multiline_attribute_read_from_dxf_writes_its_own_empty_group_1_back() {
    // The reader takes the text from the embedded MTEXT; the record's own group 1 (empty, as AutoCAD writes it)
    // is what goes back, and the value still reads back as the MTEXT text.
    let text = write(&multiline_attribute(DxfVersion::AC1032, ""));
    let back = read(&text);
    let attribute = |doc: &CadDocument| {
        doc.entities()
            .find_map(|e| match e {
                EntityType::Insert(insert) => insert.attributes.first().cloned(),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(attribute(&back).value, "first\\Psecond");
    let again = write(&back);
    let attrib = records(&again, "ATTRIB")[0];
    assert_eq!(values(attrib, 1), vec![String::new(), "first\\Psecond".to_string()], "{attrib}");
    assert_eq!(attribute(&read(&again)).value, "first\\Psecond");

    // An edited value is written as it is; before R2018 (no embedded MTEXT) the value carries the text.
    let mut edited = back.clone();
    for entity in edited.entities.iter_mut() {
        if let EntityType::Insert(insert) = std::sync::Arc::make_mut(entity) {
            insert.attributes[0].value = "edited".to_string();
        }
    }
    assert_eq!(values(records(&write(&edited), "ATTRIB")[0], 1)[0], "edited");
    let mut old = back.clone();
    old.version = DxfVersion::AC1027;
    assert_eq!(values(records(&write(&old), "ATTRIB")[0], 1)[0], "first\\Psecond");
}
