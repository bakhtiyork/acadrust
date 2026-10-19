// SPDX-License-Identifier: MPL-2.0
//! Synthetic source-identity regressions; no corpus or oracle files are needed.
use acadrust::entities::EntityType;
use acadrust::types::{Color, Handle};
use acadrust::{CadDocument, DxfReader};
use std::io::Cursor;

#[test]
fn polyface_face_constructors_identify_face_records() {
    use acadrust::entities::PolyfaceFace;
    for face in [
        PolyfaceFace::triangle(1, 2, 3),
        PolyfaceFace::quad(1, 2, 3, 4),
        PolyfaceFace::triangle_with_visibility(1, 2, 3, true, false, true),
    ] {
        // DXF VERTEX bit 128 identifies a polyface record; bit 64 is
        // reserved for coordinate vertices rather than face indices.
        assert_eq!(face.flags.bits(), 128);
    }
}

fn read(version: &str, flags: u16, vertex_flags: u16) -> CadDocument {
    let source = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{version}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n5\n100\n70\n{flags}\n39\n2.5\n40\n4\n41\n5\n0\nVERTEX\n5\n101\n8\n0\n62\n3\n48\n2.5\n60\n1\n370\n25\n102\n{{ACAD_REACTORS\n330\n777\n102\n}}\n102\n{{ACAD_XDICTIONARY\n360\n778\n102\n}}\n10\n1.25\n20\n2.5\n30\n3.75\n70\n{vertex_flags}\n91\n17\n0\nSEQEND\n5\n102\n62\n4\n48\n3.5\n60\n1\n102\n{{ACAD_REACTORS\n330\n779\n102\n}}\n0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(source.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn dxf_child_common_data_survives_each_native_polyline_route() {
    for (flags, vertex_flags) in [(0, 0), (8, 32), (64, 192), (16, 64)] {
        let doc = read("AC1032", flags, vertex_flags);
        let entity = doc.get_entity(Handle::new(0x100)).unwrap();
        let (child, seqend) = match entity {
            EntityType::Polyline2D(p) => {
                assert_eq!(p.vertices[0].id, 17);
                (&p.vertices[0].common, p.seqend.as_ref().unwrap())
            }
            EntityType::Polyline3D(p) => {
                assert_eq!(p.thickness, 2.5);
                assert_eq!((p.default_start_width, p.default_end_width), (4.0, 5.0));
                (&p.vertices[0].common, p.seqend.as_ref().unwrap())
            }
            EntityType::PolyfaceMesh(p) => {
                assert_eq!(p.thickness, 2.5);
                assert_eq!((p.start_width, p.end_width), (4.0, 5.0));
                (&p.vertices[0].common, p.seqend.as_ref().unwrap())
            }
            EntityType::PolygonMesh(p) => (&p.vertices[0].common, p.seqend.as_ref().unwrap()),
            e => panic!("unexpected route: {e:?}"),
        };
        assert_eq!(child.handle, Handle::new(0x101));
        assert_eq!(child.owner_handle, Handle::new(0x100));
        assert_eq!(child.color, Color::from_index(3));
        assert_eq!(child.linetype_scale, 2.5);
        assert!(child.invisible);
        assert_eq!(child.line_weight.value(), 25);
        assert_eq!(child.reactors, [Handle::new(0x777)]);
        assert_eq!(child.xdictionary_handle, Some(Handle::new(0x778)));
        assert_eq!(seqend.common.handle, Handle::new(0x102));
        assert_eq!(seqend.common.owner_handle, Handle::new(0x100));
        assert_eq!(seqend.common.color, Color::from_index(4));
        assert_eq!(seqend.common.linetype_scale, 3.5);
        assert!(seqend.common.invisible);
        assert_eq!(seqend.common.reactors, [Handle::new(0x779)]);
    }
}

#[cfg(feature = "acadsharp-compat")]
#[test]
fn r12_oracle_types_do_not_change_native_routes_or_modern_projection() {
    for (flags, vertex_flags, native, child_type) in [
        (0, 0, "Polyline2D", "Vertex2D"),
        (8, 32, "Polyline3D", "Vertex3D"),
        (64, 192, "PolyfaceMesh", "VertexFaceMesh"),
        (16, 64, "PolygonMesh", "PolygonMeshVertex"),
    ] {
        let doc = read("AC1009", flags, vertex_flags);
        let dump = acadrust::compat::acadsharp::dump_document(&doc, "synthetic.dxf", "dxf");
        let objects = dump["objects"].as_array().unwrap();
        let object = |h: &str| objects.iter().find(|o| o["handle"] == h).unwrap();
        assert_eq!(object("#100")["type"], "Polyline2D");
        assert_eq!(object("#101")["type"], "Vertex2D");
        assert_eq!(object("#101")["owner"], "#100");
        assert_eq!(object("#101")["props"]["Flags"], vertex_flags);
        assert_eq!(object("#102")["type"], "Seqend");
        assert_eq!(objects.iter().filter(|o| o["handle"] == "#102").count(), 1);
        // Source format is part of the view: the same native state is not
        // relabeled when projected as a DWG read.
        let native_dump = acadrust::compat::acadsharp::dump_document(&doc, "synthetic.dwg", "dwg");
        let objects = native_dump["objects"].as_array().unwrap();
        assert_eq!(
            objects.iter().find(|o| o["handle"] == "#100").unwrap()["type"],
            native
        );
        assert_eq!(
            objects.iter().find(|o| o["handle"] == "#101").unwrap()["type"],
            child_type
        );
    }
}

#[cfg(feature = "acadsharp-compat")]
#[test]
fn oracle_widths_and_child_appearance_do_not_overwrite_native_data() {
    let text = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n5\n100\n62\n2\n40\n0.2\n41\n0.1\n0\nVERTEX\n5\n101\n62\n3\n10\n1\n20\n2\n0\nVERTEX\n5\n103\n40\n0\n41\n0\n10\n3\n20\n4\n0\nSEQEND\n5\n104\n62\n4\n0\nENDSEC\n0\nEOF\n";
    let doc = DxfReader::from_reader(Cursor::new(text.as_bytes()))
        .unwrap()
        .read()
        .unwrap();
    let EntityType::Polyline2D(polyline) = doc.get_entity(Handle::new(0x100)).unwrap() else {
        panic!("2D");
    };
    assert_eq!(polyline.vertices[0].start_width, 0.2);
    assert_eq!(polyline.vertices[1].start_width, 0.0);
    assert_eq!(polyline.vertices[0].common.color, Color::from_index(3));
    let dump = acadrust::compat::acadsharp::dump_document(&doc, "synthetic.dxf", "dxf");
    let objects = dump["objects"].as_array().unwrap();
    let props = |h: &str| &objects.iter().find(|o| o["handle"] == h).unwrap()["props"];
    assert_eq!(props("#100")["StartWidth"], 0.2);
    assert_eq!(props("#100")["EndWidth"], 0.1);
    assert_eq!(props("#101")["StartWidth"], 0.0);
    assert_eq!(props("#103")["StartWidth"], 0.0);
    assert_eq!(props("#101")["Color"]["Index"], 2);
    assert_eq!(props("#104")["Color"]["Index"], 4);
}

#[test]
fn document_allocator_reserves_inline_children_before_assigning_parent() {
    use acadrust::entities::{Polyline2D, Seqend, Vertex2D};
    use acadrust::types::Vector3;
    let mut doc = CadDocument::new();
    let mut polyline = Polyline2D::new();
    let mut vertex = Vertex2D::new(Vector3::ZERO);
    vertex.common.handle = Handle::new(0x2000);
    polyline.vertices.push(vertex);
    let mut seqend = Seqend::new();
    seqend.common.handle = Handle::new(0x3000);
    polyline.seqend = Some(Box::new(seqend));
    let parent = doc.add_entity(EntityType::Polyline2D(polyline)).unwrap();
    assert!(parent.value() > 0x3000);
    assert!(doc.allocate_handle().value() > parent.value());
}

#[cfg(feature = "acadsharp-compat")]
#[test]
fn legacy_polyface_face_records_remain_ordered_vertex_identities() {
    let source = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n5\n100\n70\n64\n0\nVERTEX\n5\n101\n70\n192\n10\n1\n20\n2\n30\n3\n0\nVERTEX\n5\n102\n70\n128\n10\n0\n20\n0\n30\n0\n71\n1\n72\n-1\n73\n1\n0\nSEQEND\n5\n103\n0\nENDSEC\n0\nEOF\n";
    let mut doc = DxfReader::from_reader(Cursor::new(source.as_bytes()))
        .unwrap()
        .read()
        .unwrap();
    let EntityType::PolyfaceMesh(mesh) = doc.get_entity(Handle::new(0x100)).unwrap() else {
        panic!("mesh");
    };
    assert_eq!(mesh.faces[0].index2, -1);
    for version in [
        acadrust::types::DxfVersion::AC1009,
        acadrust::types::DxfVersion::Unknown,
    ] {
        doc.version = version;
        let dump = acadrust::compat::acadsharp::dump_document(&doc, "legacy.dxf", "dxf");
        let objects = dump["objects"].as_array().unwrap();
        let object = |h: &str| objects.iter().find(|o| o["handle"] == h).unwrap();
        assert_eq!(
            object("#100")["props"]["Vertices"],
            serde_json::json!(["#101", "#102"])
        );
        assert_eq!(object("#102")["type"], "Vertex2D");
        assert_eq!(object("#102")["owner"], "#100");
        assert_eq!(object("#102")["props"]["Flags"], 128);
        assert!(object("#102")["props"].get("Index2").is_none());
    }
}
