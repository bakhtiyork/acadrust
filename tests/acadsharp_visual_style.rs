#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{color, dump_document};
use acadrust::objects::{
    ObjectType, VisualStyle, VisualStyleProperty, VisualStylePropertyValue as V,
};
use acadrust::types::{Color, Handle};
use acadrust::CadDocument;
use serde_json::{json, Value};

fn style(legacy: bool) -> VisualStyle {
    let mut value = VisualStyle::new();
    value.handle = Handle::new(0xF000);
    value.description = "Retained style".into();
    value.style_type = 77;
    value.face_lighting_model = 1;
    value.face_lighting_quality = 2;
    value.face_color_mode = 3;
    value.face_modifier = 4;
    value.edge_model = 5;
    value.edge_style = 6;
    value.internal_use_only = true;
    value.properties = (0..58)
        .map(|i| VisualStyleProperty {
            value: V::Long(100 + i),
            enabled: 1,
        })
        .collect();
    for (i, v) in [(4, -0.4), (5, -0.5), (13, 0.13), (16, 0.16), (26, 0.26)] {
        value.properties[i].value = V::Double(v);
    }
    for (i, c) in [(6, 3), (9, 4), (10, 5), (15, 6), (20, 7)] {
        value.properties[i].value = V::Color(Color::Index(c));
    }
    value.properties[24].value = V::Bool(true);
    value.properties[54].value = V::Text("raster.png".into());
    if legacy {
        let indices = [
            4, 5, 6, 9, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 0, 12, 25, 26, 27,
            0,
        ];
        value.properties = indices
            .iter()
            .map(|&i| value.properties[i].clone())
            .collect();
        value.properties[18].value = V::Short(174);
    }
    value
}

fn projected(value: &VisualStyle, reader: &str) -> Value {
    let mut doc = CadDocument::new();
    doc.objects
        .insert(value.handle, ObjectType::VisualStyle(value.clone()));
    // The supplied reader provenance, rather than the file suffix, controls projection.
    let dump = dump_document(&doc, "misleading.dxf", reader);
    assert_eq!(
        doc.objects.get(&value.handle),
        Some(&ObjectType::VisualStyle(value.clone()))
    );
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == "#F000")
        .unwrap()["props"]
        .clone()
}

#[test]
fn dwg_visual_style_projects_only_name_and_type_without_mutating_native_values() {
    for legacy in [false, true] {
        let p = projected(&style(legacy), "dwg");
        assert_eq!(p["Name"], "Retained style");
        assert_eq!(p["Type"], 77);
        for key in [
            "Brightness",
            "DisplaySettings",
            "EdgeApplyStyleFlag",
            "EdgeColor",
            "EdgeCreaseAngle",
            "EdgeIntersectionLineType",
            "EdgeIsolineCount",
            "EdgeJitter",
            "EdgeModifiers",
            "EdgeObscuredLineType",
            "EdgeOverhang",
            "EdgeSilhouetteWidth",
            "EdgeStyle",
            "EdgeStyleModel",
            "EdgeWidth",
            "FaceColorMode",
            "FaceLightingModel",
            "FaceLightingQuality",
            "FaceModifiers",
            "FaceOpacityLevel",
            "FaceSpecularLevel",
            "HaloGap",
            "OpacityLevel",
            "ShadowType",
        ] {
            assert_eq!(p[key].as_f64(), Some(0.0), "{key}");
        }
        for key in [
            "Color",
            "EdgeIntersectionColor",
            "EdgeObscuredColor",
            "EdgeSilhouetteColor",
            "FaceStyleMonoColor",
        ] {
            assert_eq!(p[key], color(Color::ByBlock), "{key}");
        }
        assert_eq!(p["PrecisionFlag"], false);
        assert_eq!(p["InternalFlag"], false);
        assert!(p["Description"].is_null());
        assert!(p["RasterFile"].is_null());
    }
}

#[test]
fn modern_visual_style_slots_do_not_alias_unrelated_fields() {
    let p = projected(&style(false), "new");
    for (key, expected) in [
        ("Brightness", json!(0.26)),
        ("DisplaySettings", json!(125)),
        ("EdgeColor", json!(6)),
        ("EdgeCreaseAngle", json!(0.13)),
        ("EdgeIntersectionLineType", json!(112)),
        ("EdgeIsolineCount", json!(123)),
        ("EdgeModifiers", json!(114)),
        ("EdgeObscuredLineType", json!(111)),
        ("EdgeOverhang", json!(118)),
        ("EdgeSilhouetteWidth", json!(121)),
        ("HaloGap", json!(122)),
        ("OpacityLevel", json!(0.16)),
        ("FaceOpacityLevel", json!(-0.4)),
        ("FaceSpecularLevel", json!(-0.5)),
        ("PrecisionFlag", json!(true)),
        ("RasterFile", json!("raster.png")),
    ] {
        assert_eq!(p[key], expected, "{key}");
    }
    assert_eq!(p["EdgeSilhouetteColor"], color(Color::Index(7)));
    assert_eq!(p["InternalFlag"], true);
    assert_eq!(p["Description"], "Retained style");
}

#[test]
fn legacy_visual_style_slots_preserve_signed_values_and_legacy_only_flag() {
    let legacy = projected(&style(true), "dxf");
    let modern = projected(&style(false), "dxf");
    for key in [
        "Brightness",
        "Color",
        "DisplaySettings",
        "EdgeColor",
        "EdgeCreaseAngle",
        "EdgeIntersectionColor",
        "EdgeIntersectionLineType",
        "EdgeIsolineCount",
        "EdgeJitter",
        "EdgeModifiers",
        "EdgeObscuredColor",
        "EdgeObscuredLineType",
        "EdgeOverhang",
        "EdgeSilhouetteColor",
        "EdgeSilhouetteWidth",
        "EdgeWidth",
        "FaceOpacityLevel",
        "FaceSpecularLevel",
        "HaloGap",
        "OpacityLevel",
        "PrecisionFlag",
        "ShadowType",
    ] {
        assert_eq!(legacy[key], modern[key], "{key}");
    }
    assert_eq!(legacy["EdgeApplyStyleFlag"], 174);
    assert!(legacy["RasterFile"].is_null());
}
