// SPDX-License-Identifier: MPL-2.0
use std::io::Cursor;

use rayon::ThreadPoolBuilder;

use acadrust::entities::{
    EntityCommon, EntityType, Insert, PolyfaceFace, PolyfaceMesh, PolyfaceVertex,
    PolygonMeshEntity, PolygonMeshVertex, Polyline2D, Polyline3D, Seqend, Vertex2D,
    Vertex3DPolyline,
};
use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use acadrust::{
    CadDocument, Color, DwgReader, DwgWriter, DxfReader, DxfVersion, DxfWriter, Handle, Line,
    Vector3,
};

const P2D: Handle = Handle::new(0x1100);
const P2D_VERTEX: Handle = Handle::new(0x1101);
const P2D_SEQEND: Handle = Handle::new(0x1102);
const P3D: Handle = Handle::new(0x1200);
const P3D_VERTEX: Handle = Handle::new(0x1201);
const P3D_SEQEND: Handle = Handle::new(0x1202);
const PF: Handle = Handle::new(0x1300);
const PF_VERTEX: Handle = Handle::new(0x1301);
const PF_FACE: Handle = Handle::new(0x1302);
const PF_SEQEND: Handle = Handle::new(0x1303);
const PM: Handle = Handle::new(0x1400);
const PM_VERTEX: Handle = Handle::new(0x1401);
const PM_SEQEND: Handle = Handle::new(0x1402);
const INSERT: Handle = Handle::new(0x1500);
const INSERT_SEQEND: Handle = Handle::new(0x1501);

const ROUNDTRIP_VERSIONS: &[DxfVersion] = &[
    DxfVersion::AC1014,
    DxfVersion::AC1015,
    DxfVersion::AC1018,
    DxfVersion::AC1021,
    DxfVersion::AC1024,
    DxfVersion::AC1027,
    DxfVersion::AC1032,
];

fn common(handle: Handle, color: Color, invisible: bool) -> EntityCommon {
    let mut value = EntityCommon::new();
    value.handle = handle;
    value.color = color;
    value.invisible = invisible;
    value
}

fn marked_seqend(handle: Handle) -> Seqend {
    Seqend {
        common: common(handle, Color::Index(5), true),
    }
}

fn tagged_xdata(value: &str) -> ExtendedData {
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("ACAD");
    record.add_value(XDataValue::String(value.to_owned()));
    data.add_record(record);
    data
}

fn assert_tagged_xdata(common: &EntityCommon, expected: &str) {
    let record = common
        .extended_data
        .get_record("ACAD")
        .unwrap_or_else(|| panic!("missing ACAD XDATA record for {expected}"));
    assert_eq!(
        record.values.as_slice(),
        &[XDataValue::String(expected.to_owned())]
    );
}

fn identity_document(version: DxfVersion, extra_lines: usize) -> CadDocument {
    let mut document = CadDocument::with_version(version);

    let mut p2d = Polyline2D::new();
    p2d.common = common(P2D, Color::Index(2), false);
    let mut p2d_vertex = Vertex2D::new(Vector3::new(1.0, 2.0, 0.0));
    p2d_vertex.common = common(P2D_VERTEX, Color::Index(3), true);
    p2d_vertex.common.extended_data = tagged_xdata("p2d-vertex");
    if version >= DxfVersion::AC1024 {
        p2d_vertex.id = 17;
    }
    p2d.vertices.push(p2d_vertex);
    let mut p2d_seqend = marked_seqend(P2D_SEQEND);
    p2d_seqend.common.extended_data = tagged_xdata("p2d-seqend");
    p2d.seqend = Some(Box::new(p2d_seqend));
    document.add_entity(EntityType::Polyline2D(p2d)).unwrap();

    let mut p3d = Polyline3D::new();
    p3d.common = common(P3D, Color::Index(2), false);
    let mut p3d_vertex = Vertex3DPolyline::from_xyz(3.0, 4.0, 5.0);
    p3d_vertex.common = common(P3D_VERTEX, Color::Index(4), true);
    p3d.vertices.push(p3d_vertex);
    p3d.seqend = Some(Box::new(marked_seqend(P3D_SEQEND)));
    document.add_entity(EntityType::Polyline3D(p3d)).unwrap();

    let mut polyface = PolyfaceMesh::new();
    polyface.common = common(PF, Color::Index(2), false);
    let mut polyface_vertex = PolyfaceVertex::new(Vector3::new(6.0, 7.0, 8.0));
    polyface_vertex.common = common(PF_VERTEX, Color::Index(6), true);
    polyface.vertices.push(polyface_vertex);
    let mut polyface_face = PolyfaceFace::triangle(1, 1, 1);
    polyface_face.common = common(PF_FACE, Color::Index(7), true);
    polyface.faces.push(polyface_face);
    polyface.seqend = Some(Box::new(marked_seqend(PF_SEQEND)));
    document
        .add_entity(EntityType::PolyfaceMesh(polyface))
        .unwrap();

    let mut polygon = PolygonMeshEntity::new();
    polygon.common = common(PM, Color::Index(2), false);
    polygon.m_vertex_count = 1;
    polygon.n_vertex_count = 1;
    let mut polygon_vertex = PolygonMeshVertex::new();
    polygon_vertex.common = common(PM_VERTEX, Color::Index(8), true);
    polygon.vertices.push(polygon_vertex);
    polygon.seqend = Some(Box::new(marked_seqend(PM_SEQEND)));
    document
        .add_entity(EntityType::PolygonMesh(polygon))
        .unwrap();

    let mut insert = Insert::new("*Model_Space", Vector3::ZERO);
    insert.common = common(INSERT, Color::Index(2), false);
    insert.seqend = Some(Box::new(marked_seqend(INSERT_SEQEND)));
    document.add_entity(EntityType::Insert(insert)).unwrap();

    // Additions must allocate above every inline child, including SEQEND.
    // More than 1024 records also exercises the rayon entity pass.
    for _ in 0..extra_lines {
        document
            .add_entity(EntityType::Line(Line::from_coords(
                0.0, 0.0, 0.0, 1.0, 1.0, 0.0,
            )))
            .unwrap();
    }

    document
}

fn assert_identity(document: &CadDocument) {
    let EntityType::Polyline2D(p2d) = document.get_entity(P2D).unwrap() else {
        panic!("missing 2D polyline");
    };
    assert_eq!(p2d.vertices[0].common.handle, P2D_VERTEX);
    if document.version >= DxfVersion::AC1024 {
        assert_eq!(p2d.vertices[0].id, 17);
    }
    assert_eq!(p2d.vertices[0].common.owner_handle, P2D);
    assert_eq!(p2d.vertices[0].common.color, Color::Index(3));
    assert!(p2d.vertices[0].common.invisible);
    assert_tagged_xdata(&p2d.vertices[0].common, "p2d-vertex");
    let p2d_seqend = p2d.seqend.as_ref().unwrap();
    assert_eq!(p2d_seqend.common.handle, P2D_SEQEND);
    assert_eq!(p2d_seqend.common.owner_handle, P2D);
    assert_eq!(p2d_seqend.common.color, Color::Index(5));
    assert!(p2d_seqend.common.invisible);
    assert_tagged_xdata(&p2d_seqend.common, "p2d-seqend");

    let EntityType::Polyline3D(p3d) = document.get_entity(P3D).unwrap() else {
        panic!("missing 3D polyline");
    };
    assert_eq!(p3d.vertices[0].common.handle, P3D_VERTEX);
    assert_eq!(p3d.vertices[0].common.owner_handle, P3D);
    assert_eq!(p3d.vertices[0].common.color, Color::Index(4));
    assert!(p3d.vertices[0].common.invisible);
    let p3d_seqend = p3d.seqend.as_ref().unwrap();
    assert_eq!(p3d_seqend.common.handle, P3D_SEQEND);
    assert_eq!(p3d_seqend.common.owner_handle, P3D);
    assert_eq!(p3d_seqend.common.color, Color::Index(5));
    assert!(p3d_seqend.common.invisible);

    let EntityType::PolyfaceMesh(mesh) = document.get_entity(PF).unwrap() else {
        panic!("missing polyface mesh");
    };
    assert_eq!(mesh.vertices[0].common.handle, PF_VERTEX);
    assert_eq!(mesh.vertices[0].common.owner_handle, PF);
    assert_eq!(mesh.faces[0].common.handle, PF_FACE);
    assert_eq!(mesh.faces[0].common.owner_handle, PF);
    assert_eq!(mesh.faces[0].flags.bits(), 128);
    assert_eq!(mesh.vertices[0].common.color, Color::Index(6));
    assert_eq!(mesh.faces[0].common.color, Color::Index(7));
    let pf_seqend = mesh.seqend.as_ref().unwrap();
    assert_eq!(pf_seqend.common.handle, PF_SEQEND);
    assert_eq!(pf_seqend.common.owner_handle, PF);
    assert_eq!(pf_seqend.common.color, Color::Index(5));
    assert!(pf_seqend.common.invisible);
    assert_eq!(mesh.seqend_handle, Some(PF_SEQEND));

    let EntityType::PolygonMesh(mesh) = document.get_entity(PM).unwrap() else {
        panic!("missing polygon mesh");
    };
    assert_eq!(mesh.vertices[0].common.handle, PM_VERTEX);
    assert_eq!(mesh.vertices[0].common.owner_handle, PM);
    assert_eq!(mesh.vertices[0].common.color, Color::Index(8));
    let pm_seqend = mesh.seqend.as_ref().unwrap();
    assert_eq!(pm_seqend.common.handle, PM_SEQEND);
    assert_eq!(pm_seqend.common.owner_handle, PM);
    assert_eq!(pm_seqend.common.color, Color::Index(5));
    assert!(pm_seqend.common.invisible);

    let EntityType::Insert(insert) = document.get_entity(INSERT).unwrap() else {
        panic!("missing insert");
    };
    let insert_seqend = insert.seqend.as_ref().unwrap();
    assert_eq!(insert_seqend.common.handle, INSERT_SEQEND);
    assert_eq!(insert_seqend.common.owner_handle, INSERT);
    assert_eq!(insert_seqend.common.color, Color::Index(5));
    assert!(insert_seqend.common.invisible);
}

#[test]
fn dxf_and_serial_dwg_preserve_compound_child_identity() {
    for &version in ROUNDTRIP_VERSIONS {
        let source = identity_document(version, 0);

        let dxf = DxfWriter::new(&source).write_to_vec().unwrap();
        let dxf_roundtrip = DxfReader::from_reader(Cursor::new(dxf))
            .unwrap()
            .read()
            .unwrap();
        assert_identity(&dxf_roundtrip);

        let dwg = DwgWriter::write_to_vec(&source).unwrap();
        let dwg_roundtrip = ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap()
            .install(|| DwgReader::from_stream(Cursor::new(dwg)).read().unwrap());
        assert_identity(&dwg_roundtrip);
    }
}

#[test]
fn rayon_dwg_read_preserves_compound_child_identity() {
    let source = identity_document(DxfVersion::AC1032, 1_100);
    let dwg = DwgWriter::write_to_vec(&source).unwrap();
    let roundtrip = ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap()
        .install(|| DwgReader::from_stream(Cursor::new(dwg)).read().unwrap());
    assert_eq!(roundtrip.entities().count(), source.entities().count());
    assert_identity(&roundtrip);
}
