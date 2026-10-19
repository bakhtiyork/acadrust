#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_reader_document as dump_document;
use acadrust::entities::{EntityCommon, EntityType, Polyline2D, Seqend, Vertex2D};
use acadrust::types::{DxfVersion, Handle};
use acadrust::CadDocument;

fn source_polyline(
    entity_handle: u64,
    seqend_handle: u64,
    vertex_handle: Option<u64>,
) -> EntityType {
    let mut polyline = Polyline2D::new();
    polyline.common = EntityCommon {
        handle: Handle::new(entity_handle),
        ..EntityCommon::default()
    };
    polyline.seqend = Some(Box::new(Seqend {
        common: EntityCommon {
            handle: Handle::new(seqend_handle),
            owner_handle: Handle::new(entity_handle),
            ..EntityCommon::default()
        },
    }));
    if let Some(vertex_handle) = vertex_handle {
        let mut vertex = Vertex2D::new(Default::default());
        vertex.common = EntityCommon {
            handle: Handle::new(vertex_handle),
            owner_handle: Handle::new(entity_handle),
            ..EntityCommon::default()
        };
        polyline.vertices.push(vertex);
    }
    EntityType::Polyline2D(polyline)
}

fn source_document() -> CadDocument {
    let mut doc = CadDocument::with_version(DxfVersion::AC1015);
    for (entity, vertex, seqend) in [
        (0x100, 0x110, 0x42A),
        (0x101, 0x111, 0x431),
        (0x102, 0x112, 0x706),
        (0x103, 0x113, 0x79E),
    ] {
        doc.add_entity(source_polyline(entity, seqend, Some(vertex)))
            .unwrap();
    }
    // The source reader's allocator is below the compatibility-only handles.
    doc.header.source_handle_seed = Some(0x40);
    doc.header.handle_seed = 0x200;
    doc
}

fn seqend_handles(value: &serde_json::Value) -> Vec<&str> {
    value["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|object| object["type"] == "Seqend")
        .map(|object| object["handle"].as_str().unwrap())
        .collect()
}

#[test]
fn modern_dxf_projects_sequence_handles_in_collection_order() {
    let doc = source_document();
    let dump = dump_document(&doc, "fixture.dxf", "dxf");
    assert_eq!(seqend_handles(&dump), vec!["#116", "#117", "#118", "#119"]);
    assert_eq!(dump["header"]["HandleSeed"], 0x11A);
}

#[test]
fn constructed_documents_keep_sequence_source_handles() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1015);
    doc.add_entity(source_polyline(0x100, 0x42A, None)).unwrap();
    let dump = dump_document(&doc, "", "dxf");
    assert_eq!(seqend_handles(&dump), vec!["#42A"]);
}
