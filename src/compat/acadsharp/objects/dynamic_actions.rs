//! ACadSharp-compatible properties for dynamic-block actions.
//!
//! The typed dynamic-block model keeps the action payloads compact.  This
//! module expands the payload into the per-action property names used by the
//! ACadSharp dump schema.  It is called after the common action properties
//! have been collected by `objects.rs`, so entries written here intentionally
//! replace the few generic projections whose ACadSharp names or semantics
//! differ.

use crate::objects::{
    BlockAction, BlockActionWithBasePoint, BlockConnection, BlockLookupAction, DynamicBlockData,
};
use crate::CadDocument;
use serde_json::Value;
use std::collections::HashSet;

use super::{handle, handles, int, props, text, vec3};

/// Add the action-specific ACadSharp projection for `value`.
pub(super) fn extend(
    value: &DynamicBlockData,
    properties: &mut Vec<(&'static str, Value)>,
    doc: &CadDocument,
) {
    match value {
        DynamicBlockData::MoveAction(value) => {
            action_base(&value.action, doc, properties);
            put(
                properties,
                "XDeltaConnection",
                connection(&value.connections[0]),
            );
            put(
                properties,
                "YDeltaConnection",
                connection(&value.connections[1]),
            );
        }
        DynamicBlockData::FlipAction(value) => {
            action_base(&value.action, doc, properties);
            put(
                properties,
                "FlipConnection",
                connection(&value.connections[0]),
            );
            put(
                properties,
                "UpdatedFlipConnection",
                connection(&value.connections[1]),
            );
            put(
                properties,
                "UpdatedBaseConnection",
                connection(&value.connections[2]),
            );
            put(
                properties,
                "UpdatedEndConnection",
                connection(&value.connections[3]),
            );
        }
        DynamicBlockData::RotateAction(value) => {
            base_point_action(&value.action, doc, properties);
            put(
                properties,
                "AngleDeltaConnection",
                value
                    .connections
                    .first()
                    .map(connection)
                    .unwrap_or(Value::Null),
            );
            // The Rust reader names the first native base-point triple
            // `offset`; ACadSharp exposes that triple as BasePoint.  Its
            // final triple is exposed as Value1012.
            put(properties, "BasePoint", vec3(value.action.offset));
            put(properties, "Value1012", vec3(value.action.base_point));
            put(properties, "Value280", Value::Bool(value.action.dependent));
        }
        DynamicBlockData::ScaleAction(value) => {
            base_point_action(&value.action, doc, properties);
            put(
                properties,
                "ScaleConnection",
                connection_or_default(value.connections.first()),
            );
            put(
                properties,
                "XScaleConnection",
                connection_or_default(value.connections.get(1)),
            );
            put(
                properties,
                "YScaleConnection",
                connection_or_default(value.connections.get(2)),
            );
            put(properties, "ScaleType", int(value.dwg_tail_byte as i64));
            put(properties, "BasePoint", vec3(value.action.offset));
            put(properties, "Value1012", vec3(value.action.base_point));
            put(properties, "Value280", Value::Bool(value.action.dependent));
        }
        DynamicBlockData::ArrayAction(value) => {
            action_base(&value.action, doc, properties);
            put(
                properties,
                "BaseConnection",
                connection(&value.connections[0]),
            );
            put(
                properties,
                "EndConnection",
                connection(&value.connections[1]),
            );
            put(
                properties,
                "UpdatedBaseConnection",
                connection(&value.connections[2]),
            );
            put(
                properties,
                "UpdatedEndConnection",
                connection(&value.connections[3]),
            );
        }
        DynamicBlockData::LookupAction(value) => {
            action_base(&value.action, doc, properties);
            put(properties, "Columns", lookup_columns(value));
        }
        DynamicBlockData::StretchAction(value) => {
            action_base(&value.action, doc, properties);
            put(
                properties,
                "EndXDeltaConnection",
                connection(&value.connections[0]),
            );
            put(
                properties,
                "EndYDeltaConnection",
                connection(&value.connections[1]),
            );
            put(
                properties,
                "StretchBindings",
                stretch_bindings(doc, &value.handles),
            );
            put(properties, "StretchNodes", stretch_nodes(&value.codes));
        }
        DynamicBlockData::PolarStretchAction(value) => {
            action_base(&value.action, doc, properties);
            put(
                properties,
                "BaseXDeltaConnection",
                connection(&value.connections[0]),
            );
            put(
                properties,
                "BaseYDeltaConnection",
                connection(&value.connections[1]),
            );
            put(
                properties,
                "BaseConnection",
                connection(&value.connections[2]),
            );
            put(
                properties,
                "EndConnection",
                connection(&value.connections[3]),
            );
            put(
                properties,
                "UpdatedBaseConnection",
                connection(&value.connections[4]),
            );
            put(
                properties,
                "UpdatedEndConnection",
                connection(&value.connections[5]),
            );
            put(
                properties,
                "RotateBindings",
                entity_list(doc, value.handles.iter().copied()),
            );
            put(
                properties,
                "StretchBindings",
                stretch_bindings(doc, &value.bindings),
            );
            put(properties, "StretchNodes", stretch_nodes(&value.codes));
        }
        _ => {}
    }
}

/// Add common action properties whose defaults are supplied by `CadObject`.
fn action_base(
    action: &BlockAction,
    doc: &CadDocument,
    properties: &mut Vec<(&'static str, Value)>,
) {
    put(
        properties,
        "Entities",
        entity_handles(doc, action.dependencies.iter().copied()),
    );
}

fn base_point_action(
    action: &BlockActionWithBasePoint,
    doc: &CadDocument,
    properties: &mut Vec<(&'static str, Value)>,
) {
    action_base(&action.action, doc, properties);
    put(
        properties,
        "UpdateBaseXConnection",
        connection_or_default(action.connections.first()),
    );
    put(
        properties,
        "UpdateBaseYConnection",
        connection_or_default(action.connections.get(1)),
    );
}

fn connection(value: &BlockConnection) -> Value {
    props(
        "EvalConnection",
        [("Id", int(value.code as i64)), ("Name", text(&value.name))],
    )
}

fn connection_or_default(value: Option<&BlockConnection>) -> Value {
    value
        .map(connection)
        .unwrap_or_else(|| props("EvalConnection", [("Id", int(0)), ("Name", Value::Null)]))
}

fn lookup_columns(value: &BlockLookupAction) -> Value {
    let rows = value.row_count.max(0) as usize;
    let columns = value.column_count.max(0) as usize;
    Value::Array(
        value
            .columns
            .iter()
            .enumerate()
            .map(|(column_index, column)| {
                let column_rows = if columns == 0 {
                    Vec::new()
                } else {
                    value
                        .expressions
                        .chunks(columns)
                        .take(rows)
                        .filter_map(|row| row.get(column_index).cloned())
                        .map(text)
                        .collect()
                };
                props(
                    "BlockLookupAction.ColumnData",
                    [
                        ("ConnectionName", text(&column.connection_name)),
                        ("IsLookupProperty", Value::Bool(column.lookup_property)),
                        ("IsReadOnly", Value::Bool(!column.writable)),
                        ("NodeId", int(column.node_id as i64)),
                        ("Rows", Value::Array(column_rows)),
                        ("Type", int(column.property_type as i64)),
                        ("UnmatchedName", text(&column.unmatched_name)),
                        ("ValueType", int(column.value_type as i64)),
                    ],
                )
            })
            .collect(),
    )
}

fn entity_handles<I>(doc: &CadDocument, values: I) -> Value
where
    I: IntoIterator<Item = crate::types::Handle>,
{
    let mut seen = HashSet::new();
    handles(
        values
            .into_iter()
            .filter(|value| doc.get_entity(*value).is_some() && seen.insert(*value)),
    )
}

/// Polar selection handles are a list in ACadSharp.  Preserve repeated
/// entries while dropping references that do not resolve to entities.
fn entity_list<I>(doc: &CadDocument, values: I) -> Value
where
    I: IntoIterator<Item = crate::types::Handle>,
{
    handles(
        values
            .into_iter()
            .filter(|value| doc.get_entity(*value).is_some()),
    )
}

fn stretch_bindings(doc: &CadDocument, values: &[crate::objects::BlockStretchHandle]) -> Value {
    Value::Array(
        values
            .iter()
            .filter(|value| doc.get_entity(value.handle).is_some())
            .map(|value| {
                props(
                    "StretchEntityBind",
                    [
                        ("Entity", handle(value.handle)),
                        (
                            "PointIndexes",
                            Value::Array(
                                value
                                    .indexes
                                    .iter()
                                    .map(|index| int(*index as i64))
                                    .collect(),
                            ),
                        ),
                    ],
                )
            })
            .collect(),
    )
}

fn stretch_nodes(values: &[crate::objects::BlockStretchCode]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| {
                props(
                    "StretchNode",
                    [
                        ("NodeId", int(value.code as i64)),
                        (
                            "PointIndexes",
                            Value::Array(
                                value
                                    .indexes
                                    .iter()
                                    .map(|index| int(*index as i64))
                                    .collect(),
                            ),
                        ),
                    ],
                )
            })
            .collect(),
    )
}

/// Replace a key already emitted by `dynamic_data_props`, or append it.
fn put(properties: &mut Vec<(&'static str, Value)>, key: &'static str, value: Value) {
    if let Some((_, current)) = properties.iter_mut().find(|(name, _)| *name == key) {
        *current = value;
    } else {
        properties.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::{
        BlockActionOffsets, BlockActionWithBasePoint, BlockArrayAction, BlockBasePointAction,
        BlockElement, BlockEvalExpression, BlockFlipAction, BlockLookupAction, BlockLookupColumn,
        BlockMoveAction, BlockPolarStretchAction, BlockStretchAction, BlockStretchCode,
        BlockStretchHandle,
    };
    use crate::types::{Handle, Vector2, Vector3};
    use crate::{CadDocument, EntityType, Line};
    use serde_json::json;

    fn connection(code: i32, name: &str) -> BlockConnection {
        BlockConnection {
            code,
            name: name.to_owned(),
        }
    }

    fn action() -> BlockAction {
        BlockAction {
            element: BlockElement {
                eval: BlockEvalExpression {
                    value_code: 40,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn project(value: &DynamicBlockData, doc: &CadDocument) -> Vec<(&'static str, Value)> {
        let mut properties = Vec::new();
        extend(value, &mut properties, doc);
        properties
    }

    fn property(properties: &[(&'static str, Value)], name: &str) -> Value {
        properties
            .iter()
            .find(|(key, _)| *key == name)
            .unwrap_or_else(|| panic!("missing property {name}"))
            .1
            .clone()
    }

    fn document_with_line(handle: Handle) -> CadDocument {
        let mut document = CadDocument::new();
        let mut line = Line::new();
        line.common.handle = handle;
        document.add_entity(EntityType::Line(line)).unwrap();
        document
    }

    #[test]
    fn projects_move_flip_and_array_connections() {
        let doc = CadDocument::new();
        let move_value = DynamicBlockData::MoveAction(BlockMoveAction {
            action: action(),
            connections: [connection(11, "dx"), connection(12, "dy")],
            offsets: BlockActionOffsets::default(),
        });
        let move_properties = project(&move_value, &doc);
        assert_eq!(
            property(&move_properties, "XDeltaConnection")["Id"],
            json!(11)
        );
        assert_eq!(
            property(&move_properties, "YDeltaConnection")["Name"],
            json!("dy")
        );

        let flip_value = DynamicBlockData::FlipAction(BlockFlipAction {
            action: action(),
            connections: [
                connection(21, "flip"),
                connection(22, "updated-flip"),
                connection(23, "updated-base"),
                connection(24, "updated-end"),
            ],
        });
        let flip_properties = project(&flip_value, &doc);
        assert_eq!(
            property(&flip_properties, "UpdatedFlipConnection")["Id"],
            json!(22)
        );
        assert_eq!(
            property(&flip_properties, "UpdatedEndConnection")["Name"],
            json!("updated-end")
        );

        let array_value = DynamicBlockData::ArrayAction(BlockArrayAction {
            action: action(),
            connections: [
                connection(31, "base"),
                connection(32, "end"),
                connection(33, "updated-base"),
                connection(34, "updated-end"),
            ],
            ..Default::default()
        });
        let array_properties = project(&array_value, &doc);
        assert_eq!(
            property(&array_properties, "BaseConnection")["Id"],
            json!(31)
        );
        assert_eq!(
            property(&array_properties, "UpdatedEndConnection")["Id"],
            json!(34)
        );
    }

    #[test]
    fn projects_rotate_and_scale_base_point_fields() {
        let doc = CadDocument::new();
        let base = BlockActionWithBasePoint {
            action: action(),
            offset: Vector3::new(1.0, 2.0, 3.0),
            connections: [connection(41, "update-x"), connection(42, "update-y")].into(),
            dependent: true,
            base_point: Vector3::new(4.0, 5.0, 6.0),
        };
        let rotate_value = DynamicBlockData::RotateAction(BlockBasePointAction {
            action: base.clone(),
            connections: vec![connection(43, "angle")],
            ..Default::default()
        });
        let rotate_properties = project(&rotate_value, &doc);
        assert_eq!(
            property(&rotate_properties, "BasePoint"),
            json!([1.0, 2.0, 3.0])
        );
        assert_eq!(
            property(&rotate_properties, "Value1012"),
            json!([4.0, 5.0, 6.0])
        );
        assert_eq!(property(&rotate_properties, "Value280"), json!(true));
        assert_eq!(
            property(&rotate_properties, "UpdateBaseYConnection")["Id"],
            json!(42)
        );
        assert_eq!(
            property(&rotate_properties, "AngleDeltaConnection")["Name"],
            json!("angle")
        );

        let scale_value = DynamicBlockData::ScaleAction(BlockBasePointAction {
            action: base,
            connections: vec![
                connection(51, "scale"),
                connection(52, "x-scale"),
                connection(53, "y-scale"),
            ],
            dwg_tail_byte: 7,
        });
        let scale_properties = project(&scale_value, &doc);
        assert_eq!(property(&scale_properties, "ScaleType"), json!(7));
        assert_eq!(
            property(&scale_properties, "XScaleConnection")["Id"],
            json!(52)
        );
        assert_eq!(
            property(&scale_properties, "UpdateBaseXConnection")["Id"],
            json!(41)
        );
        let absent = project(&DynamicBlockData::RotateAction(Default::default()), &doc);
        assert_eq!(property(&absent, "AngleDeltaConnection"), Value::Null);
        assert_eq!(
            property(&absent, "UpdateBaseXConnection"),
            json!({"Id":0,"Name":null})
        );
    }

    #[test]
    fn projects_lookup_rows_and_read_only_columns() {
        let value = DynamicBlockData::LookupAction(BlockLookupAction {
            action: action(),
            row_count: 2,
            column_count: 2,
            expressions: vec!["r0c0", "r0c1", "r1c0", "r1c1"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            columns: vec![
                BlockLookupColumn {
                    node_id: 61,
                    writable: true,
                    ..Default::default()
                },
                BlockLookupColumn {
                    node_id: 62,
                    writable: false,
                    ..Default::default()
                },
            ],
            flag_280: true,
        });
        let properties = project(&value, &CadDocument::new());
        let columns = property(&properties, "Columns");
        assert_eq!(columns[0]["Rows"], json!(["r0c0", "r1c0"]));
        assert_eq!(columns[1]["Rows"], json!(["r0c1", "r1c1"]));
        assert_eq!(columns[0]["IsReadOnly"], json!(false));
        assert_eq!(columns[1]["IsReadOnly"], json!(true));

        // A corrupt declared row count must stay bounded by retained
        // expressions rather than driving an unbounded loop.
        let malformed = DynamicBlockData::LookupAction(BlockLookupAction {
            action: action(),
            row_count: i32::MAX,
            column_count: 2,
            expressions: vec!["only-row".to_owned()],
            columns: vec![BlockLookupColumn::default()],
            ..Default::default()
        });
        let malformed_columns = property(&project(&malformed, &CadDocument::new()), "Columns");
        assert_eq!(malformed_columns[0]["Rows"], json!(["only-row"]));
    }

    #[test]
    fn projects_stretch_bindings_nodes_and_polar_bindings() {
        let entity_handle = Handle::new(0x70);
        let missing_handle = Handle::new(0x71);
        let doc = document_with_line(entity_handle);
        let stretch_value = DynamicBlockData::StretchAction(BlockStretchAction {
            action: BlockAction {
                dependencies: vec![entity_handle, missing_handle, entity_handle],
                ..action()
            },
            connections: [connection(81, "end-x"), connection(82, "end-y")],
            points: vec![Vector2::new(1.0, 2.0)],
            handles: vec![
                BlockStretchHandle {
                    handle: entity_handle,
                    indexes: vec![1, 2],
                },
                BlockStretchHandle {
                    handle: missing_handle,
                    indexes: vec![3],
                },
            ],
            codes: vec![BlockStretchCode {
                code: 83,
                indexes: vec![4, 5],
            }],
            offsets: BlockActionOffsets::default(),
        });
        let stretch_properties = project(&stretch_value, &doc);
        assert_eq!(property(&stretch_properties, "Entities"), json!(["#70"]));
        assert_eq!(
            property(&stretch_properties, "StretchBindings")
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            property(&stretch_properties, "StretchNodes")[0]["NodeId"],
            json!(83)
        );

        let polar_value = DynamicBlockData::PolarStretchAction(BlockPolarStretchAction {
            action: action(),
            connections: [
                connection(91, "base-x"),
                connection(92, "base-y"),
                connection(93, "base"),
                connection(94, "end"),
                connection(95, "updated-base"),
                connection(96, "updated-end"),
            ],
            handles: vec![entity_handle, entity_handle, missing_handle],
            bindings: vec![BlockStretchHandle {
                handle: entity_handle,
                indexes: vec![7],
            }],
            codes: vec![BlockStretchCode {
                code: 97,
                indexes: vec![8],
            }],
            ..Default::default()
        });
        let polar_properties = project(&polar_value, &doc);
        assert_eq!(
            property(&polar_properties, "BaseXDeltaConnection")["Id"],
            json!(91)
        );
        assert_eq!(
            property(&polar_properties, "RotateBindings")[0],
            json!("#70")
        );
        assert_eq!(
            property(&polar_properties, "RotateBindings")
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            property(&polar_properties, "StretchBindings")[0]["Entity"],
            json!("#70")
        );
    }
}
