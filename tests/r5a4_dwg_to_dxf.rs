// SPDX-License-Identifier: MPL-2.0
//! Job R5a-4 (gate G4-RR, DWG-sourced round trips): each test writes a synthetic document as DWG and reads it, as a
//! DWG source, then writes that document as DXF and reads it again, and checks that the native model of the two reads
//! agrees on the datum the fix is about. No external file is used.
use std::io::Cursor;

use acadrust::entities::{EntityType, Line, Solid3D};
use acadrust::objects::ObjectType;
use acadrust::types::{Color, Handle, Vector3};
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

/// A document of `version` with one owned dictionary entry (a hard-owned extension dictionary and its XRECORD).
fn document_with_owned_entry(version: DxfVersion) -> (CadDocument, Handle) {
    let mut doc = CadDocument::with_version(version);
    let owner = doc.layers.iter().next().expect("layer 0").handle;
    let record = doc.ensure_xrecord(owner, "R5A4_ENTRY");
    (doc, record)
}

#[test]
fn dwg_dictionary_states_hard_ownership_per_dictionary_and_the_entries_follow() {
    let (doc, _) = document_with_owned_entry(DxfVersion::AC1032);
    let read = dwg(&doc);
    let dictionary = read
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Dictionary(dictionary) if dictionary.get("R5A4_ENTRY").is_some() => Some(dictionary),
            _ => None,
        })
        .expect("extension dictionary");
    assert!(dictionary.hard_owner, "the DWG states the dictionary as hard-owning");
    assert!(
        dictionary.is_entry_hard_owner("R5A4_ENTRY"),
        "the entry is hard-owned, as the DXF reader records it for the same drawing"
    );
    let back = dxf(&read);
    let again = back
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::Dictionary(dictionary) if dictionary.get("R5A4_ENTRY").is_some() => Some(dictionary),
            _ => None,
        })
        .expect("dictionary after the DXF round trip");
    assert_eq!(again.hard_owner_entries, dictionary.hard_owner_entries);
}

#[test]
fn dwg_header_names_and_summary_agree_with_the_dxf_read() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut style = acadrust::tables::DimStyle::new("R5A4-DIM");
    style.handle = doc.allocate_handle();
    let style_handle = style.handle;
    doc.dim_styles.add(style).expect("dim style");
    doc.header.current_dimstyle_handle = style_handle;
    doc.header.current_dimstyle_name = "Standard".into();
    let mut mline = acadrust::objects::MLineStyle::new("R5A4-ML");
    mline.handle = doc.allocate_handle();
    let mline_handle = mline.handle;
    doc.objects.insert(mline_handle, ObjectType::MLineStyle(mline));
    doc.header.current_multiline_style_handle = mline_handle;
    doc.header.multiline_style = "Standard".into();
    doc.summary_info.last_saved_by = "R5A4 user".into();
    let read = dwg(&doc);
    assert_eq!(read.header.current_dimstyle_name, "R5A4-DIM", "resolved from the header handle");
    assert_eq!(read.header.multiline_style, "R5A4-ML", "$CMLSTYLE resolved from the header handle");
    assert_eq!(read.header.last_saved_by, "R5A4 user", "the DXF $LASTSAVEDBY is the summary's last saved by");
    let back = dxf(&read);
    assert_eq!(back.header.current_dimstyle_name, read.header.current_dimstyle_name);
    assert_eq!(back.header.last_saved_by, read.header.last_saved_by);
    assert!(back.header.legacy.is_none() && read.header.legacy.is_none());
}

#[test]
fn only_a_pre_r13_dxf_header_keeps_the_legacy_view_state() {
    let text = "  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\nAC1032\r\n  9\r\n$UNITMODE\r\n 70\r\n     1\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
    let modern = DxfReader::from_reader(Cursor::new(text.as_bytes().to_vec())).unwrap().read().unwrap();
    assert!(modern.header.legacy.is_none());
    assert_eq!(modern.header.unit_mode, 1);
    let r12 = text.replace("AC1032", "AC1009");
    let old = DxfReader::from_reader(Cursor::new(r12.into_bytes())).unwrap().read().unwrap();
    assert_eq!(old.header.legacy.as_ref().map(|state| state.unit_mode), Some(1));
}

#[test]
fn a_preview_is_written_as_thumbnailimage_and_read_back_in_its_format() {
    use acadrust::document::{Preview, PreviewFormat};
    let dib: Vec<u8> = (0..300u32)
        .map(|i| if i < 4 { [0x28u8, 0, 0, 0][i as usize] } else { (i * 7) as u8 })
        .collect();
    let png: Vec<u8> = [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4, 5, 6, 7, 8].to_vec();
    for (format, data) in [(PreviewFormat::Bmp, dib), (PreviewFormat::Png, png)] {
        let mut doc = CadDocument::with_version(DxfVersion::AC1032);
        doc.preview = Some(Preview { format, data: data.clone() });
        let text = dxf_text(&doc);
        assert!(text.contains("THUMBNAILIMAGE"), "written for R2000 and later");
        assert_eq!(dxf(&doc).preview, Some(Preview { format, data: data.clone() }));
        assert_eq!(dxf_binary(&doc).preview, Some(Preview { format, data }));
    }
    let mut old = CadDocument::with_version(DxfVersion::AC1014);
    old.preview = Some(Preview { format: PreviewFormat::Bmp, data: vec![0x28, 0, 0, 0, 1, 2, 3] });
    assert!(!dxf_text(&old).contains("THUMBNAILIMAGE"), "an R14 DXF has no such section");
}

#[test]
fn entity_graphic_data_is_read_for_every_entity_even_where_group_92_has_another_meaning() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut line = Line::new();
    line.end = Vector3::new(1.0, 2.0, 0.0);
    line.common.graphic_data = Some((0..300u32).map(|i| (i % 251) as u8).collect());
    let handle = doc.add_entity(EntityType::Line(line.clone())).unwrap();
    let expected = line.common.graphic_data.clone();
    let back = dxf(&doc);
    let kept = back.get_entity(handle).expect("line").common().graphic_data.clone();
    assert_eq!(kept, expected);
    // A MESH writes group 92 for its vertex count after the AcDbEntity subclass: the graphics size before it
    // must not be taken for it, nor it for the size.
    let mut mesh = acadrust::entities::Mesh::new();
    mesh.vertices = vec![Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)];
    mesh.faces = vec![acadrust::entities::MeshFace::new(vec![0, 1, 2])];
    mesh.common.graphic_data = Some(vec![9, 8, 7, 6, 5, 4, 3, 2, 1]);
    let handle = doc.add_entity(EntityType::Mesh(mesh)).unwrap();
    let back = dxf(&doc);
    let EntityType::Mesh(read) = back.get_entity(handle).expect("mesh") else { panic!("mesh") };
    assert_eq!(read.common.graphic_data.as_deref(), Some(&[9u8, 8, 7, 6, 5, 4, 3, 2, 1][..]));
    assert_eq!(read.vertices.len(), 3);
}

#[test]
fn modern_table_style_cells_travel_in_a_cell_style_map_and_return_to_the_style() {
    use acadrust::objects::{DataObjectData, NamedTableCellStyle, TableStyle};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut style = TableStyle::new("R5A4-TS");
    style.handle = doc.allocate_handle();
    let mut cell = NamedTableCellStyle::default();
    cell.id = 1;
    cell.style_type = 1;
    cell.name = "_TITLE".into();
    style.modern_overrides = vec![(1, cell)];
    let handle = style.handle;
    doc.objects.insert(handle, ObjectType::TableStyle(style));
    doc.ensure_extension_dictionary(handle);
    let text = dxf_text(&doc);
    assert!(text.contains("CELLSTYLEMAP"), "the DXF writes the carrier object");
    let back = dxf(&doc);
    let Some(ObjectType::TableStyle(read)) = back.objects.get(&handle) else { panic!("style") };
    assert_eq!(read.modern_overrides.len(), 1, "restored into the style");
    assert!(!read.cell_style_map_handle.is_null(), "the carrier's handle is kept");
    assert!(
        !back.objects.values().any(|object| matches!(
            object,
            ObjectType::DataObject(data) if matches!(data.data, DataObjectData::CellStyleMap(_))
        )),
        "the carrier is taken out of the document"
    );
    // Written again, the object takes the same handle.
    let again = dxf(&back);
    let Some(ObjectType::TableStyle(second)) = again.objects.get(&handle) else { panic!("style") };
    assert_eq!(second.cell_style_map_handle, read.cell_style_map_handle);
}

#[test]
fn table_style_row_text_style_is_resolved_to_a_handle_when_dxf_names_it() {
    let doc = CadDocument::with_version(DxfVersion::AC1032);
    let standard = doc.text_styles.get("Standard").expect("Standard").handle;
    let back = dxf(&doc);
    let style = back
        .objects
        .values()
        .find_map(|object| match object {
            ObjectType::TableStyle(style) => Some(style),
            _ => None,
        })
        .expect("table style");
    assert_eq!(style.data_row_style.text_style_handle, Some(standard));
}

#[test]
fn viewport_override_layer_is_named_by_its_base_layer_with_the_record_handle() {
    use acadrust::tables::Layer;
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut shadow = Layer::new("A-WALL @ 8");
    shadow.handle = doc.allocate_handle();
    let shadow_handle = shadow.handle;
    doc.layers.add(shadow).expect("layer");
    let mut base = Layer::new("A-WALL");
    base.handle = doc.allocate_handle();
    doc.layers.add(base).expect("layer");
    let mut line = Line::new();
    line.common.layer = "A-WALL @ 8".into();
    let handle = doc.add_entity(EntityType::Line(line)).unwrap();
    let text = dxf_text(&doc);
    assert!(text.contains("\r\nA-WALL @ 8\r\n"));
    let back = dxf(&doc);
    let common = back.get_entity(handle).expect("line").common();
    assert_eq!(common.layer, "A-WALL");
    assert_eq!(common.layer_handle, Some(shadow_handle));
    assert!(dxf_text(&back).contains("\r\nA-WALL @ 8\r\n"), "written under the record's name again");
}

#[test]
fn a_dxf_block_with_a_public_name_is_not_anonymous_whatever_its_flag_word_says() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut record = acadrust::tables::BlockRecord::new("PUBLIC-BLOCK");
    record.handle = doc.allocate_handle();
    record.source_flags = 1;
    doc.block_records.add(record).expect("record");
    let text = dxf_text(&doc);
    let back = DxfReader::from_reader(Cursor::new(text.into_bytes())).unwrap().read().unwrap();
    let block = back.block_records.get("PUBLIC-BLOCK").expect("block");
    assert!(!block.flags.anonymous);
}

#[test]
fn acis_revision_guid_empty_bit_and_ds_data_survive_the_dxf() {
    use acadrust::entities::solid3d::AcisRevision;
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut solid = Solid3D::new();
    solid.acis_data.sab_data = b"ACIS BinaryFile".to_vec();
    solid.acis_data.is_binary = true;
    solid.acis_data.acis_empty_bit = true;
    solid.acis_data.revision = AcisRevision {
        has_guid: true,
        major: 0x1A11_3328,
        minor1: -5267,
        minor2: -11187,
        bytes: [0x82, 0x4D, 0x78, 0xB3, 0x36, 0x68, 0xF9, 0xE7],
        end_marker: 0,
    };
    solid.uid = solid.acis_data.revision.guid_string().expect("guid");
    assert_eq!(solid.uid, "{1a113328-eb6d-d44d-824d-78b33668f9e7}");
    let handle = doc.add_entity(EntityType::Solid3D(solid.clone())).unwrap();
    let back = dxf(&doc);
    let EntityType::Solid3D(read) = back.get_entity(handle).expect("solid") else { panic!("solid") };
    assert_eq!(read.uid, solid.uid);
    assert_eq!(read.acis_data.revision, solid.acis_data.revision);
    assert!(read.acis_data.acis_empty_bit);
    assert!(read.common.has_ds_data, "the geometry came from ACDSDATA");
    // Without a revision the writer's zero GUID reads back as none.
    let mut plain = Solid3D::new();
    plain.acis_data.sab_data = b"ACIS BinaryFile".to_vec();
    plain.acis_data.is_binary = true;
    let handle = doc.add_entity(EntityType::Solid3D(plain)).unwrap();
    let EntityType::Solid3D(read) = dxf(&doc).get_entity(handle).expect("solid").clone() else { panic!("solid") };
    assert!(read.uid.is_empty() && !read.acis_data.revision.has_guid);
}

#[test]
fn sun_true_color_is_written_in_group_421() {
    use acadrust::objects::{ClassObject, ClassObjectData, Sun};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut sun = Sun::default();
    sun.color = Color::Rgb { r: 200, g: 100, b: 50 };
    let mut object = ClassObject::new(ClassObjectData::Sun(sun));
    object.handle = doc.allocate_handle();
    let handle = object.handle;
    doc.objects.insert(handle, ObjectType::ClassObject(object));
    let back = dxf(&doc);
    let Some(ObjectType::ClassObject(read)) = back.objects.get(&handle) else { panic!("sun") };
    let ClassObjectData::Sun(read) = &read.data else { panic!("sun data") };
    assert_eq!(read.color, Color::Rgb { r: 200, g: 100, b: 50 });
}

#[test]
fn radius_dimension_base_definition_point_mirrors_the_chord_point_in_dxf_too() {
    use acadrust::entities::{Dimension, DimensionRadius};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let dim = DimensionRadius::new(Vector3::new(1.0, 2.0, 0.0), Vector3::new(4.0, 6.0, 0.0));
    let handle = doc.add_entity(EntityType::Dimension(Box::new(Dimension::Radius(dim)))).unwrap();
    let back = dxf(&doc);
    let EntityType::Dimension(read) = back.get_entity(handle).expect("dimension") else { panic!("dimension") };
    let Dimension::Radius(read) = read.as_ref() else { panic!("radius") };
    assert_eq!(read.definition_point, Vector3::new(4.0, 6.0, 0.0));
    assert_eq!(read.base.definition_point, read.definition_point);
}

#[test]
fn a_pre_r2004_dwg_keeps_the_defaults_of_the_fields_its_record_lacks() {
    let doc = CadDocument::with_version(DxfVersion::AC1015);
    let read = dwg(&doc);
    let mut layouts = 0;
    for object in read.objects.values() {
        if let ObjectType::Layout(layout) = object {
            layouts += 1;
            assert_eq!(layout.shade_plot_dpi, 300, "no shade-plot DPI before R2004");
        }
    }
    assert!(layouts > 0, "the document has layouts");
}

#[test]
fn the_dxf_vx_table_has_no_handle_after_r2000() {
    let doc = CadDocument::with_version(DxfVersion::AC1032);
    assert!(dxf(&doc).vx_table.handle().is_null());
}

#[test]
fn the_dxf_reader_takes_the_decoded_typeface_out_of_the_retained_xdata_as_the_dwg_reader_does() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut style = acadrust::tables::TextStyle::new("R5A4-FONT");
    style.handle = doc.allocate_handle();
    style.true_type_font = "Arial".into();
    style.true_type_font_flags = 34;
    doc.text_styles.add(style).expect("style");
    let back = dxf(&doc);
    let read = back.text_styles.get("R5A4-FONT").expect("style");
    assert_eq!((read.true_type_font.as_str(), read.true_type_font_flags), ("Arial", 34));
    assert!(
        back.object_xdata(read.handle).is_none_or(|xdata| xdata.get_record("ACAD").is_none()),
        "the typeface lives in the fields"
    );
    assert!(read.true_type_font_eed_position.is_some());
    // The record is written again from the fields.
    assert!(dxf_text(&back).contains("\r\n1001\r\nACAD\r\n"));
}

#[test]
fn a_group_is_named_by_its_dictionary_key() {
    use acadrust::objects::Group;
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let dictionary = doc.header.acad_group_dict_handle;
    let mut group = Group::new("");
    group.handle = doc.allocate_handle();
    group.owner = dictionary;
    let handle = group.handle;
    doc.objects.insert(handle, ObjectType::Group(group));
    if let Some(ObjectType::Dictionary(entries)) = doc.objects.get_mut(&dictionary) {
        entries.add_entry("R5A4-GROUP", handle);
    }
    for read in [dxf(&doc), dwg(&doc)] {
        let Some(ObjectType::Group(group)) = read.objects.get(&handle) else { panic!("group") };
        assert_eq!(group.name, "R5A4-GROUP");
    }
}

#[test]
fn a_pre_r2010_dwg_multileader_style_keeps_the_dxf_defaults_and_the_dxf_leaves_the_groups_out() {
    use acadrust::objects::TextAttachmentType;
    let doc = CadDocument::with_version(DxfVersion::AC1021);
    let read = dwg(&doc);
    let styles: Vec<_> = read
        .objects
        .values()
        .filter_map(|object| match object {
            ObjectType::MultiLeaderStyle(style) => Some(style),
            _ => None,
        })
        .collect();
    assert!(!styles.is_empty());
    for style in styles {
        assert_eq!(style.text_bottom_attachment, TextAttachmentType::BottomOfBottomLine);
        assert!(!style.name.is_empty(), "named as the DXF reader names it");
    }
    // Written as R2010 (as binary DXF of a pre-R2010 file is), the groups the DWG never stated stay out.
    let text = String::from_utf8_lossy(
        &DxfWriter::new(&read).with_version(DxfVersion::AC1024).write_to_vec().expect("write"),
    )
    .into_owned();
    let start = text.find("\r\nMLEADERSTYLE\r\n").expect("style record");
    let end = text[start + 2..].find("\r\n  0\r\n").map_or(text.len(), |end| start + 2 + end);
    let record = &text[start..end];
    assert!(!record.contains("\r\n272\r\n") && !record.contains("\r\n273\r\n"), "{record}");
}

#[test]
fn a_pre_r2004_layout_lists_the_viewports_of_its_block_as_the_dxf_reader_does() {
    use acadrust::entities::Viewport;
    let mut doc = CadDocument::with_version(DxfVersion::AC1015);
    let mut viewport = Viewport::new();
    viewport.id = 2;
    doc.add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "Layout1").expect("viewport");
    let from_dwg = dwg(&doc);
    let from_dxf = dxf(&doc);
    for read in [&from_dwg, &from_dxf] {
        let layout = read
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::Layout(layout) if layout.name == "Layout1" => Some(layout),
                _ => None,
            })
            .expect("layout");
        assert!(!layout.viewports.is_empty(), "the viewport entity is listed");
    }
    let listed = |read: &CadDocument| {
        read.objects
            .values()
            .find_map(|object| match object {
                ObjectType::Layout(layout) if layout.name == "Layout1" => Some(layout.viewports.clone()),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(listed(&from_dwg), listed(&from_dxf));
}

#[test]
fn an_attribute_embedded_text_without_rotation_states_no_direction_in_the_dwg_read_either() {
    use acadrust::entities::{AttributeDefinition, MText};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut attdef = AttributeDefinition::new("TAG".into(), "Prompt".into(), "value".into());
    attdef.is_multiline = true;
    let mut text = MText::new();
    text.value = "a\\Pb".into();
    attdef.embedded_mtext = Some(Box::new(text));
    let handle = doc.add_entity(EntityType::AttributeDefinition(attdef)).unwrap();
    let from_dwg = dwg(&doc);
    let from_dxf = dxf(&from_dwg);
    for read in [&from_dwg, &from_dxf] {
        let EntityType::AttributeDefinition(attdef) = read.get_entity(handle).expect("attdef") else { panic!() };
        let embedded = attdef.embedded_mtext.as_ref().expect("embedded text");
        assert_eq!(embedded.dwg_x_direction, None);
    }
}

#[test]
fn a_dwg_table_names_its_block_and_the_dxf_carrier_of_a_table_style_takes_a_free_handle() {
    use acadrust::entities::Table;
    use acadrust::objects::{NamedTableCellStyle, TableStyle};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut block = acadrust::tables::BlockRecord::new("*T1");
    block.handle = doc.allocate_handle();
    let block_handle = block.handle;
    doc.block_records.add(block).expect("block");
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    table.block_record_handle = Some(block_handle);
    let table_handle = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let mut style = TableStyle::new("R5A4-TS");
    style.handle = doc.allocate_handle();
    let mut cell = NamedTableCellStyle::default();
    cell.id = 1;
    cell.name = "_TITLE".into();
    style.modern_overrides = vec![(1, cell)];
    let style_handle = style.handle;
    // Handles nothing uses (erased objects leave them): one for the carrier of this style, one for that of the
    // document's Standard table style.
    let gaps = [doc.allocate_handle(), doc.allocate_handle()];
    doc.objects.insert(style_handle, ObjectType::TableStyle(style));
    doc.ensure_extension_dictionary(style_handle);
    let read = dwg(&doc);
    let EntityType::Table(table) = read.get_entity(table_handle).expect("table") else { panic!("table") };
    let record_name = read
        .block_records
        .iter()
        .find(|record| record.handle == block_handle)
        .map(|record| record.name.clone())
        .expect("the block record");
    assert!(!record_name.is_empty());
    assert_eq!(table.block_name, record_name, "named by the block record it references");
    let Some(ObjectType::TableStyle(style)) = read.objects.get(&style_handle) else { panic!("style") };
    assert!(style.cell_style_map_handle.is_null(), "an R2010+ DWG has no CELLSTYLEMAP");
    for gap in gaps {
        assert!(!read.objects.contains_key(&gap) && read.get_entity(gap).is_none(), "the gap is free");
    }
    // The DXF writes each carrier in a free handle after its style (not above the seed); the DXF read gives
    // the cell styles back to the style and takes the carrier out.
    let back = dxf(&read);
    let Some(ObjectType::TableStyle(again)) = back.objects.get(&style_handle) else { panic!("style") };
    assert_eq!(again.cell_style_map_handle, gaps[1]);
    assert_eq!(again.modern_overrides.len(), 1);
    assert!(gaps.iter().all(|gap| !back.objects.contains_key(gap)));
}

#[test]
fn a_dwg_cell_style_without_a_background_colour_has_no_fill() {
    use acadrust::entities::{CellStyle, Table};
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::ZERO, 1, 2);
    for (column, colour) in [(0usize, Color::None), (1usize, Color::Index(1))] {
        let mut style = CellStyle::new();
        style.background_color = colour;
        style.override_flags = 1;
        table.cell_mut(0, column).expect("cell").style = Some(style);
    }
    let handle = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let read = dwg(&doc);
    let EntityType::Table(table) = read.get_entity(handle).expect("table") else { panic!("table") };
    let fill = |column: usize| table.rows[0].cells[column].style.as_ref().map(|style| style.fill_enabled);
    assert_eq!(fill(0), Some(false), "no colour, no fill");
    assert_eq!(fill(1), Some(true));
}
