#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::{EntityType, Mesh, MeshEdge};
use acadrust::types::{Handle, Vector3};
use acadrust::CadDocument;
use serde_json::json;

fn mesh_object<'a>(dump: &'a serde_json::Value, handle: Handle) -> &'a serde_json::Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["handle"] == json!(format!("#{:X}", handle.value())))
        .unwrap()
}

#[test]
fn mesh_compat_projects_reader_crease_sentinel_and_texture_coordinates() {
    let mut document = CadDocument::new();
    let mut mesh = Mesh::new();
    mesh.edges.push(MeshEdge::new(0, 1));
    mesh.edges.push(MeshEdge::with_crease(1, 2, 0.5));

    let handle = document.add_entity(EntityType::Mesh(mesh)).unwrap();
    let coordinates = vec![Vector3::new(0.25, 0.5, 0.75), Vector3::new(1.0, 2.0, 3.0)];
    document.set_mesh_texture_coordinates(handle, &coordinates);

    let Some(EntityType::Mesh(native)) = document.get_entity(handle) else {
        panic!("expected native mesh");
    };
    assert_eq!(native.edges[0].crease, None);
    assert_eq!(native.edges[1].crease, Some(0.5));
    assert_eq!(document.mesh_texture_coordinates(handle), coordinates);

    let dump = dump_document(&document, "synthetic.dxf", "dxf");
    let object = mesh_object(&dump, handle);
    let props = &object["props"];
    assert_eq!(
        props["Edges"],
        json!([
            {"Crease": 0.0, "End": 1, "Start": 0},
            {"Crease": 0.5, "End": 2, "Start": 1},
        ])
    );
    assert_eq!(
        props["TextureCoordinates"],
        json!([[0.25, 0.5, 0.75], [1.0, 2.0, 3.0]])
    );

    let missing_texture_handle = document.add_entity(EntityType::Mesh(Mesh::new())).unwrap();
    let dump = dump_document(&document, "synthetic.dxf", "dxf");
    assert_eq!(
        mesh_object(&dump, missing_texture_handle)["props"]["TextureCoordinates"],
        json!([])
    );
}
