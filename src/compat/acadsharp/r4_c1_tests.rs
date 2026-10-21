// SPDX-License-Identifier: MPL-2.0
//! Focused tests for the R4-C1 corpus fixes: LWPOLYLINE flags, the arc
//! dimension name, the two-line angular measurement, legacy code-page
//! replacement characters, and block membership as the block lists it.
use super::{dump_document, dump_reader_document};
use crate::entities::dimension::{Dimension, DimensionAngular2Ln, DimensionArc};
use crate::entities::{EntityType, Line, LwPolyline, Text};
use crate::tables::BlockRecord;
use crate::types::{DxfVersion, Handle, Vector2, Vector3};
use crate::CadDocument;
use serde_json::Value;

fn find<'a>(dump: &'a Value, h: u64) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{h:X}"))
        .unwrap_or_else(|| panic!("missing object #{h:X}"))
}

fn has(dump: &Value, h: u64) -> bool {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["handle"] == format!("#{h:X}"))
}

#[test]
fn lwpolyline_flags_carry_closed_and_plinegen() {
    let mut doc = CadDocument::new();
    let mut open = LwPolyline::from_points(vec![Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0)]);
    open.plinegen = true;
    let a = doc.add_entity(EntityType::LwPolyline(open)).unwrap();
    let mut closed = LwPolyline::from_points(vec![Vector2::new(0.0, 0.0), Vector2::new(1.0, 0.0)]);
    closed.is_closed = true;
    closed.plinegen = true;
    let b = doc.add_entity(EntityType::LwPolyline(closed)).unwrap();
    let c = doc
        .add_entity(EntityType::LwPolyline(LwPolyline::new()))
        .unwrap();
    let dump = dump_document(&doc, "", "new");
    assert_eq!(find(&dump, a.value())["props"]["Flags"], 128);
    assert_eq!(find(&dump, b.value())["props"]["Flags"], 129);
    assert_eq!(find(&dump, c.value())["props"]["Flags"], 0);
}

#[test]
fn arc_dimension_reports_its_own_object_name() {
    let mut doc = CadDocument::new();
    let h = doc
        .add_entity(EntityType::Dimension(Box::new(Dimension::Arc(
            DimensionArc::default(),
        ))))
        .unwrap();
    let dump = dump_document(&doc, "", "new");
    assert_eq!(find(&dump, h.value())["props"]["ObjectName"], "ARC_DIMENSION");
}

fn angular(arc: Vector3) -> f64 {
    // v1 = second - first = (10,0,0), v2 = definition - vertex = (10,10,0):
    // the lines meet at the origin and the plain angle is 45 degrees.
    let mut dim = DimensionAngular2Ln::new(
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
        Vector3::new(10.0, 10.0, 0.0),
    );
    dim.dimension_arc = arc;
    let mut doc = CadDocument::new();
    let h = doc
        .add_entity(EntityType::Dimension(Box::new(Dimension::Angular2Ln(dim))))
        .unwrap();
    let dump = dump_document(&doc, "", "new");
    find(&dump, h.value())["props"]["Measurement"]
        .as_f64()
        .expect("numeric measurement")
}

#[test]
fn two_line_angular_measurement_follows_the_reference_sector_rule() {
    use std::f64::consts::{FRAC_PI_4, PI};
    // Arc point between the directions: both cross products agree in sign.
    assert!((angular(Vector3::new(8.0, 3.0, 0.0)) - FRAC_PI_4).abs() < 1e-12);
    // Arc point outside that sector: the signs differ and the reference
    // reports the supplement of the angle, not the angle.
    assert!((angular(Vector3::new(-5.0, 5.0, 0.0)) - (PI - FRAC_PI_4)).abs() < 1e-12);
}

#[test]
fn legacy_code_page_replacement_character_is_a_question_mark_in_the_view() {
    let mut text = Text::with_value("\u{FFFD}8@150", Vector3::ZERO);
    text.source = None;
    let mut dwg = CadDocument::with_version(DxfVersion::AC1015);
    dwg.dwg_source_version = Some(DxfVersion::AC1015);
    let h = dwg.add_entity(EntityType::Text(text.clone())).unwrap();
    let dump = dump_document(&dwg, "", "dwg");
    // The invalid lead byte swallows the ASCII byte that follows it ("8").
    assert_eq!(find(&dump, h.value())["props"]["Value"], "?@150");

    // A replacement before a non-ASCII character or at the end swallows nothing.
    let mut tail = CadDocument::with_version(DxfVersion::AC1015);
    tail.dwg_source_version = Some(DxfVersion::AC1015);
    let h = tail
        .add_entity(EntityType::Text(Text::with_value("A\u{FFFD}\u{FFFD}\u{4E2D}\u{FFFD}", Vector3::ZERO)))
        .unwrap();
    let dump = dump_document(&tail, "", "dwg");
    assert_eq!(find(&dump, h.value())["props"]["Value"], "A??\u{4E2D}?");

    // R2007+ strings are UTF-16: nothing was replaced by a code-page decoder.
    let mut modern = CadDocument::with_version(DxfVersion::AC1021);
    modern.dwg_source_version = Some(DxfVersion::AC1021);
    let h = modern.add_entity(EntityType::Text(text)).unwrap();
    let dump = dump_document(&modern, "", "dwg");
    assert_eq!(find(&dump, h.value())["props"]["Value"], "\u{FFFD}8@150");
}

/// A block record whose source list names only the first of two entities that
/// both name it as owner.
fn block_with_unlisted_entity() -> (CadDocument, u64, u64, u64) {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    doc.header.source_handle_seed = Some(0x1000);
    let mut record = BlockRecord::new("B1");
    record.handle = Handle::new(0x300);
    let block = record.handle;
    doc.block_records.add(record).unwrap();
    let mut listed = Line::default();
    listed.common.handle = Handle::new(0x301);
    listed.common.owner_handle = block;
    let mut stale = Line::default();
    stale.common.handle = Handle::new(0x302);
    stale.common.owner_handle = block;
    doc.add_entity(EntityType::Line(listed)).unwrap();
    doc.add_entity(EntityType::Line(stale)).unwrap();
    let record = doc.block_records.get_mut("B1").unwrap();
    record.entity_handles = vec![Handle::new(0x301), Handle::new(0x302)];
    doc.block_entity_handles
        .insert(block, vec![Handle::new(0x301)]);
    (doc, 0x300, 0x301, 0x302)
}

#[test]
fn block_entities_are_the_handles_the_block_lists() {
    let (doc, block, listed, stale) = block_with_unlisted_entity();
    let dump = dump_reader_document(&doc, "", "dwg");
    assert_eq!(
        find(&dump, block)["props"]["Entities"],
        serde_json::json!([format!("#{listed:X}")])
    );
    assert!(has(&dump, listed));
    assert!(!has(&dump, stale), "an unlisted entity is not registered");
}

#[test]
fn documents_without_a_source_list_keep_every_owned_entity() {
    let (mut doc, block, listed, stale) = block_with_unlisted_entity();
    doc.block_entity_handles.clear();
    let dump = dump_reader_document(&doc, "", "dxf");
    assert_eq!(
        find(&dump, block)["props"]["Entities"],
        serde_json::json!([format!("#{listed:X}"), format!("#{stale:X}")])
    );
}
