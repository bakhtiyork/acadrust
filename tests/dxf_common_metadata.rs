use acadrust::entities::underlay::UnderlayType;
use acadrust::objects::{ObjectType, UnderlayDefinition};
use acadrust::CadDocument;
use acadrust::{
    DxfReader, EntityType, Handle, SemanticNodeV1, SemanticPartV1, SemanticRelationshipKindV1,
};
use std::io::Cursor;

const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n5\n510\n330\n1\n8\nA\n2\nB\n70\n0\n10\n0\n20\n0\n30\n0\n102\n{ACAD_XDICTIONARY\n102\n{NESTED\n360\n99\n102\n}\n360\n12\n0\nENDBLK\n5\n511\n330\n1\n8\nA\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nPOINT\n5\n20\n330\n1\n8\nA\n347\n2A\n10\n0\n20\n0\n30\n0\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nSCALE\n5\n30\n330\n1\n102\n{ACAD_REACTORS\n330\n31\n330\n31\n102\n}\n102\n{ACAD_XDICTIONARY\n360\n32\n102\n}\n300\n1:1\n140\n1.0\n141\n1.0\n290\n1\n0\nCUSTOM\n5\n50\n102\n{CUSTOM_GROUP\n330\n51\n102\n}\n330\n1\n0\nDICTIONARYVAR\n5\n40\n330\n1\n102\n{ACAD_XDICTIONARY\n360\n42\n0\nENDSEC\n0\nEOF\n";

#[test]
fn common_material_relationships_and_block_markers_survive_dxf_read() {
    let document = DxfReader::from_reader(Cursor::new(DXF.as_bytes()))
        .expect("reader")
        .read()
        .expect("document");

    let point = document.get_entity(Handle::new(0x20)).expect("point");
    assert!(matches!(point, EntityType::Point(_)));
    assert_eq!(point.common().material_handle, Some(Handle::new(0x2A)));

    let block = document.block_records.get("B").expect("block record");
    let begin = block.block_entity_common.as_ref().expect("BLOCK common");
    let end = block.block_end_common.as_ref().expect("ENDBLK common");
    assert_eq!(begin.handle, Handle::new(0x510));
    assert_eq!(begin.owner_handle, Handle::new(1));
    assert_eq!(begin.layer, "A");
    assert_eq!(begin.xdictionary_handle, Some(Handle::new(0x12)));
    assert_eq!(end.handle, Handle::new(0x511));
    assert_eq!(end.owner_handle, Handle::new(1));
    assert_eq!(end.layer, "A");

    assert_eq!(
        document.extension_dictionary_handle(Handle::new(0x30)),
        Some(Handle::new(0x32))
    );
    assert_eq!(
        document.object_owner(Handle::new(0x30)),
        Some(Handle::new(1))
    );
    assert_eq!(
        document.object_owner(Handle::new(0x50)),
        Some(Handle::new(1))
    );
    assert_eq!(
        document.extension_dictionary_handle(Handle::new(0x50)),
        None
    );
    assert_eq!(
        document.extension_dictionary_handle(Handle::new(0x40)),
        Some(Handle::new(0x42))
    );
    let mut reactors = Vec::new();
    document.semantic_inventory_v1().visit(|part| {
        let SemanticPartV1::Relationship {
            kind: SemanticRelationshipKindV1::Reactor,
            source: acadrust::SemanticReferenceV1::Resolved(SemanticNodeV1::Object(object)),
            target: _,
        } = part
        else {
            return;
        };
        if matches!(object, acadrust::objects::ObjectType::Scale(scale) if scale.handle == Handle::new(0x30)) {
            reactors.push(0x31);
        }
    });
    assert_eq!(reactors, vec![0x31, 0x31]);
}

#[test]
fn underlay_definition_owner_is_visible_to_object_owner_lookup() {
    let mut document = CadDocument::new();
    let mut definition = UnderlayDefinition::new(UnderlayType::Pdf);
    definition.handle = Handle::new(0x50);
    definition.owner_handle = Handle::new(0x60);
    document.objects.insert(
        definition.handle,
        ObjectType::UnderlayDefinition(definition),
    );

    assert_eq!(
        document.object_owner(Handle::new(0x50)),
        Some(Handle::new(0x60))
    );
}
