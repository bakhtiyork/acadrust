// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::{EntityType, Leader, LeaderCreationType};
use acadrust::types::Vector3;
use acadrust::{CadDocument, DxfReader};
use serde_json::Value;
use std::io::Cursor;

fn dxf(codes: &str) -> CadDocument {
    let text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLEADER\n5\n100\n100\nAcDbEntity\n100\nAcDbLeader\n{codes}0\nENDSEC\n0\nEOF\n"
    );
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn native(doc: &CadDocument) -> &Leader {
    doc.entities()
        .find_map(|entity| match entity {
            EntityType::Leader(value) => Some(value),
            _ => None,
        })
        .unwrap()
}

fn projected(doc: &CadDocument) -> Value {
    dump_document(doc, "", "new")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["type"] == "Leader")
        .unwrap()["props"]
        .clone()
}

fn projected_native(leader: Leader) -> Value {
    let mut doc = CadDocument::new();
    doc.add_entity(EntityType::Leader(leader)).unwrap();
    projected(&doc)
}

#[test]
fn dxf_omitted_fields_use_acadsharp_defaults_without_mutating_native_values() {
    let doc = dxf("10\n0\n20\n0\n10\n5\n20\n0\n");
    let value = native(&doc);
    assert_eq!(value.creation_type, LeaderCreationType::NoAnnotation);
    assert!(value.arrow_enabled);
    assert_eq!(value.text_height, 2.5);
    assert_eq!(value.text_width, 0.0);
    assert!(value.dxf_source.is_dxf());
    assert!(!value.dxf_source.contains(71));
    assert!(!value.dxf_source.contains(40));
    assert!(!value.dxf_source.contains(41));
    assert!(!value.dxf_source.contains(73));
    let mut clone = value.clone();
    clone.dxf_source = Default::default();
    assert_eq!(value, &clone);

    let props = projected(&doc);
    assert_eq!(props["ArrowHeadEnabled"], false);
    assert_eq!(props["CreationType"], 0);
    assert_eq!(props["TextHeight"], 0.0);
    assert_eq!(props["TextWidth"], 0.0);
}

#[test]
fn dxf_explicit_fields_are_projected_and_group_75_is_ignored() {
    let doc = dxf(
        "71\n0\n73\n3\n74\n1\n75\n1\n40\n3.5\n41\n4.5\n211\n1\n221\n0\n231\n0\n10\n0\n20\n0\n10\n5\n20\n0\n",
    );
    let value = native(&doc);
    assert!(!value.arrow_enabled);
    assert_eq!(value.creation_type, LeaderCreationType::NoAnnotation);
    assert_eq!(value.text_height, 3.5);
    assert_eq!(value.text_width, 4.5);
    assert!(value.dxf_source.contains(71));
    assert!(value.dxf_source.contains(40));
    assert!(value.dxf_source.contains(41));
    assert!(value.dxf_source.contains(73));

    let props = projected(&doc);
    assert_eq!(props["ArrowHeadEnabled"], false);
    assert_eq!(props["CreationType"], 3);
    assert_eq!(props["HookLineDirection"], 1);
    assert!(value.hookline_enabled);
    assert_eq!(props["HasHookline"], false);
    assert_eq!(props["TextHeight"], 3.5);
    assert_eq!(props["TextWidth"], 4.5);
}

#[test]
fn has_hookline_follows_reversed_segment_and_preserves_throwing_getter() {
    let horizontal = Vector3::UNIT_X;

    let mut same = Leader::two_point(Vector3::ZERO, Vector3::new(5.0, 0.0, 0.0));
    same.horizontal_direction = horizontal;
    assert_eq!(projected_native(same)["HasHookline"], false);

    let mut opposite = Leader::two_point(Vector3::ZERO, Vector3::new(-5.0, 0.0, 0.0));
    opposite.horizontal_direction = horizontal;
    assert_eq!(projected_native(opposite)["HasHookline"], true);

    let mut sloped = Leader::two_point(Vector3::ZERO, Vector3::new(5.0, 1.0, 0.0));
    sloped.horizontal_direction = horizontal;
    assert_eq!(projected_native(sloped)["HasHookline"], false);

    let mut zero_segment = Leader::two_point(Vector3::ZERO, Vector3::ZERO);
    zero_segment.horizontal_direction = horizontal;
    assert_eq!(
        projected_native(zero_segment)["HasHookline"],
        "!throws:InvalidOperationException"
    );

    let mut zero_horizontal = Leader::two_point(Vector3::ZERO, Vector3::new(5.0, 0.0, 0.0));
    zero_horizontal.horizontal_direction = Vector3::ZERO;
    assert_eq!(
        projected_native(zero_horizontal)["HasHookline"],
        "!throws:InvalidOperationException"
    );
}

#[test]
fn style_resolution_and_modern_dwg_text_defaults_are_only_projected() {
    use acadrust::tables::DimStyle;
    use acadrust::types::{DxfVersion, Handle};
    let mut doc = CadDocument::new();
    let mut style = DimStyle::new("Custom");
    style.handle = Handle::new(0xF000);
    doc.dim_styles.add(style).unwrap();
    let mut value = Leader::new();
    value.dimension_style = "Custom".into();
    value.text_height = 3.5;
    value.text_width = 4.5;
    let h = doc.add_entity(EntityType::Leader(value)).unwrap();
    assert_eq!(projected(&doc)["Style"], "#F000");
    doc.dwg_source_version = Some(DxfVersion::AC1021);
    assert_eq!(projected(&doc)["TextHeight"], 3.5);
    doc.dwg_source_version = Some(DxfVersion::AC1024);
    assert_eq!(projected(&doc)["TextHeight"], 0.0);
    assert_eq!(projected(&doc)["TextWidth"], 0.0);
    assert_eq!(native(&doc).text_height, 3.5);
    if let Some(EntityType::Leader(value)) = doc.get_entity_mut(h) {
        value.dimension_style = "Missing".into();
    }
    let standard = doc.dim_styles.get("Standard").unwrap().handle;
    assert_eq!(projected(&doc)["Style"], format!("#{:X}", standard.value()));
}
