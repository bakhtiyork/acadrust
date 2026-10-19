// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, handle, new_document};
use acadrust::entities::{EntityType, Line};
use acadrust::objects::ObjectType;
use acadrust::types::{Handle, Vector3};
use acadrust::CadDocument;
use serde_json::{json, Value};

fn object(dump: &Value, h: Handle) -> &Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == handle(h))
        .unwrap()
}

#[test]
fn default_registration_graph_and_dependencies() {
    let doc = new_document();
    let dump = dump_document(&doc, "", "new");
    let objects = dump["objects"].as_array().unwrap();
    assert_eq!(objects.len(), 71);
    assert!(dump["classes"].as_array().unwrap().is_empty());
    // The native writer registry is distinct from the empty source classes.
    assert!(doc.classes.contains("MATERIAL"));
    assert!(doc.classes.contains("TABLESTYLE"));
    for (index, value) in objects.iter().enumerate() {
        assert_eq!(value["handle"], handle(Handle::new(index as u64 + 1)));
        let owner = value["owner"].as_str().expect("all defaults have an owner");
        assert!(owner == "#0" || objects.iter().any(|o| o["handle"] == owner));
    }
    // The table constructor order is part of the registration contract.
    let tables = [
        "BlockRecordsTable",
        "LayersTable",
        "DimensionStylesTable",
        "TextStylesTable",
        "LineTypesTable",
        "ViewsTable",
        "UCSTable",
        "VPortsTable",
        "AppIdsTable",
    ];
    for (value, expected) in objects.iter().zip(tables) {
        assert_eq!(value["type"], expected);
        assert_eq!(value["owner"], "#0");
    }
    assert_eq!(doc.header.handle_seed, 72);
    assert_eq!(doc.next_handle(), 72);
    assert_eq!(doc.app_ids.len(), 1);
    assert_eq!(doc.line_types.len(), 3);
    assert_eq!(doc.text_styles.len(), 1);
    let by_layer = doc.line_types.get("ByLayer").unwrap().handle;
    let standard = doc.text_styles.get("Standard").unwrap().handle;
    assert_eq!(by_layer.value(), 43); // dependency of MLineStyle, before table defaults
    assert_eq!(standard.value(), 45); // dependency of MultiLeaderStyle
    assert_eq!(dump["header"]["CurrentLineType"], handle(by_layer));
    assert_eq!(dump["header"]["CurrentTextStyle"], handle(standard));
    assert_eq!(dump["header"]["DimensionTextStyle"], handle(standard));
    let dimension = doc.dim_styles.get("Standard").unwrap();
    assert_eq!(
        object(&dump, dimension.handle)["props"]["Style"],
        handle(standard)
    );
    for value in objects {
        match value["type"].as_str().unwrap() {
            "CadDictionary" => assert_eq!(value["props"]["ClonningFlags"], 0),
            "MLineStyle" => {
                let elements = value["props"]["Elements"].as_array().unwrap();
                assert_eq!(elements.len(), 2);
                assert!(elements.iter().all(|e| e["LineType"] == handle(by_layer)));
            }
            "MultiLeaderStyle" => {
                assert_eq!(value["props"]["TextStyle"], handle(standard));
                assert_eq!(value["props"]["LeaderLineType"], handle(by_layer));
            }
            _ => {}
        }
    }
    let root = object(&dump, doc.header.named_objects_dict_handle);
    assert_eq!(root["owner"], "#0");
    assert_eq!(
        root["props"]["EntryNames"],
        json!([
            "ACAD_COLOR",
            "ACAD_GROUP",
            "ACAD_LAYOUT",
            "ACAD_MATERIAL",
            "ACAD_SORTENTS",
            "ACAD_MLEADERSTYLE",
            "ACAD_MLINESTYLE",
            "ACAD_TABLESTYLE",
            "ACAD_PLOTSETTINGS",
            "AcDbVariableDictionary",
            "ACAD_SCALELIST",
            "ACAD_VISUALSTYLE",
            "ACAD_IMAGE_DICT",
            "ACAD_FIELDLIST",
            "ACAD_PDFDEFINITIONS"
        ])
    );
    let scales: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            ObjectType::Scale(scale) => Some(scale),
            _ => None,
        })
        .collect();
    assert_eq!(scales.len(), 17);
    assert_eq!(scales.iter().filter(|s| s.is_unit_scale).count(), 1);
}

#[test]
fn layouts_register_blocks_markers_and_paper_viewport_once() {
    let mut doc = new_document();
    let dump = dump_document(&doc, "", "new");
    for (name, expected_viewports) in [("*Model_Space", 0), ("*Paper_Space", 1)] {
        let block = doc.block_records.get(name).unwrap();
        let layout = object(&dump, block.layout);
        assert_eq!(layout["props"]["AssociatedBlock"], handle(block.handle));
        assert_eq!(
            layout["props"]["Viewports"].as_array().unwrap().len(),
            expected_viewports
        );
        assert_eq!(block.entity_handles.len(), expected_viewports);
        assert!(layout["props"]["LastActiveViewport"].is_null());
        assert!(layout["props"]["BaseUCS"].is_null());
        assert!(layout["props"]["UCS"].is_null());
        for marker in [block.block_entity_handle, block.block_end_handle] {
            assert_eq!(object(&dump, marker)["owner"], handle(block.handle));
            assert!(!block.entity_handles.contains(&marker));
            assert!(doc.get_entity(marker).is_some());
        }
        if expected_viewports == 1 {
            let viewport = block.entity_handles[0];
            assert_eq!(layout["props"]["PaperViewport"], handle(viewport));
            assert_eq!(object(&dump, viewport)["owner"], handle(block.handle));
            assert_eq!(object(&dump, viewport)["props"]["Id"], 1);
        } else {
            assert!(layout["props"]["PaperViewport"].is_null());
        }
    }
    let next = doc
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        )))
        .unwrap();
    assert_eq!(next.value(), 72);
    assert_eq!(doc.header.handle_seed, 73);
    assert_eq!(
        doc.get_entity(next).unwrap().common().owner_handle,
        doc.header.model_space_block_handle
    );
    assert_eq!(
        dump_document(&doc, "", "new")["objects"]
            .as_array()
            .unwrap()
            .len(),
        72
    );
}

#[test]
fn native_dwg_write_preserves_default_materials_and_table_style() {
    use acadrust::io::dwg::{DwgReader, DwgWriter};
    let doc = new_document();
    let bytes = DwgWriter::write_to_vec(&doc).unwrap();
    let read = DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    for (h, object) in &doc.objects {
        match object {
            ObjectType::Material(material) => {
                let Some(ObjectType::Material(actual)) = read.objects.get(h) else {
                    panic!("native writer lost material {} at {h:?}", material.name)
                };
                assert_eq!(actual.name, material.name);
            }
            ObjectType::TableStyle(style) => {
                let Some(ObjectType::TableStyle(actual)) = read.objects.get(h) else {
                    panic!("native writer lost table style at {h:?}")
                };
                assert_eq!(actual.name, style.name);
            }
            _ => {}
        }
    }
    assert!(dump_document(&doc, "", "new")["classes"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn default_proxy_view_preserves_native_graphic_bytes_in_both_reader_views() {
    let mut doc = CadDocument::new();
    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.graphic_data = Some(vec![0, 1, 255, 42]);
    let h = doc.add_entity(EntityType::Line(line)).unwrap();
    let mut polyline = acadrust::entities::Polyline3D::from_points(vec![Vector3::ZERO]);
    let vertex = Handle::new(0x7000);
    polyline.vertices[0].handle = vertex;
    doc.add_entity(EntityType::Polyline3D(polyline)).unwrap();
    for reader in ["dxf", "dwg"] {
        let dump = dump_document(&doc, "synthetic", reader);
        for value in dump["objects"].as_array().unwrap() {
            if let Some(proxy) = value["props"].get("ProxyGeometries") {
                assert_eq!(*proxy, json!([]));
            }
        }
        assert_eq!(object(&dump, h)["props"]["ProxyGeometries"], json!([]));
        assert_eq!(object(&dump, vertex)["props"]["ProxyGeometries"], json!([]));
    }
    assert_eq!(
        doc.get_entity(h).unwrap().common().graphic_data,
        Some(vec![0, 1, 255, 42])
    );
}

#[test]
fn native_constructor_retains_its_writer_defaults() {
    let doc = CadDocument::new();
    assert_eq!(doc.layers.get("0").unwrap().handle.value(), 0x10);
    assert_eq!(doc.dim_styles.handle().value(), 0xA);
    assert!(doc.classes.get_by_name("LAYOUT").is_some());
    assert!(doc.classes.get_by_name("ACDBDICTIONARYWDFLT").is_some());
    assert!(doc.app_ids.get("AcadAnnotative").is_some());
}

#[test]
fn paper_viewport_comes_from_block_membership_not_layout_source_list() {
    let mut doc = new_document();
    let block = doc.block_records.get("*Paper_Space").unwrap();
    let layout_handle = block.layout;
    let viewport_handle = block.entity_handles[0];
    let Some(ObjectType::Layout(layout)) = doc.objects.get_mut(&layout_handle) else {
        panic!("paper layout")
    };
    layout.viewports.clear();
    assert_eq!(layout.viewport, Handle::NULL);
    let dump = dump_document(&doc, "", "new");
    assert_eq!(
        object(&dump, layout_handle)["props"]["PaperViewport"],
        handle(viewport_handle)
    );
    assert!(object(&dump, layout_handle)["props"]["LastActiveViewport"].is_null());
}
