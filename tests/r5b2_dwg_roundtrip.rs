// SPDX-License-Identifier: MPL-2.0
//! R5b-2 (gate G5): defects of the DWG writer found by the DWG round trips of DXF sources (a DXF is written by
//! `DxfWriter` and read again, so the document is a DXF source) and of DWG sources, one test per fix. Each test builds a
//! synthetic document; no external file is used. Each test fails with its fix reverted.

use std::io::Cursor;

use acadrust::entities::{EntityType, Hatch, Line, MText};
use acadrust::objects::ObjectType;
use acadrust::tables::{BlockRecord, DimStyle, LineType};
use acadrust::types::{Color, DxfVersion, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

const R2000_PLUS: [DxfVersion; 5] = [
    DxfVersion::AC1015,
    DxfVersion::AC1018,
    DxfVersion::AC1024,
    DxfVersion::AC1027,
    DxfVersion::AC1032,
];

/// The document as a DXF source: written as ASCII DXF and read back.
fn dxf_source(document: &CadDocument) -> CadDocument {
    DxfReader::from_reader(Cursor::new(DxfWriter::new(document).write_to_vec().expect("DXF write")))
        .expect("DXF reader")
        .read()
        .expect("DXF read")
}

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn class_names(document: &CadDocument) -> Vec<String> {
    document.classes.iter().map(|class| class.dxf_name.clone()).collect()
}

/// A document of `version` whose CLASSES section, as a host application writes it, states a short list of classes with
/// their own flags and names, and which holds a layout (a fixed-code object the DXF list does not name).
fn document_with_stated_classes(version: DxfVersion) -> CadDocument {
    let mut document = CadDocument::with_version(version);
    document.classes.clear();
    document
        .classes
        .add_or_update(acadrust::classes::DxfClass::new("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault"));
    let mut visual = acadrust::classes::DxfClass::new("VISUALSTYLE", "AcDbVisualStyle");
    visual.proxy_flags = acadrust::classes::ProxyFlags(4095);
    visual.instance_count = 7;
    document.classes.add_or_update(visual);
    let mut custom = acadrust::classes::DxfClass::new("ACAD_EVALUATION_GRAPH", "AcDbEvalGraph");
    custom.application_name = "AcDbEvalGraph|Description: evaluation graph".into();
    custom.proxy_flags = acadrust::classes::ProxyFlags(1153);
    document.classes.add_or_update(custom);
    document
}

/// The classes of a DXF source are the ones its CLASSES section stated, in order and with their own flags and names; the
/// classes of the fixed object types a DWG states (LAYOUT, ACDBPLACEHOLDER) follow ACDBDICTIONARYWDFLT.
/// AutoCAD's own DWG and DXF of one drawing differ in exactly these two classes (sample_AC1032 and its DXF).
#[test]
fn a_dxf_source_writes_the_class_list_its_source_stated() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        let source = dxf_source(&document_with_stated_classes(version));
        assert!(source.classes.source_classes().is_some(), "{version:?}: read from a DXF");
        let stated: Vec<String> = source.classes.source_classes().unwrap().iter().map(|c| c.dxf_name.clone()).collect();
        let read = write_and_read(&source, version);
        let names = class_names(&read);
        // The default registry (ACadSharp's UpdateDxfClasses, 250 classes) is not written for a source that lists its own.
        assert!(names.len() < 40, "{version:?}: {} classes written: {names:?}", names.len());
        for name in &stated {
            assert!(names.contains(name), "{version:?}: class {name} is stated by the source: {names:?}");
        }
        let visual = read.classes.get_by_name("VISUALSTYLE").expect("VISUALSTYLE");
        assert_eq!(visual.proxy_flags.0, 4095, "{version:?}: proxy flags as the source states them");
        let custom = read.classes.get_by_name("ACAD_EVALUATION_GRAPH").expect("custom class");
        assert_eq!(custom.application_name, "AcDbEvalGraph|Description: evaluation graph", "{version:?}");
        assert_eq!(custom.proxy_flags.0, 1153, "{version:?}");
        // The layouts of the document are written under fixed type codes, and their classes follow the dictionary one
        // when the source's list does not name them.
        let at = |name: &str| names.iter().position(|n| n == name);
        if let Some(wdflt) = at("ACDBDICTIONARYWDFLT") {
            if !stated.iter().any(|n| n == "LAYOUT") {
                assert_eq!(at("ACDBPLACEHOLDER"), Some(wdflt + 1), "{version:?}: {names:?}");
                assert_eq!(at("LAYOUT"), Some(wdflt + 2), "{version:?}: {names:?}");
            }
        }
    }
}

/// The entities of MLINE and OLE2FRAME have fixed type codes; a DXF whose CLASSES does not name them gets no class for
/// them, as AutoCAD's own DWG has none.
#[test]
fn fixed_code_entities_do_not_add_a_class_to_a_dxf_source() {
    let mut document = document_with_stated_classes(DxfVersion::AC1015);
    document
        .add_entity(EntityType::MLine(acadrust::entities::MLine::new()))
        .unwrap();
    let source = dxf_source(&document);
    let read = write_and_read(&source, DxfVersion::AC1015);
    assert!(!class_names(&read).iter().any(|name| name == "MLINE"), "{:?}", class_names(&read));
}

/// A same-version source keeps the class it stated as live and the count it stated: the records a file counts under a
/// class (history objects in the ACDS data, annotation context data) are not the objects the census enumerates.
#[test]
fn a_dxf_source_keeps_its_stated_class_counts() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
        let source = dxf_source(&document_with_stated_classes(version));
        let read = write_and_read(&source, version);
        let visual = read.classes.get_by_name("VISUALSTYLE").unwrap();
        assert_eq!(visual.instance_count, 7, "{version:?}: the stated count");
        assert!(!visual.was_zombie, "{version:?}");
        // Another version gets the census.
        let other = write_and_read(&source, if version == DxfVersion::AC1032 { DxfVersion::AC1027 } else { DxfVersion::AC1024 });
        assert_ne!(other.classes.get_by_name("VISUALSTYLE").map(|c| c.instance_count), Some(7), "{version:?}: not the stated count");
    }
}

/// The native-dump record of a block's markers (`block_marker/<block record handle>`): the colours of BLOCK and ENDBLK.
fn marker_colors(document: &CadDocument, block: &str) -> (serde_json::Value, serde_json::Value) {
    let handle = document.block_records.get(block).expect("block record").handle;
    let mut dump = Vec::new();
    acadrust::native_dump::write_native_dump(document, &mut dump).unwrap();
    let key = format!("\"k\":\"block_marker/{:016X}\"", handle.value());
    let line = String::from_utf8(dump).unwrap().lines().find(|line| line.contains(&key)).expect("marker record").to_string();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    (value["v"]["begin"]["properties"]["color"].clone(), value["v"]["end"]["properties"]["color"].clone())
}

/// The BLOCK and ENDBLK entities of a DXF carry colour, layer and the rest of an entity's common data; the DWG states
/// them (they were written as the defaults).
#[test]
fn block_markers_of_a_dxf_source_state_their_common_data() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut record = BlockRecord::new("MARKED");
        record.handle = document.allocate_handle();
        let mut begin = acadrust::entities::EntityCommon::default();
        begin.color = Color::Index(1);
        let mut end = acadrust::entities::EntityCommon::default();
        end.color = Color::Index(3);
        record.block_entity_common = Some(Box::new(begin));
        record.block_end_common = Some(Box::new(end));
        document.block_records.add(record).unwrap();
        let source = dxf_source(&document);
        assert_eq!(marker_colors(&source, "MARKED").0["0"], 1, "{version:?}: the DXF states the colour");
        let read = write_and_read(&source, version);
        let (begin, end) = marker_colors(&read, "MARKED");
        assert_eq!(begin["0"], 1, "{version:?}: BLOCK colour {begin}");
        assert_eq!(end["0"], 3, "{version:?}: ENDBLK colour {end}");
    }
}

/// A DXF names the current dimension style, text style, layer and linetype ($DIMSTYLE, ...) and never states a handle: the
/// DWG header's current styles are the named ones.
#[test]
fn a_dxf_source_writes_the_current_styles_it_names() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut style = DimStyle::new("CUSTOM");
        style.handle = document.allocate_handle();
        document.dim_styles.add(style).unwrap();
        document.header.current_dimstyle_name = "CUSTOM".into();
        let source = dxf_source(&document);
        assert_eq!(source.header.current_dimstyle_name, "CUSTOM");
        let read = write_and_read(&source, version);
        assert_eq!(read.header.current_dimstyle_name, "CUSTOM", "{version:?}");
    }
}

/// The handle of a document built in memory still wins over a stale name: the rule above is for a DXF source.
#[test]
fn a_constructed_document_keeps_the_handle_of_its_current_style() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut style = DimStyle::new("BYHANDLE");
    style.handle = document.allocate_handle();
    let handle = style.handle;
    document.dim_styles.add(style).unwrap();
    document.header.current_dimstyle_handle = handle;
    document.header.current_dimstyle_name = "Standard".into();
    assert_eq!(write_and_read(&document, DxfVersion::AC1032).header.current_dimstyle_name, "BYHANDLE");
}

/// A DXF names the arrow blocks and the dimension line types; a DWG states them as handles, and AutoCAD's built-in
/// arrowheads are the blocks `_<name>`.
#[test]
fn a_dxf_source_writes_arrow_blocks_and_dimension_line_types_by_name() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut record = BlockRecord::new("_BoxBlank");
        record.handle = document.allocate_handle();
        document.block_records.add(record).unwrap();
        let mut line_type = LineType::new("DASHED");
        line_type.handle = document.allocate_handle();
        document.line_types.add(line_type).unwrap();
        document.header.dim_arrow_block = "BoxBlank".into();
        document.header.dim_linetype_name = "DASHED".into();
        let source = dxf_source(&document);
        let read = write_and_read(&source, version);
        assert_eq!(read.header.dim_arrow_block, "_BoxBlank", "{version:?}: the arrow block");
        if version >= DxfVersion::AC1024 {
            assert_eq!(read.header.dim_linetype_name, "DASHED", "{version:?}: the dimension line type");
        }
    }
}

/// A DIMSTYLE record of a DXF omits group 3 when the suffix is empty, and AutoCAD's DWG states the empty text: a record
/// read without group 3 has an empty suffix, and the suffix `<>` is stated so that it reads back as `<>`.
#[test]
fn an_omitted_dimension_postfix_is_written_empty() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut style = DimStyle::new("NOPOST");
        style.handle = document.allocate_handle();
        style.dimpost = "".into();
        document.dim_styles.add(style).unwrap();
        let mut style = DimStyle::new("MEASURE");
        style.handle = document.allocate_handle();
        style.dimpost = "<>".into();
        document.dim_styles.add(style).unwrap();
        let text = String::from_utf8(DxfWriter::new(&document).write_to_vec().unwrap()).unwrap();
        // Take group 3 out of the NOPOST record, as AutoCAD's DXF does.
        let lines: Vec<&str> = text.split('\n').collect();
        let mut kept: Vec<&str> = Vec::new();
        let (mut in_record, mut at) = (false, 0);
        while at + 1 < lines.len() {
            let (code, value) = (lines[at].trim(), lines[at + 1].trim());
            if code == "0" {
                in_record = false;
            }
            if code == "2" && value == "NOPOST" {
                in_record = true;
            }
            if in_record && code == "3" {
                at += 2;
                continue;
            }
            kept.push(lines[at]);
            kept.push(lines[at + 1]);
            at += 2;
        }
        let source = DxfReader::from_reader(Cursor::new(kept.join("\n").into_bytes())).unwrap().read().unwrap();
        assert_eq!(source.dim_styles.get("NOPOST").unwrap().dimpost, "", "{version:?}: an absent group is the empty suffix");
        assert_eq!(source.dim_styles.get("MEASURE").unwrap().dimpost, "<>", "{version:?}: `<>` is stated");
        let read = write_and_read(&source, version);
        assert_eq!(read.dim_styles.get("NOPOST").unwrap().dimpost, "", "{version:?}");
        assert_eq!(read.dim_styles.get("MEASURE").unwrap().dimpost, "<>", "{version:?}");
        // A suffix the DXF states is written as stated.
        let mut document = CadDocument::with_version(version);
        let mut style = DimStyle::new("MM");
        style.handle = document.allocate_handle();
        style.dimpost = "<> mm".into();
        document.dim_styles.add(style).unwrap();
        assert_eq!(write_and_read(&dxf_source(&document), version).dim_styles.get("MM").unwrap().dimpost, "<> mm", "{version:?}");
    }
}

/// AutoCAD's DWG states the gradient name LINEAR for a hatch without a gradient, where a DXF omits it; a hatch built in
/// memory keeps its empty name through a DWG.
#[test]
fn hatches_of_a_dxf_source_state_the_default_gradient_name() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        document.add_entity(EntityType::Hatch(Box::new(Hatch::new()))).unwrap();
        let name_of = |document: &CadDocument| {
            document
                .entities()
                .find_map(|e| match e {
                    EntityType::Hatch(h) => Some(h.gradient_color.name.clone()),
                    _ => None,
                })
                .unwrap()
        };
        if version >= DxfVersion::AC1018 {
            assert_eq!(name_of(&write_and_read(&dxf_source(&document), version)), "LINEAR", "{version:?}: a DXF source");
        }
        assert_eq!(name_of(&write_and_read(&document, version)), "", "{version:?}: a constructed document");
    }
}

/// A multi-line attribute of R2013 and earlier is a TEXT with an ACAD_MLATT record; the outer tag has an artificial
/// `_001` suffix that the native tag drops and the source tag keeps, and the file states it again.
#[test]
fn a_recovered_multiline_attribute_tag_is_written_as_the_source_spelled_it() {
    let mut text = acadrust::entities::text::TextSource::default();
    text.tag = Some(("MULTI_LINE_ATT_001".to_string(), "MULTI_LINE_ATT".to_string()));
    let mut attribute = acadrust::entities::AttributeEntity::new("MULTI_LINE_ATT".to_string(), "first line".to_string());
    attribute.source = Some(Box::new(text));
    let mut document = CadDocument::with_version(DxfVersion::AC1024);
    let mut insert = acadrust::entities::Insert::new("BLK", Vector3::ZERO);
    insert.attributes.push(attribute);
    let mut record = BlockRecord::new("BLK");
    record.handle = document.allocate_handle();
    document.block_records.add(record).unwrap();
    document.add_entity(EntityType::Insert(insert)).unwrap();
    let read = write_and_read(&document, DxfVersion::AC1024);
    let tags: Vec<String> = read
        .entities()
        .filter_map(|e| match e {
            EntityType::Insert(i) => Some(i.attributes.iter().map(|a| a.tag.clone()).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(tags, vec!["MULTI_LINE_ATT_001".to_string()]);
}

/// A hidden per-viewport layer record (`<base> @ <viewport>`) is the layer an entity is on; the entity names the base
/// layer and the record's handle, and the DWG states the record, not the base layer.
#[test]
fn an_entity_on_a_viewport_override_layer_record_names_the_record() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut record = acadrust::tables::Layer::new("0 @ 7");
        record.handle = document.allocate_handle();
        let handle = record.handle;
        document.layers.add(record).unwrap();
        let mut line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        line.common.layer = "0".into();
        line.common.layer_handle = Some(handle);
        document.add_entity(EntityType::Line(line)).unwrap();
        let read = write_and_read(&document, version);
        let on = read.entities().find_map(|e| match e {
            EntityType::Line(l) => Some((l.common.layer.clone(), l.common.layer_handle)),
            _ => None,
        });
        let (layer, layer_handle) = on.expect("line");
        assert_eq!(layer, "0", "{version:?}: the base layer name");
        let record = read.layers.iter().find(|entry| entry.name == "0 @ 7").expect("the override record").handle;
        assert_eq!(layer_handle, Some(record), "{version:?}: the record the entity is on, not layer 0");
    }
}

/// Instance counts of the stated class table and the maintenance release of the AC1018 file header: AutoCAD's R2004
/// files state 104 (sample_AC1018.dwg and the $ACADMAINTVER of its DXF).
#[test]
fn an_r2004_file_states_the_maintenance_release_autocad_states() {
    let document = CadDocument::with_version(DxfVersion::AC1018);
    let bytes = DwgWriter::write_to_vec(&document).unwrap();
    assert_eq!(bytes[0x0B], 104);
    let read = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
    assert_eq!(read.header.maintenance_version, 104);
}

/// A Template section of zeros (the empty description and MEASUREMENT 0) is data: it is written, read back, and a reader
/// that takes a missing section for the metric default (ACadSharp does) reads the English measurement the DXF stated.
#[test]
fn a_template_of_zeros_is_a_section() {
    for version in [DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        document.header.measurement = 0;
        let read = write_and_read(&document, version);
        assert!(read.header.dwg_template_data.is_some(), "{version:?}: the Template section is there");
        assert_eq!(read.header.measurement, 0, "{version:?}");
    }
}

/// An AutoCAD file of R2004 that has no Template section keeps none on a same-version write.
#[test]
fn a_same_version_file_without_a_template_section_keeps_none() {
    let document = CadDocument::with_version(DxfVersion::AC1018);
    let mut first = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&document).unwrap())).read().unwrap();
    first.header.dwg_template_data = None;
    let second = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&first).unwrap())).read().unwrap();
    assert!(second.header.dwg_template_data.is_none());
}

/// An MText with a CIF escape keeps as one decoded character through a DWG, whichever the format of the source.
#[test]
fn a_dxf_source_mtext_round_trips() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = MText::new();
    text.value = "94\u{b0}".into();
    document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    let read = write_and_read(&dxf_source(&document), DxfVersion::AC1032);
    let value = read.entities().find_map(|e| match e {
        EntityType::MText(m) => Some(m.value.clone()),
        _ => None,
    });
    assert_eq!(value.as_deref(), Some("94\u{b0}"));
}

/// An MText whose source spells a character as a `\U+XXXX` escape (AutoCAD keeps the escape in the MTEXT of its own DWG of
/// every version) is written to DWG with that spelling while the value is unedited, as the DXF writer writes it: the
/// re-read text has the escape again (and the decoded value). An edited value is written as it is.
#[test]
fn an_mtext_keeps_the_escape_its_source_spelled() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = MText::new();
    text.value = "94\\U+00B0".into();
    document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    let mtext = |document: &CadDocument| {
        document
            .entities()
            .find_map(|e| match e {
                EntityType::MText(m) => Some((m.value.clone(), m.source_value.clone())),
                _ => None,
            })
            .expect("mtext")
    };
    let source = dxf_source(&document);
    assert_eq!(mtext(&source), ("94\u{b0}".to_string(), Some("94\\U+00B0".to_string())), "the DXF source");
    for version in R2000_PLUS {
        let from_dxf = write_and_read(&source, version);
        assert_eq!(mtext(&from_dxf), mtext(&source), "{version:?}: the DWG of the DXF source");
        let from_dwg = write_and_read(&from_dxf, version);
        assert_eq!(mtext(&from_dwg), mtext(&source), "{version:?}: the DWG of the DWG source");
    }
    let mut edited = source.clone();
    for entity in edited.entities_mut() {
        if let EntityType::MText(m) = entity {
            m.value = "95\u{b0}".into();
        }
    }
    assert_eq!(mtext(&write_and_read(&edited, DxfVersion::AC1032)).0, "95\u{b0}", "an edited value");
}

/// The reference point of a 3D solid whose geometry is kept in the data store (R2013 and later) is the source's own data
/// (the centre of the bounding box, as AutoCAD writes it) and is written back.
#[test]
fn a_data_store_solid_keeps_its_reference_point() {
    for version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let mut solid = acadrust::entities::Solid3D::new();
        solid.acis_data.sab_data = b"ACIS BinaryFile".to_vec();
        solid.acis_data.is_binary = true;
        solid.acis_data.wireframe_data_present = true;
        solid.acis_data.wireframe_point_present = true;
        solid.point_of_reference = Vector3::new(1.5, -2.5, 3.5);
        document.add_entity(EntityType::Solid3D(solid)).unwrap();
        let read = write_and_read(&document, version);
        let point = read.entities().find_map(|e| match e {
            EntityType::Solid3D(s) => Some(s.point_of_reference),
            _ => None,
        });
        assert_eq!(point, Some(Vector3::new(1.5, -2.5, 3.5)), "{version:?}");
    }
}

fn table_with_empty_items(rows: usize, columns: usize, merged: &[(usize, usize, usize, usize)]) -> acadrust::entities::Table {
    use acadrust::entities::table::{CellContent, CellRange};
    let mut table = acadrust::entities::Table::new(Vector3::ZERO, rows, columns);
    for row in &mut table.rows {
        for cell in &mut row.cells {
            cell.contents = vec![CellContent::new()];
        }
    }
    for &(top_row, left_col, bottom_row, right_col) in merged {
        table.merged_ranges.push(CellRange { top_row, left_col, bottom_row, right_col });
    }
    table
}

/// A TABLECONTENT object of a DXF states an (empty) item for every cell; AutoCAD's own DWG entity states none for the
/// cells a merge of one row covers (the title row) and keeps the items of the cells a merge of several rows covers.
#[test]
fn cells_covered_by_a_one_row_merge_have_no_empty_item() {
    for version in [DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let table = table_with_empty_items(4, 3, &[(0, 0, 0, 2), (2, 0, 3, 2)]);
        document.add_entity(EntityType::Table(Box::new(table))).unwrap();
        let read = write_and_read(&document, version);
        let counts: Vec<Vec<usize>> = read
            .entities()
            .find_map(|e| match e {
                EntityType::Table(t) => Some(t.rows.iter().map(|row| row.cells.iter().map(|c| c.contents.len()).collect()).collect()),
                _ => None,
            })
            .expect("table");
        assert_eq!(counts[0], vec![1, 0, 0], "{version:?}: the title row merge");
        assert_eq!(counts[1], vec![1, 1, 1], "{version:?}");
        assert_eq!(counts[2], vec![1, 1, 1], "{version:?}: the first row of a two-row merge");
        assert_eq!(counts[3], vec![1, 1, 1], "{version:?}: the covered row of a two-row merge");
    }
}

/// A DXF names the diffuse, specular, reflection, opacity and bump map sources only when they are not the default, and
/// AutoCAD's own DWG states the image file for them; a map with a file name must keep it (it was dropped with source 0),
/// and the material itself is kept by a same-version DWG of R2000 (AutoCAD's DXF and DWG of that version carry it).
#[test]
fn materials_of_a_dxf_source_keep_their_map_files() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let mut material = acadrust::objects::Material::new();
        material.name = "TILES".into();
        material.handle = document.allocate_handle();
        material.owner = document.header.named_objects_dict_handle;
        material.diffuse_map.file_name = "C:\\tiles\\floor.png".into();
        let handle = material.handle;
        document.objects.insert(handle, ObjectType::Material(material));
        if let Some(ObjectType::Dictionary(root)) = document.objects.get_mut(&document.header.named_objects_dict_handle) {
            root.add_entry("TILES_HOLDER", handle);
        }
        // AutoCAD's DXF names the map source (group 72) only when it is not the default: take it out of the text.
        let text = String::from_utf8(DxfWriter::new(&document).write_to_vec().unwrap()).unwrap();
        let lines: Vec<&str> = text.split('\n').collect();
        let mut kept: Vec<&str> = Vec::new();
        let mut in_material = false;
        let mut at = 0;
        while at + 1 < lines.len() {
            let (code, value) = (lines[at].trim(), lines[at + 1].trim());
            if code == "0" {
                in_material = value == "MATERIAL";
            }
            if in_material && code == "72" {
                at += 2;
                continue;
            }
            kept.push(lines[at]);
            kept.push(lines[at + 1]);
            at += 2;
        }
        let source = DxfReader::from_reader(Cursor::new(kept.join("\n").into_bytes())).unwrap().read().unwrap();
        assert!(source.objects.values().any(|o| matches!(o, ObjectType::Material(m) if m.name == "TILES" && m.diffuse_map.source == 1)), "{version:?}: read from the DXF without group 72 (the image file)");
        let read = write_and_read(&source, version);
        let material = read
            .objects
            .values()
            .find_map(|o| match o {
                ObjectType::Material(m) if m.name == "TILES" => Some(m.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{version:?}: the material is written"));
        assert_eq!(material.diffuse_map.file_name, "C:\\tiles\\floor.png", "{version:?}");
        assert_eq!(material.diffuse_map.source, 1, "{version:?}: the image file");
        assert_eq!(material.refraction_map.source, 0, "{version:?}: the refraction map keeps the current scene");
    }
}

fn style_properties(document: &CadDocument) -> Vec<(i16, String)> {
    document
        .objects
        .values()
        .find_map(|o| match o {
            ObjectType::VisualStyle(style) => Some(style.properties.iter().map(|p| (p.enabled, format!("{:?}", p.value))).collect()),
            _ => None,
        })
        .unwrap_or_default()
}

fn roundtrip_record_entries(document: &CadDocument) -> Vec<String> {
    // the record as the file held it: a reader takes the record it applied out of the model (R5b-10)
    let document = &document.with_round_trip_records();
    for object in document.objects.values() {
        if let ObjectType::VisualStyle(style) = object {
            if let Some(dictionary) = style.xdictionary_handle {
                if let Some(ObjectType::Dictionary(entries)) = document.objects.get(&dictionary) {
                    if let Some(record) = entries.get("ACAD_XREC_ROUNDTRIP") {
                        if let Some(ObjectType::XRecord(record)) = document.objects.get(&record) {
                            return record.entries.iter().map(|e| format!("{}:{:?}", e.code, e.value)).collect();
                        }
                    }
                }
            }
        }
    }
    Vec::new()
}

/// A visual style of an R2010 file has 28 properties and states the later ones in its ACAD_XREC_ROUNDTRIP record. Written
/// to R2004 (which has the 24 of the legacy layout and the record) twice, the record must be the same; written to R2013
/// (all 58 in the record of properties) the values of the record are the style's properties.
#[test]
fn an_r2010_visual_style_keeps_its_later_properties_through_other_versions() {
    // A style of 58 properties (the ones an R2013 file reads), one of the later ones changed.
    let mut document = CadDocument::with_version(DxfVersion::AC1027);
    let mut style = acadrust::objects::VisualStyle::new();
    style.handle = document.allocate_handle();
    style.owner = document.header.named_objects_dict_handle;
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::VisualStyle(style));
    let mut full = write_and_read(&document, DxfVersion::AC1027);
    let changed = {
        let ObjectType::VisualStyle(style) = full.objects.values_mut().find(|o| matches!(o, ObjectType::VisualStyle(_))).unwrap() else { unreachable!() };
        assert_eq!(style.properties.len(), 58);
        let property = &mut style.properties[30];
        property.value = match &property.value {
            acadrust::objects::VisualStylePropertyValue::Long(_) => acadrust::objects::VisualStylePropertyValue::Long(7),
            acadrust::objects::VisualStylePropertyValue::Double(_) => acadrust::objects::VisualStylePropertyValue::Double(7.0),
            acadrust::objects::VisualStylePropertyValue::Bool(v) => acadrust::objects::VisualStylePropertyValue::Bool(!v),
            other => other.clone(),
        };
        format!("{:?}", property.value)
    };
    // The file of R2004 AutoCAD writes from it carries the later properties in the round-trip record.
    let r2004 = write_and_read(&full, DxfVersion::AC1018);
    // What an R2010 file holds: the first 28 properties and the record without the property flags group.
    let mut r2010 = r2004.with_round_trip_records();
    r2010.version = DxfVersion::AC1024;
    let style_handle = r2010.objects.iter().find_map(|(h, o)| matches!(o, ObjectType::VisualStyle(_)).then_some(*h)).unwrap();
    if let Some(ObjectType::VisualStyle(style)) = r2010.objects.get_mut(&style_handle) {
        style.properties.truncate(28);
    }
    let record = r2010
        .extension_dictionary_handle(style_handle)
        .and_then(|d| match r2010.objects.get(&d) {
            Some(ObjectType::Dictionary(entries)) => entries.get("ACAD_XREC_ROUNDTRIP"),
            _ => None,
        })
        .expect("round-trip record");
    if let Some(ObjectType::XRecord(record)) = r2010.objects.get_mut(&record) {
        let mut kept = Vec::new();
        let mut entries = std::mem::take(&mut record.entries).into_iter();
        while let Some(entry) = entries.next() {
            if entry.value.as_string().is_some_and(|key| key.starts_with("RTVSPropertyOp")) {
                entries.next();
                continue;
            }
            kept.push(entry);
        }
        record.entries = kept;
    }
    // R2010 to R2004 twice: the record is stable.
    let first = write_and_read(&r2010, DxfVersion::AC1018);
    let second = write_and_read(&first, DxfVersion::AC1018);
    assert_eq!(roundtrip_record_entries(&first), roundtrip_record_entries(&second), "R2004 twice");
    // R2010 to R2013: the later properties are the style's.
    for version in [DxfVersion::AC1027, DxfVersion::AC1032] {
        let read = write_and_read(&r2010, version);
        let properties = style_properties(&read);
        assert_eq!(properties.len(), 58, "{version:?}");
        assert_eq!(properties[30].1, changed, "{version:?}: the property the record stated");
        assert!(roundtrip_record_entries(&read).is_empty(), "{version:?}: the record is dropped once the properties hold it");
    }
}

fn has_pre2007_carrier(document: &CadDocument) -> bool {
    document.objects.values().any(|object| match object {
        ObjectType::XRecord(record) => record
            .entries
            .iter()
            .any(|entry| entry.value.as_string() == Some("ACAD_ROUNDTRIP_PRE2007_TABLESTYLE")),
        _ => false,
    })
}

/// A table style that an R14 file states only by its ACAD_ROUNDTRIP_PRE2007_TABLESTYLE record (the legacy rows' data types)
/// loses the record when a file of R2010 or later is written (the TABLESTYLE record states the types itself), and the
/// second write has nothing more to drop: the same document written twice is the same file.
#[test]
fn the_pre_2007_table_style_record_goes_with_the_first_modern_write() {
    use acadrust::objects::NamedTableCellStyle;
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut style = acadrust::objects::TableStyle::new("TS");
    style.handle = document.allocate_handle();
    style.owner_handle = document.header.named_objects_dict_handle;
    style.modern_overrides.push((1, NamedTableCellStyle::default()));
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::TableStyle(style));
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let mut old = write_and_read(&document, version);
        assert!(has_pre2007_carrier(&old), "{version:?}: the file states the record");
        // The style of a file that has the record and no cell style map.
        for object in old.objects.values_mut() {
            if let ObjectType::TableStyle(style) = object {
                style.modern_overrides.clear();
            }
        }
        let first = write_and_read(&old, DxfVersion::AC1032);
        let second = write_and_read(&first, DxfVersion::AC1032);
        assert_eq!(has_pre2007_carrier(&first), has_pre2007_carrier(&second), "{version:?}: written twice");
        assert!(!has_pre2007_carrier(&first), "{version:?}: an R2018 file does not carry the record");
    }
}

/// The first long of the R2010+ TABLESTYLE record states bit 8 in every AutoCAD file (R2010 to R2018). It is not a style
/// flag: the reader keeps it out of `flags` and in `modern_unknown_long1`, and the writer states it again, so the record
/// reads the same after every write and the flags a caller set are not changed.
#[test]
fn the_table_style_record_of_r2010_keeps_bit_8_of_its_first_long_outside_the_flags() {
    for version in [DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        let mut document = CadDocument::with_version(version);
        let mut style = acadrust::objects::TableStyle::new("TS");
        style.handle = document.allocate_handle();
        style.owner_handle = document.header.named_objects_dict_handle;
        style.modern_unknown_long1 = 8;
        let handle = style.handle;
        document.objects.insert(handle, ObjectType::TableStyle(style));
        let first = write_and_read(&document, version);
        let second = write_and_read(&first, version);
        for read in [&first, &second] {
            let Some(ObjectType::TableStyle(style)) = read.objects.values().find(|object| {
                matches!(object, ObjectType::TableStyle(style) if style.name == "TS")
            }) else {
                panic!("{version:?}: table style missing");
            };
            assert_eq!(style.modern_unknown_long1 & 8, 8, "{version:?}: bit 8 stated");
            assert_eq!(style.flags.bits() & 8, 0, "{version:?}: bit 8 is not a style flag");
        }
    }
}

/// A DXF of R2000 or R2004 that states the grid behaviour bits of a viewport keeps them in its DWG, and so does a constructed
/// document: the status word of every R2000+ viewport holds them (AutoCAD's own sample_AC1015.dwg and sample_AC1018.dwg state
/// them as sample_AC1032.dwg does; R5b-4 replaced "R2007 and later only").
#[test]
fn a_dxf_source_keeps_the_grid_bits_of_its_viewport_before_r2007() {
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let mut document = CadDocument::with_version(version);
        let mut viewport = acadrust::entities::Viewport::new();
        viewport.grid_flags.adaptive = true;
        viewport.grid_flags.subdivision = true;
        viewport.id = 2;
        document.add_entity(EntityType::Viewport(Box::new(viewport))).unwrap();
        let bits = |document: &CadDocument| {
            document
                .entities()
                .find_map(|e| match e {
                    EntityType::Viewport(v) => Some(v.grid_flags.to_bits()),
                    _ => None,
                })
                .expect("viewport")
        };
        // The status word (group 90) of a DXF of this version as AutoCAD writes it states the grid bits (18 to 21): put them
        // into the text.
        let text = String::from_utf8(DxfWriter::new(&document).write_to_vec().unwrap()).unwrap();
        let lines: Vec<&str> = text.split('\n').collect();
        let mut patched: Vec<String> = Vec::new();
        let mut in_viewport = false;
        let mut at = 0;
        while at + 1 < lines.len() {
            let (code, value) = (lines[at].trim(), lines[at + 1].trim());
            if code == "0" {
                in_viewport = value == "VIEWPORT";
            }
            patched.push(lines[at].to_string());
            if in_viewport && code == "90" {
                patched.push(format!("{}", value.parse::<i32>().unwrap() | (6 << 18)));
            } else {
                patched.push(lines[at + 1].to_string());
            }
            at += 2;
        }
        let source = DxfReader::from_reader(Cursor::new(patched.join("\n").into_bytes())).unwrap().read().unwrap();
        let stated = bits(&source);
        assert_eq!(stated, acadrust::entities::viewport::GridFlags { beyond_limits: false, adaptive: true, subdivision: true, follow_dynamic: false }.to_bits(), "{version:?}: the DXF states them");
        assert_eq!(bits(&write_and_read(&source, version)), stated, "{version:?}: the DWG of a DXF source");
        assert_eq!(bits(&write_and_read(&document, version)), bits(&document), "{version:?}: a constructed document");
    }
}
