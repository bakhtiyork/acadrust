//! Synthetic regression cases for the dynamic-block compatibility view.

use super::*;
use crate::objects::*;
use crate::{CadDocument, EntityType, Line};

fn mapped(data: DynamicBlockData, class: &str) -> Value {
    let doc = CadDocument::new();
    let object = DynamicBlockObject {
        cpp_class_name: format!("AcDb{class}"),
        data,
        ..Default::default()
    };
    dump_dynamic_block(&doc, "dwg", Handle::new(0x900), &object)["props"].clone()
}

#[test]
fn graph_uses_stored_indices_next_ids_and_edge_flags() {
    let graph = BlockEvaluationGraph {
        first_node_id: 43,
        first_node_id_copy: 44,
        nodes: vec![BlockEvaluationNode {
            id: 17,
            next_id: 29,
            edge_flags: 32,
            node_data: [3, 4, 5, 6],
            expression: Handle::new(0x901),
            ..Default::default()
        }],
        edges: vec![BlockEvaluationEdge {
            id: 31,
            next_id: 7,
            incoming_edge: 9,
            source_node: 17,
            destination_node: 29,
            outgoing_edges: [1, 2, 3, 4, 5],
        }],
    };
    let p = mapped(DynamicBlockData::EvaluationGraph(graph), "EvalGraph");
    assert_eq!(
        p["Nodes"][0],
        json!({
            "Data1":3,"Data2":4,"Data3":5,"Data4":6,"Expression":"#901",
            "Flags":32,"Id":29,"Index":17
        })
    );
    assert_eq!(
        p["Edges"][0],
        json!({
            "Data1":1,"Data2":2,"Data3":3,"Data4":4,"Data5":5,
            "Flags":7,"FromNodeIndex":17,"Index":31,"ToNodeIndex":29,"TrackedCount":9
        })
    );
    assert_eq!(
        (p["Value96"].clone(), p["Value97"].clone()),
        (json!(43), json!(44))
    );
}

#[test]
fn grip_location_connections_and_absent_evaluations_are_projected() {
    for code in [-9999, -1, 0] {
        assert_eq!(
            eval_pair(&BlockEvalExpression {
                value_code: code,
                ..Default::default()
            }),
            Value::Null
        );
    }
    let p = mapped(
        DynamicBlockData::GripLocationComponent(BlockGripExpression {
            eval: BlockEvalExpression {
                node_id: 7,
                major: 4,
                minor: 6,
                value_code: 40,
                value: BlockEvalValue::Real(12.5),
                ..Default::default()
            },
            grip_type: 19,
            expression: "UpdatedEndX".into(),
        }),
        "BlockGripExpr",
    );
    assert_eq!(p["Connection"], json!({"Id":19,"Name":"UpdatedEndX"}));
    assert_eq!(p["EvaluatedValue"], json!({"Code":40,"Value":12.5}));
    assert_eq!(p["Name"], "");
    assert_eq!(p["Value98"], 4);
    assert_eq!(p["Value99"], 6);
}

#[test]
fn all_grips_keep_expression_ids_cycling_weight_and_element_versions_distinct() {
    let grip = BlockGrip {
        element: BlockElement {
            eval: BlockEvalExpression {
                major: 1,
                minor: 2,
                node_id: 13,
                ..Default::default()
            },
            major: 33,
            minor: 329,
            name: "End Grip".into(),
            eed_1071: 12,
        },
        flags_91: 41,
        flags_92: 42,
        insert_cycling_weight: -1,
        location: Vector3::new(1.0, 2.0, 3.0),
        insert_cycling: true,
    };
    let oriented = BlockOrientedGrip {
        grip: grip.clone(),
        orientation: Vector3::new(4.0, 5.0, 6.0),
    };
    let cases = [
        (
            DynamicBlockData::AlignmentGrip(oriented.clone()),
            "BlockAlignmentGrip",
        ),
        (DynamicBlockData::LinearGrip(oriented), "BlockLinearGrip"),
        (
            DynamicBlockData::FlipGrip(BlockFlipGrip {
                grip: grip.clone(),
                combined_state: 73,
                orientation: Vector3::new(7.0, 8.0, 9.0),
            }),
            "BlockFlipGrip",
        ),
        (
            DynamicBlockData::LookupGrip(grip.clone()),
            "BlockLookupGrip",
        ),
        (DynamicBlockData::PolarGrip(grip.clone()), "BlockPolarGrip"),
        (
            DynamicBlockData::RotationGrip(grip.clone()),
            "BlockRotationGrip",
        ),
        (
            DynamicBlockData::VisibilityGrip(grip.clone()),
            "BlockVisibilityGrip",
        ),
        (DynamicBlockData::XYGrip(grip), "BlockXYGrip"),
    ];
    for (data, class) in cases {
        let p = mapped(data, class);
        assert_eq!(p["ExpressionId1"], 41, "{class}");
        assert_eq!(p["ExpressionId2"], 42, "{class}");
        assert_eq!(p["Value93"], -1, "{class}");
        assert_eq!(p["Value98"], 33, "{class}");
        assert_eq!(p["Value99"], 329, "{class}");
        assert_eq!(p["Id"], 13, "{class}");
        assert_eq!(p["Name"], "", "{class}");
        assert_eq!(p["EvaluatedValue"], Value::Null, "{class}");
        match class {
            "BlockAlignmentGrip" => assert_eq!(p["AlignmentY"], 5.0),
            "BlockLinearGrip" => assert_eq!(p["DistanceZ"], 6.0),
            "BlockFlipGrip" => assert_eq!(p["FlipExpressionId"], 73),
            "BlockLookupGrip" => assert_eq!(p["SubclassMarker"], "AcDbBlockLookUpGrip"),
            _ => {}
        }
    }
}

#[test]
fn parameter_connections_and_grip_ids_use_the_retained_slots() {
    let property = |id| BlockParameterProperty {
        connections: vec![BlockConnection {
            code: id,
            name: format!("Connection{id}"),
        }],
    };
    let one = BlockOnePointParameter {
        property_count: 91,
        properties: [property(3), property(4)],
        ..Default::default()
    };
    let p = mapped(
        DynamicBlockData::PointParameter(BlockPointParameter {
            parameter: one,
            ..Default::default()
        }),
        "BlockPointParameter",
    );
    assert_eq!(p["GripId"], 91);
    assert_eq!(
        p["DisplacementX"],
        json!({"Connections":[{"Id":3,"Name":"Connection3"}]})
    );
    assert_eq!(p["DisplacementY"]["Connections"][0]["Id"], 4);

    let two = BlockTwoPointParameter {
        properties: [property(5), property(6), property(7), property(8)],
        property_states: [71, 72, 73, 74],
        ..Default::default()
    };
    let p = mapped(
        DynamicBlockData::FlipParameter(BlockFlipParameter {
            parameter: two,
            flags_96: 17,
            tooltip: "UpdatedFlip".into(),
            ..Default::default()
        }),
        "BlockFlipParameter",
    );
    assert_eq!(p["GripIds"], json!([71, 72, 73, 74]));
    for (name, id) in [
        ("FirstPointDisplacementX", 5),
        ("FirstPointDisplacementY", 6),
        ("SecondPointDisplacementX", 7),
        ("SecondPointDisplacementY", 8),
    ] {
        assert_eq!(p[name]["Connections"][0]["Id"], id);
    }
    assert_eq!(
        p["UpdatedFlipConnection"],
        json!({"Id":17,"Name":"UpdatedFlip"})
    );
}

#[test]
fn visibility_states_resolve_references_and_keep_dwg_quirks_out_of_native_state() {
    let mut doc = CadDocument::new();
    let entity = doc.add_entity(EntityType::Line(Line::new())).unwrap();
    let expression = Handle::new(0x900);
    doc.objects.insert(
        expression,
        ObjectType::DynamicBlock(DynamicBlockObject {
            handle: expression,
            cpp_class_name: "AcDbBlockLinearGrip".into(),
            data: DynamicBlockData::LinearGrip(BlockOrientedGrip::default()),
            ..Default::default()
        }),
    );
    let parameter = BlockVisibilityParameter {
        name: "Visible label".into(),
        eval_node_id: 12,
        property_info_count: 34,
        chain_actions: false,
        is_initialized: true,
        all_blocks: vec![entity, Handle::new(0xdead)],
        property_info: [
            BlockParameterPropertyInfo {
                connections: vec![BlockParameterConnection {
                    code: 27,
                    name: "X".into(),
                }],
            },
            Default::default(),
        ],
        states: vec![BlockVisibilityState {
            name: "shown".into(),
            visible_blocks: vec![entity, entity, Handle::new(0xdead)],
            visible_params: vec![expression, expression, entity, Handle::new(0xdead)],
        }],
        ..Default::default()
    };
    for reader in ["dwg", "dxf"] {
        let p = dump_visibility_parameter(&doc, reader, Handle::new(0x901), &parameter)["props"]
            .clone();
        assert_eq!(p["Name"], "");
        assert_eq!(p["Label"], "Visible label");
        assert_eq!(p["GripId"], if reader == "dxf" { 0 } else { 34 });
        assert_eq!(p["ChainActions"], reader == "dwg");
        assert_eq!(p["Value281"], reader == "dxf");
        assert_eq!(p["Entities"], json!([handle(entity)]));
        assert_eq!(
            p["States"],
            json!([["shown", {"Name":"shown", "Entities":[handle(entity)], "Expressions":["#900"]}]])
        );
        assert_eq!(p["DisplacementX"]["Connections"][0]["Id"], 27);
        assert_eq!(p["DisplacementY"], json!({"Connections":[]}));
    }
    assert!(!parameter.chain_actions);
    assert!(parameter.is_initialized);
    assert_eq!(parameter.property_info_count, 34);
    assert_eq!(parameter.states[0].visible_params.len(), 4);
}

#[test]
fn dynamic_names_come_from_dictionary_ownership() {
    let mut doc = CadDocument::new();
    let owner = Handle::new(0x900);
    let h = Handle::new(0x901);
    let mut dictionary = Dictionary::new();
    dictionary.add_entry("ACAD_ENHANCEDBLOCK", h);
    doc.objects
        .insert(owner, ObjectType::Dictionary(dictionary));
    let graph = DynamicBlockObject {
        owner,
        dxf_name: "ACAD_EVALUATION_GRAPH".into(),
        cpp_class_name: "AcDbEvalGraph".into(),
        data: DynamicBlockData::EvaluationGraph(Default::default()),
        ..Default::default()
    };
    assert_eq!(
        dump_dynamic_block(&doc, "dwg", h, &graph)["props"]["Name"],
        "ACAD_ENHANCEDBLOCK"
    );
}

#[test]
fn lookup_and_rotation_markers_use_acadsharp_spelling() {
    let p = mapped(
        DynamicBlockData::LookupParameter(Default::default()),
        "BlockLookupParameter",
    );
    assert_eq!(p["SubclassMarker"], "AcDbBlockLookUpParameter");
    let p = mapped(
        DynamicBlockData::RotateAction(Default::default()),
        "BlockRotateAction",
    );
    assert_eq!(p["SubclassMarker"], "AcDbBlockRotationAction");
}
