// SPDX-License-Identifier: MPL-2.0
//! R5b-3 (gate G5): defects found by the native round trips of DXF sources through a DWG, one test per fix. Each test builds
//! a synthetic document; no external file is used. Each test fails with its fix reverted.

use std::io::Cursor;

use acadrust::entities::{EntityType, MText};
use acadrust::objects::ObjectType;
use acadrust::types::{DxfVersion, Vector3};
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

fn mtext_directions(document: &CadDocument) -> Vec<Option<Vector3>> {
    document
        .entities()
        .filter_map(|e| match e {
            EntityType::MText(m) => Some(m.dwg_x_direction),
            _ => None,
        })
        .collect()
}

/// A DXF MTEXT states its X axis (group 11) only for a rotated text, and a DWG always states one. The default axis is no
/// stated direction in either reader, so the native model of the DXF and of its DWG are the same; a rotated text keeps
/// its direction.
#[test]
fn the_default_x_axis_of_an_mtext_is_no_stated_direction() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut plain = MText::new();
        plain.value = "plain".into();
        document.add_entity(EntityType::MText(Box::new(plain))).unwrap();
        let mut turned = MText::new();
        turned.value = "turned".into();
        turned.rotation = 0.5;
        document.add_entity(EntityType::MText(Box::new(turned))).unwrap();
        let source = dxf_source(&document);
        let read = write_and_read(&source, version);
        let expected = mtext_directions(&source);
        assert_eq!(expected.len(), 2);
        assert_eq!(expected[0], None, "{version:?}: the DXF states no direction for the default axis");
        assert!(expected[1].is_some(), "{version:?}: a rotated text states one");
        assert_eq!(mtext_directions(&read), expected, "{version:?}: the DWG read");
    }
}

/// The default X axis of a text is that of its plane, (1, 0, 0) only for the normal (0, 0, 1): an MTEXT on another plane
/// whose DWG states (1, 0, 0) keeps the direction, so a DXF written from it states group 11 (without it the direction
/// would be the plane's own X axis).
#[test]
fn an_mtext_on_another_plane_keeps_its_stated_x_axis() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let mut text = MText::new();
        text.value = "below".into();
        text.normal = Vector3::new(0.0, 0.0, -1.0);
        text.dwg_x_direction = Some(Vector3::new(1.0, 0.0, 0.0));
        document.add_entity(EntityType::MText(Box::new(text))).unwrap();
        let read = write_and_read(&document, version);
        assert_eq!(mtext_directions(&read), vec![Some(Vector3::new(1.0, 0.0, 0.0))], "{version:?}: the DWG read");
        let dxf = String::from_utf8(DxfWriter::new(&read).write_to_vec().unwrap()).unwrap();
        let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
        let start = (0..lines.len() - 1).find(|&i| lines[i] == "0" && lines[i + 1] == "MTEXT").expect("an MTEXT record");
        let mut codes = Vec::new();
        let mut at = start + 2;
        while at + 1 < lines.len() && lines[at] != "0" {
            codes.push(lines[at]);
            at += 2;
        }
        assert!(codes.contains(&"11"), "{version:?}: the DXF states group 11: {codes:?}");
    }
}

/// `$CMLSTYLE` names the multiline style by its key in the ACAD_MLINESTYLE dictionary, which can differ in case from the
/// style's own name (AutoCAD's DXF keys it "Standard" and names the style "STANDARD"). A DWG states a handle, and the name
/// read from it is the key.
#[test]
fn the_current_multiline_style_is_named_by_its_dictionary_key() {
    for version in R2000_PLUS {
        let mut document = CadDocument::with_version(version);
        let handle = document.header.current_multiline_style_handle;
        assert!(!handle.is_null(), "{version:?}: the template has a current multiline style");
        match document.objects.get_mut(&handle) {
            Some(ObjectType::MLineStyle(style)) => style.name = "STANDARD".into(),
            other => panic!("{version:?}: no MLineStyle at the current handle: {other:?}"),
        }
        document.header.multiline_style = "Standard".into();
        let read = write_and_read(&document, version);
        assert_eq!(read.header.multiline_style, "Standard", "{version:?}");
    }
}

fn viewport_ids(document: &CadDocument) -> Vec<i16> {
    document
        .entities()
        .filter_map(|e| match e {
            EntityType::Viewport(v) => Some(v.id),
            _ => None,
        })
        .collect()
}

/// An R14 DWG hangs the viewport entity headers (VX table records) below a control object. A DXF has no VX table, and the
/// writer made the records without the control (the table had no handle), so the file lost the list that numbers the
/// viewports: the document read from it differed from the one read from a second write of itself.
#[test]
fn the_vx_records_a_dxf_source_gets_in_r14_hang_below_their_control() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document
        .add_entity_to_layout(EntityType::Viewport(Box::default()), "Layout1")
        .expect("a viewport on the first layout");
    document
        .add_entity_to_layout(EntityType::Viewport(Box::default()), "Layout1")
        .expect("a second viewport");
    let source = dxf_source(&document);
    assert!(source.vx_table.handle().is_null(), "a DXF has no VX table");
    let first = write_and_read(&source, DxfVersion::AC1014);
    assert!(!first.vx_control_entries.is_empty(), "the control object lists the records");
    let second = write_and_read(&first, DxfVersion::AC1014);
    assert_eq!(second.vx_control_entries.len(), first.vx_control_entries.len());
    assert_eq!(viewport_ids(&second), viewport_ids(&first), "the second write numbers the viewports as the first did");
}

/// The extended data of a block record of an R14 file keeps the order of its applications through a second write: the
/// DesignCenter units block (application ACAD, which the writer states again from the record) stays where the file had it.
/// The record AutoCAD's R14 file states for the preview image (`CONTENTBLOCKICON 1071 0 1004 ..`, R5b-7) is the preview of the block
/// record once read; a record of another shape is data of the file and stays in the model in its place.
#[test]
fn a_block_record_keeps_the_order_of_its_xdata_applications_in_r14() {
    use acadrust::tables::{AppId, BlockRecord};
    use acadrust::xdata::{ExtendedDataRecord, XDataValue};
    for (restored, first_value) in [(true, 0), (false, 1)] {
        let mut document = CadDocument::with_version(DxfVersion::AC1014);
        if document.app_ids.get("CONTENTBLOCKICON").is_none() {
            let mut app = AppId::new("CONTENTBLOCKICON");
            app.handle = document.allocate_handle();
            document.app_ids.add(app).unwrap();
        }
        let mut record = BlockRecord::new("door");
        record.handle = document.allocate_handle();
        record.units = 6;
        let handle = record.handle;
        document.block_records.add(record).unwrap();
        let mut units = ExtendedDataRecord::new("ACAD");
        units.add_value(XDataValue::String("DesignCenter Data".into()));
        units.add_value(XDataValue::ControlString("{".into()));
        units.add_value(XDataValue::Integer16(1));
        units.add_value(XDataValue::Integer16(6));
        units.add_value(XDataValue::ControlString("}".into()));
        document.set_object_xdata_record(handle, units);
        let mut icon = ExtendedDataRecord::new("CONTENTBLOCKICON");
        icon.add_value(XDataValue::Integer32(first_value));
        icon.add_value(XDataValue::BinaryData(vec![1, 2, 3, 4]));
        document.set_object_xdata_record(handle, icon);
        let order = |document: &CadDocument| -> Vec<String> {
            let handle = document.block_records.get("door").expect("block record").handle;
            document
                .object_xdata(handle)
                .expect("xdata")
                .records()
                .iter()
                .map(|record| record.application_name.clone())
                .collect()
        };
        let preview = |document: &CadDocument| document.block_records.get("door").expect("block record").preview_data.clone();
        let first = write_and_read(&document, DxfVersion::AC1014);
        let second = write_and_read(&first, DxfVersion::AC1014);
        if restored {
            // the record is the preview once read, stated again in its place
            for (name, read) in [("written once", &first), ("written again", &second)] {
                assert_eq!(order(read), ["ACAD"], "{name}");
                assert_eq!(preview(read), [1, 2, 3, 4], "{name}");
            }
        } else {
            assert_eq!(order(&first), ["ACAD", "CONTENTBLOCKICON"], "written once");
            assert_eq!(order(&second), ["ACAD", "CONTENTBLOCKICON"], "written again");
            assert!(preview(&second).is_empty());
        }
    }
}

fn hard_owner_flags(document: &CadDocument) -> Vec<(String, bool)> {
    let mut flags: Vec<(String, bool)> = document
        .objects
        .values()
        .filter_map(|object| match object {
            ObjectType::Dictionary(dictionary) => Some((
                dictionary.entries.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>().join(","),
                dictionary.hard_owner,
            )),
            _ => None,
        })
        .collect();
    flags.sort();
    flags
}

/// The byte an R14 DICTIONARY has where R2000 has the hard-owner flag is the flag: AutoCAD's own R14 files state 1 in the
/// dictionaries that hold hard-owned entries, and the reader reads it as such, so the writer states it again.
#[test]
fn an_r14_dictionary_keeps_its_hard_owner_flag() {
    let mut document = CadDocument::with_version(DxfVersion::AC1014);
    let mut dictionary = acadrust::objects::Dictionary::new();
    dictionary.handle = document.allocate_handle();
    dictionary.owner = document.header.named_objects_dict_handle;
    dictionary.hard_owner = true;
    let handle = dictionary.handle;
    document.objects.insert(handle, ObjectType::Dictionary(dictionary));
    if let Some(ObjectType::Dictionary(root)) = document.objects.get_mut(&document.header.named_objects_dict_handle) {
        root.add_entry("R5B3_HARD", handle);
    }
    let first = write_and_read(&document, DxfVersion::AC1014);
    let hard = hard_owner_flags(&first);
    assert!(hard.iter().any(|(_, flag)| *flag), "the flag is written: {hard:?}");
    let second = write_and_read(&first, DxfVersion::AC1014);
    assert_eq!(hard_owner_flags(&second), hard, "a second write keeps every flag");
}

fn viewport_owner(document: &CadDocument, viewport: acadrust::types::Handle) -> acadrust::types::Handle {
    document
        .entities()
        .find_map(|e| match e {
            EntityType::Viewport(v) if v.common.handle == viewport => Some(v.common.owner_handle),
            _ => None,
        })
        .expect("the viewport of a record")
}

/// AutoCAD's R14 files hold VX records for the viewports of the active paper space only (sample_AC1014.dwg has none for the
/// viewports of the other layouts' blocks): a drawing that has its own VX table keeps it as it is there (the writer made a
/// record for every viewport the table lacked, which grew the handle seed and renumbered the viewports), and only a viewport
/// of the active paper space that lacks a record gets one.
#[test]
fn a_partial_vx_table_of_an_r14_drawing_is_filled_for_the_active_paper_space_only() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.add_layout("Second").expect("a second layout");
    for layout in ["Layout1", "Layout1", "Second"] {
        document
            .add_entity_to_layout(EntityType::Viewport(Box::default()), layout)
            .expect("a viewport");
    }
    let first = write_and_read(&dxf_source(&document), DxfVersion::AC1014);
    let paper_space = first.header.paper_space_block_handle;
    let records: Vec<_> = first.vx_table.iter().cloned().collect();
    assert!(records.len() >= 4, "a reserved record and one per viewport: {}", records.len());
    let of_viewports: Vec<_> = records.iter().filter(|record| !record.viewport.is_null()).collect();
    let other = of_viewports
        .iter()
        .find(|record| viewport_owner(&first, record.viewport) != paper_space)
        .expect("a record of the viewport of the second layout");
    let active = of_viewports
        .iter()
        .find(|record| viewport_owner(&first, record.viewport) == paper_space)
        .expect("a record of a viewport of the active paper space");
    let without = |dropped: acadrust::types::Handle| {
        let mut partial = first.clone();
        partial.vx_table.clear();
        for record in records.iter().filter(|record| record.handle != dropped) {
            partial.vx_table.add_allow_duplicate(record.clone());
        }
        assert_eq!(partial.vx_table.len(), records.len() - 1);
        partial
    };
    let again = write_and_read(&without(other.handle), DxfVersion::AC1014);
    assert_eq!(again.vx_table.len(), records.len() - 1, "the records the drawing had");
    assert!(!again.vx_table.iter().any(|record| record.viewport == other.viewport), "no record for the other layout");
    let again = write_and_read(&without(active.handle), DxfVersion::AC1014);
    assert_eq!(again.vx_table.len(), records.len(), "the viewport of the active paper space gets a record again");
    assert!(again.vx_table.iter().any(|record| record.viewport == active.viewport));
}

/// A block record that never stated its scaling (a DWG before R2007 has no such field) is written with any scaling (byte 0,
/// AutoCAD's kAny) in a later version: AutoCAD states 0 for 23 of the 27 records of its own sample_AC1024.dwg and
/// sample_AC1032.dwg (1 for the 4 the drawing made uniform-only), so a record without the field gains no restriction. A record
/// that states uniform-only scaling keeps it.
#[test]
fn a_block_that_never_stated_its_scaling_keeps_any_scaling_in_a_later_version() {
    use acadrust::tables::BlockRecord;
    let mut document = CadDocument::with_version(DxfVersion::AC1014);
    let mut record = BlockRecord::new("door");
    record.handle = document.allocate_handle();
    document.block_records.add(record).unwrap();
    let r14 = write_and_read(&document, DxfVersion::AC1014);
    assert!(!r14.block_records.get("door").unwrap().scale_uniformly, "a record without the field: nothing stated");
    for version in [DxfVersion::AC1024, DxfVersion::AC1032] {
        let later = write_and_read(&r14, version);
        assert!(!later.block_records.get("door").unwrap().scale_uniformly, "{version:?}: the byte written is 0");
    }
    // A record of R2007 or later states the byte itself, and a second conversion keeps it.
    let mut stated = write_and_read(&r14, DxfVersion::AC1032);
    stated.block_records.get_mut("door").unwrap().scale_uniformly = true;
    let again = write_and_read(&stated, DxfVersion::AC1032);
    assert!(again.block_records.get("door").unwrap().scale_uniformly, "a stated 1 stays");
}
