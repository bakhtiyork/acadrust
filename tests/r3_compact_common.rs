// SPDX-License-Identifier: MPL-2.0
//! Focused regressions for compact, copy-on-write entity appearance data.
//!
//! These tests deliberately use one synthetic heavy polyline instead of the
//! full identity matrix in `r3_identity_roundtrip.rs`.  The large child list
//! is enough to exercise both the serial and Rayon DWG entity passes.

use std::io::Cursor;
use std::mem::size_of;
use std::sync::Arc;

use acadrust::entities::{EntityCommon, EntityType, Polyline2D, Vertex2D};
use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use acadrust::{CadDocument, Color, DwgReader, DwgWriter, DxfVersion, Handle, Vector3};
use rayon::ThreadPoolBuilder;

const POLYLINE: Handle = Handle::new(0x2100);
const FIRST_VERTEX: Handle = Handle::new(0x2200);
const EXCEPTION_VERTEX: Handle = Handle::new(0x2201);
const REACTOR: Handle = Handle::new(0x2e01);
const FULL_VISUAL_STYLE: Handle = Handle::new(0x2e02);
const CHILD_COUNT: usize = 1_100;

fn tagged_xdata(value: &str) -> ExtendedData {
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("ACAD");
    record.add_value(XDataValue::String(value.to_owned()));
    data.add_record(record);
    data
}

fn synthetic_polyline(with_exception: bool) -> CadDocument {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut polyline = Polyline2D::new();
    polyline.common.handle = POLYLINE;

    for index in 0..CHILD_COUNT {
        let mut vertex = Vertex2D::new(Vector3::new(index as f64, 0.0, 0.0));
        vertex.common.handle = if index == 0 {
            FIRST_VERTEX
        } else if index == 1 {
            EXCEPTION_VERTEX
        } else {
            Handle::new(FIRST_VERTEX.value() + index as u64)
        };
        vertex.common.owner_handle = POLYLINE;

        if with_exception && index == 1 {
            // Keep one child outside the empty/default appearance bucket.  A
            // real DWG child can carry all of these common fields, and the
            // reader must retain them without poisoning the shared bucket.
            vertex.common.color = Color::Index(7);
            vertex.common.extended_data = tagged_xdata("exception");
            vertex.common.reactors = vec![REACTOR];
            // Graphic bytes are a rare common field which the DWG preamble
            // writes verbatim, so this also checks that the exceptional
            // property bucket survives a write/read cycle.
            vertex.common.graphic_data = Some(vec![0xa5, 0x5a]);
        }

        polyline.vertices.push(vertex);
    }

    document
        .add_entity(EntityType::Polyline2D(polyline))
        .expect("synthetic polyline should be accepted");
    document
}

fn read_serial(bytes: &[u8]) -> CadDocument {
    ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("serial Rayon pool should build")
        .install(|| {
            DwgReader::from_stream(Cursor::new(bytes.to_vec()))
                .read()
                .expect("synthetic DWG should read")
        })
}

fn read_rayon(bytes: &[u8]) -> CadDocument {
    ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("Rayon pool should build")
        .install(|| {
            DwgReader::from_stream(Cursor::new(bytes.to_vec()))
                .read()
                .expect("synthetic DWG should read")
        })
}

fn assert_child_data(document: &CadDocument) {
    let EntityType::Polyline2D(polyline) = document
        .get_entity(POLYLINE)
        .expect("polyline should survive the DWG round trip")
    else {
        panic!("synthetic entity changed type");
    };

    assert_eq!(polyline.vertices.len(), CHILD_COUNT);
    assert_eq!(polyline.common.handle, POLYLINE);
    assert_eq!(polyline.vertices[0].common.handle, FIRST_VERTEX);
    assert_eq!(polyline.vertices[0].common.owner_handle, POLYLINE);
    let first_raw = polyline.vertices[0]
        .common
        .raw_record
        .as_ref()
        .expect("DWG child should retain its raw record");
    let second_raw = polyline.vertices[2]
        .common
        .raw_record
        .as_ref()
        .expect("DWG child should retain its raw record");
    assert!(
        !Arc::ptr_eq(first_raw, second_raw),
        "raw child records must stay distinct from shared appearance"
    );

    // Every ordinary child has the same empty/default appearance.  Identity
    // fields remain inline, so adjacent equal appearance data can be shared
    // safely.  The reader intentionally bounds its cache to one previous
    // appearance per 512-record chunk, so chunk boundaries need not share
    // with the first child.
    let shared = &polyline.vertices[0].common.properties;
    let mut shared_pairs = 0usize;
    for (index, vertex) in polyline.vertices.iter().enumerate() {
        let expected_handle = if index == 0 {
            FIRST_VERTEX
        } else if index == 1 {
            EXCEPTION_VERTEX
        } else {
            Handle::new(FIRST_VERTEX.value() + index as u64)
        };
        assert_eq!(vertex.common.handle, expected_handle);
        assert_eq!(vertex.common.owner_handle, POLYLINE);
        if index > 0 && index != 1 {
            let previous = &polyline.vertices[index - 1].common;
            if Arc::ptr_eq(&previous.properties, &vertex.common.properties) {
                shared_pairs += 1;
            }
        }
    }
    assert!(
        shared_pairs > CHILD_COUNT - 20,
        "only {shared_pairs} ordinary adjacent child pairs shared appearance"
    );

    let exception = &polyline.vertices[1].common;
    assert!(!Arc::ptr_eq(shared, &exception.properties));
    assert_eq!(exception.color, Color::Index(7));
    assert_eq!(exception.reactors, [REACTOR]);
    assert_eq!(exception.graphic_data.as_deref(), Some(&[0xa5, 0x5a][..]));
    assert_eq!(
        exception
            .extended_data
            .get_record("ACAD")
            .and_then(|record| record.values.first()),
        Some(&XDataValue::String("exception".to_owned()))
    );
}

#[test]
fn entity_common_defaults_share_and_mutation_is_copy_on_write() {
    let parent = EntityCommon::new();
    let sibling = parent.clone();
    let mut child = parent.clone();

    assert!(Arc::ptr_eq(&parent.properties, &sibling.properties));
    assert!(Arc::ptr_eq(&parent.properties, &child.properties));

    child.layer = "child-layer".to_owned();
    child.extended_data = tagged_xdata("child-xdata");
    child.reactors.push(REACTOR);
    child.full_visual_style_handle = Some(FULL_VISUAL_STYLE);

    assert!(!Arc::ptr_eq(&parent.properties, &child.properties));
    assert!(Arc::ptr_eq(&parent.properties, &sibling.properties));
    assert_eq!(parent.layer, "0");
    assert!(parent.extended_data.records().is_empty());
    assert!(parent.reactors.is_empty());
    assert_eq!(parent.full_visual_style_handle, None);
    assert_eq!(sibling.layer, "0");
    assert!(sibling.extended_data.records().is_empty());
    assert!(sibling.reactors.is_empty());
    assert_eq!(sibling.full_visual_style_handle, None);
}

#[test]
fn entity_common_defaults_use_one_shared_appearance_and_stay_compact() {
    let first = EntityCommon::new();
    let second = EntityCommon::default();
    assert!(Arc::ptr_eq(&first.properties, &second.properties));

    assert!(
        size_of::<EntityCommon>() <= 32,
        "EntityCommon grew to {} bytes",
        size_of::<EntityCommon>()
    );
    assert!(
        size_of::<Vertex2D>() <= 128,
        "Vertex2D grew to {} bytes",
        size_of::<Vertex2D>()
    );
}

#[cfg(feature = "serde")]
#[test]
fn compact_common_preserves_flat_serde_shape() {
    let mut source = EntityCommon::with_layer("custom");
    source.handle = FIRST_VERTEX;
    source.owner_handle = POLYLINE;
    source.extended_data = tagged_xdata("serialized");
    source.reactors.push(REACTOR);
    let value = serde_json::to_value(&source).unwrap();
    assert!(value.get("properties").is_none());
    assert_eq!(value["layer"], "custom");
    assert!(value.get("handle").is_some());
    assert!(value.get("owner_handle").is_some());
    let mut restored: EntityCommon = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored, source);
    assert_eq!(serde_json::to_value(&restored).unwrap(), value);
    restored.reactors.clear();
    assert_eq!(source.reactors, [REACTOR]);
}

#[test]
fn serial_and_rayon_dwg_reads_intern_default_appearance_and_preserve_exceptions() {
    let source = synthetic_polyline(true);
    let dwg = DwgWriter::write_to_vec(&source).expect("synthetic document should write");

    let serial = read_serial(&dwg);
    assert_child_data(&serial);
    let serial_rewritten =
        DwgWriter::write_to_vec(&serial).expect("serial-read document should write");
    let serial_reread = read_serial(&serial_rewritten);
    assert_child_data(&serial_reread);

    let rayon = read_rayon(&dwg);
    assert_child_data(&rayon);
    let rayon_rewritten =
        DwgWriter::write_to_vec(&rayon).expect("Rayon-read document should write");
    let rayon_reread = read_rayon(&rayon_rewritten);
    assert_child_data(&rayon_reread);
}

