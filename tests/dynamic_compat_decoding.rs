//! Byte fixtures for dynamic-block DXF fields that were previously discarded.

use acadrust::{
    objects::{DynamicBlockData, ObjectType},
    types::{Handle, Vector3},
    CadDocument, DxfReader,
};
use std::io::Cursor;

fn read_object(kind: &str, payload: &str) -> CadDocument {
    let dxf = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\n{kind}\n5\n900\n330\n0\n{payload}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(dxf.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn data(doc: &CadDocument) -> &DynamicBlockData {
    match &doc.objects[&Handle::new(0x900)] {
        ObjectType::DynamicBlock(object) => &object.data,
        value => panic!("expected dynamic object, got {value:?}"),
    }
}

#[test]
fn alignment_grip_reads_all_three_orientation_components_and_legacy_codes() {
    for components in [
        "140\n2.5\n141\n3.5\n142\n4.5\n",
        "140\n2.5\n150\n3.5\n160\n4.5\n",
    ] {
        let doc = read_object(
            "BLOCKALIGNMENTGRIP",
            &format!("100\nAcDbBlockAlignmentGrip\n{components}"),
        );
        let DynamicBlockData::AlignmentGrip(value) = data(&doc) else {
            panic!()
        };
        assert_eq!(value.orientation, Vector3::new(2.5, 3.5, 4.5));
    }
}

#[test]
fn lookup_parameter_reads_autocad_and_legacy_subclass_spellings() {
    for subclass in ["AcDbBlockLookUpParameter", "AcDbBlockLookupParameter"] {
        let doc = read_object("BLOCKLOOKUPPARAMETER", &format!("100\nAcDbEvalExpr\n90\n12\n98\n33\n99\n329\n100\nAcDbBlock1PtParameter\n93\n25\n100\n{subclass}\n94\n42\n303\nLookup label\n304\nLookup description\n"));
        let DynamicBlockData::LookupParameter(value) = data(&doc) else {
            panic!()
        };
        assert_eq!(value.index, 42);
        assert_eq!(value.lookup_name, "Lookup label");
        assert_eq!(value.lookup_description, "Lookup description");
        assert_eq!(value.parameter.property_count, 25);
        assert_eq!(value.parameter.parameter.element.eval.node_id, 12);
    }
}
