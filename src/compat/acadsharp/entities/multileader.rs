// SPDX-License-Identifier: MPL-2.0
//! The reference's MLeader view; native defaults and content remain intact.

use super::*;
use crate::entities::MultiLeader;
use crate::objects::ObjectType;
use crate::types::Color;

fn leader_color(value: Color) -> Value {
    let mut result = color(value);
    if let Color::Rgb { r, g, b } = value {
        result["TrueColor"] = json!(i32::from(r) | (i32::from(g) << 8) | (i32::from(b) << 16));
    }
    result
}

fn standard_style(doc: &CadDocument) -> Option<Handle> {
    let ObjectType::Dictionary(root) = doc.objects.get(&doc.header.named_objects_dict_handle)?
    else {
        return None;
    };
    let (_, styles) = root
        .entries
        .iter()
        .find(|(name, _)| name == "ACAD_MLEADERSTYLE")?;
    let ObjectType::Dictionary(styles) = doc.objects.get(styles)? else {
        return None;
    };
    styles
        .entries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("Standard"))
        .map(|(_, h)| *h)
}

pub(super) fn collect(doc: &CadDocument, value: &MultiLeader, out: &mut Vec<Value>) {
    // A multileader the reader restored from AutoCAD's round-trip records shows the words of the file's legacy record, as ACadSharp reads them.
    let legacy = crate::io::dwg::autocad_records::unrestored_mleader(doc, value);
    let value = legacy.as_ref().unwrap_or(value);
    let source = value.dxf_source.as_deref();
    let dxf = source.is_some();
    let absent = |code| source.is_some_and(|s| !s.contains(code));
    let scalar = |code, native, default| number(if absent(code) { default } else { native });
    let enumeration =
        |code, native, default| enum_value(if absent(code) { default } else { native });
    let old_dwg = doc
        .dwg_source_version
        .is_some_and(|v| v < DxfVersion::AC1024);
    // Reference DXF templates never assign these two block references, nor
    // construct BlockAttribute entries. DWG templates do both.
    let block = |h: Option<Handle>| {
        if dxf {
            Value::Null
        } else {
            optional_handle(h.filter(|h| doc.block_records.iter().any(|v| v.handle == *h)))
        }
    };
    let attributes = if dxf {
        json!([])
    } else {
        array(value.block_attributes.iter().map(|a| {
            props(
                "MultiLeader.BlockAttribute",
                [
                    (
                        "AttributeDefinition",
                        optional_handle(a.attribute_definition_handle.filter(|h| {
                            matches!(doc.get_entity(*h), Some(EntityType::AttributeDefinition(_)))
                        })),
                    ),
                    ("Index", integer(a.index)),
                    ("Text", Value::String(a.text.clone())),
                    ("Width", number(a.width)),
                ],
            )
        }))
    };
    field_common(
        doc,
        &value.common,
        "MultiLeader",
        "MULTILEADER",
        -999,
        "AcDbMLeader",
        false,
        vec![
            ("Arrowhead", block(value.arrowhead_handle)),
            ("ArrowheadSize", scalar(42, value.arrowhead_size, 0.0)),
            ("BlockAttributes", attributes),
            ("BlockContent", block(value.block_content_handle)),
            (
                "BlockContentColor",
                leader_color(if dxf {
                    Color::ByBlock
                } else {
                    value.block_content_color
                }),
            ),
            (
                "BlockContentConnection",
                enum_value(value.block_connection_type as i64),
            ),
            (
                "BlockContentRotation",
                number(if dxf {
                    value.block_rotation.to_radians()
                } else {
                    value.block_rotation
                }),
            ),
            ("BlockContentScale", vec3(value.block_scale)),
            (
                "ContentType",
                enumeration(172, value.content_type as i64, 0),
            ),
            // Embedded context is an unattached CadObject, not an inline value or
            // an independently registered object. See DUMP-FORMAT's handle rule.
            ("ContextData", handle(Handle::NULL)),
            (
                "EnableAnnotationScale",
                bool_value(value.enable_annotation_scale),
            ),
            (
                "EnableDogleg",
                bool_value(!absent(291) && value.enable_dogleg),
            ),
            (
                "EnableLanding",
                bool_value(!absent(290) && value.enable_landing),
            ),
            ("ExtendedToText", bool_value(value.extend_leader_to_text)),
            ("LandingDistance", scalar(41, value.dogleg_length, 0.0)),
            (
                "LeaderLineType",
                handle(
                    value
                        .line_type_handle
                        .filter(|h| doc.line_types.iter().any(|v| v.handle == *h))
                        .unwrap_or_else(|| {
                            doc.line_types
                                .get("ByLayer")
                                .map(|v| v.handle)
                                .unwrap_or(Handle::NULL)
                        }),
                ),
            ),
            (
                "LeaderLineWeight",
                enum_value(value.line_weight.value() as i64),
            ),
            // DxfPropertyBase ignores packed colors in codes 91/92/93.
            (
                "LineColor",
                leader_color(if dxf {
                    Color::ByLayer
                } else {
                    value.line_color
                }),
            ),
            ("PathType", enumeration(170, value.path_type as i64, 0)),
            (
                "PropertyOverrideFlags",
                integer(value.property_override_flags.bits() as i32),
            ),
            ("ScaleFactor", number(value.scale_factor)),
            (
                "Style",
                handle(
                    value
                        .style_handle
                        .filter(|h| {
                            matches!(doc.objects.get(h), Some(ObjectType::MultiLeaderStyle(_)))
                        })
                        .or_else(|| standard_style(doc))
                        .unwrap_or(Handle::NULL),
                ),
            ),
            ("TextAligninIPE", integer(value.text_align_in_ipe)),
            ("TextAlignment", enum_value(value.text_alignment as i64)),
            ("TextAngle", enum_value(value.text_angle_type as i64)),
            (
                "TextAttachmentDirection",
                enum_value(value.text_attachment_direction as i64),
            ),
            (
                "TextAttachmentPoint",
                enumeration(179, value.text_attachment_point as i64, 0),
            ),
            (
                "TextBottomAttachment",
                enumeration(
                    272,
                    if old_dwg {
                        0
                    } else {
                        value.text_bottom_attachment as i64
                    },
                    0,
                ),
            ),
            (
                "TextColor",
                leader_color(if dxf {
                    Color::ByBlock
                } else {
                    value.text_color
                }),
            ),
            (
                "TextDirectionNegative",
                bool_value(value.text_direction_negative),
            ),
            ("TextFrame", bool_value(value.text_frame)),
            (
                "TextLeftAttachment",
                enumeration(173, value.text_left_attachment as i64, 0),
            ),
            (
                "TextRightAttachment",
                enumeration(95, value.text_right_attachment as i64, 0),
            ),
            (
                "TextStyle",
                handle(
                    value
                        .text_style_handle
                        .filter(|h| doc.text_styles.iter().any(|v| v.handle == *h))
                        .unwrap_or_else(|| {
                            doc.text_styles
                                .get("Standard")
                                .map(|v| v.handle)
                                .unwrap_or(Handle::NULL)
                        }),
                ),
            ),
            (
                "TextTopAttachment",
                enumeration(
                    273,
                    if old_dwg {
                        0
                    } else {
                        value.text_top_attachment as i64
                    },
                    0,
                ),
            ),
        ],
        out,
    );
}
