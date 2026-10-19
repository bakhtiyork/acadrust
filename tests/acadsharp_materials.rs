// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, new_document};
use acadrust::objects::{Material, MaterialTexture, ObjectType};
use acadrust::types::{DxfVersion, Handle};
use acadrust::{CadDocument, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn read(codes: &str) -> CadDocument {
    let source = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n\
         0\nSECTION\n2\nOBJECTS\n0\nMATERIAL\n5\nF001\n100\nAcDbMaterial\n1\nTest\n\
         {codes}0\nENDSEC\n0\nEOF\n"
    );
    DxfReader::from_reader(Cursor::new(source.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn material(doc: &CadDocument) -> &Material {
    match doc.objects.get(&Handle::new(0xF001)).unwrap() {
        ObjectType::Material(value) => value,
        _ => panic!("expected material"),
    }
}

fn projected(doc: &CadDocument, reader: &str) -> Value {
    dump_document(doc, "", reader)["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["handle"] == "#F001")
        .unwrap()["props"]
        .clone()
}

#[test]
fn omitted_dxf_material_fields_use_view_defaults_without_changing_native_values() {
    let doc = read("");
    let native = material(&doc);
    assert_eq!(native.channel_flags, 127);
    assert_eq!(native.diffuse_map.source, 0);
    assert_eq!(native.diffuse_map.projection, 2);
    assert!(native.ambient_color.rgb.is_none());
    let p = projected(&doc, "dxf");
    assert_eq!(p["ChannelFlags"], 1);
    assert_eq!(p["Description"], Value::Null);
    for prefix in [
        "Diffuse",
        "Specular",
        "Reflection",
        "Opacity",
        "Bump",
        "Refraction",
    ] {
        assert_eq!(p[format!("{prefix}MapSource")], 1);
        assert_eq!(p[format!("{prefix}ProjectionMethod")], 1);
        assert_eq!(p[format!("{prefix}MapFileName")], Value::Null);
    }
    for prefix in ["Ambient", "Diffuse", "Specular"] {
        assert_eq!(p[format!("{prefix}Color")]["IsByBlock"], true);
    }
    let mut without_provenance = native.clone();
    without_provenance.dxf_source = None;
    assert_eq!(*native, without_provenance);
}

#[test]
fn explicit_dxf_zero_defaults_and_empty_strings_are_preserved() {
    let mut codes = "2\n\n94\n127\n".to_string();
    for (source, projection, filename) in [
        (72, 73, 3),
        (77, 78, 4),
        (171, 172, 6),
        (175, 176, 7),
        (179, 270, 8),
        (273, 274, 9),
    ] {
        codes += &format!("{source}\n0\n{projection}\n2\n{filename}\n\n");
    }
    let doc = read(&codes);
    let p = projected(&doc, "dxf");
    assert_eq!(p["ChannelFlags"], 127);
    assert_eq!(p["Description"], "");
    for prefix in [
        "Diffuse",
        "Specular",
        "Reflection",
        "Opacity",
        "Bump",
        "Refraction",
    ] {
        assert_eq!(p[format!("{prefix}MapSource")], 0);
        assert_eq!(p[format!("{prefix}ProjectionMethod")], 2);
        assert_eq!(p[format!("{prefix}MapFileName")], "");
    }
}

#[test]
fn dxf_color_methods_keep_native_rgb_and_project_mapper_semantics() {
    let doc = read("70\n1\n90\n1122867\n71\n1\n91\n4478310\n76\n1\n92\n7833753\n");
    let native = material(&doc);
    assert_eq!(native.diffuse_color.rgb, Some(0x445566));
    assert_eq!(native.specular_color.rgb, Some(0x778899));
    let p = projected(&doc, "dxf");
    assert_eq!(p["AmbientColor"]["R"], 0x11);
    assert_eq!(p["AmbientColor"]["G"], 0x22);
    assert_eq!(p["AmbientColor"]["B"], 0x33);
    assert_eq!(p["AmbientColor"]["TrueColor"], 0x332211);
    assert_eq!(p["DiffuseColor"]["IsByBlock"], true);
    assert_eq!(p["SpecularColor"]["IsByBlock"], true);
    assert_eq!(p["DiffuseColorMethod"], 1);
    // Native code 90 also supports a historical self-illumination slot. The
    // reference mapper always assigns it to AmbientColor, regardless of method.
    let doc = read("70\n0\n90\n1122867\n");
    assert_eq!(material(&doc).self_illumination, 1122867.0);
    let p = projected(&doc, "dxf");
    assert_eq!(p["AmbientColor"]["TrueColor"], 0x332211);
    assert_eq!(p["SelfIllumination"], 0.0);
}

#[test]
fn dxf_advanced_map_aliases_are_projected_without_overwriting_native_maps() {
    let doc = read(
        "42\n0.75\n73\n2\n460\n1\n42\n0.25\n73\n3\n3\nnormal.png\n270\n4\n271\n5\n272\n6\n273\n7\n",
    );
    let native = material(&doc);
    assert_eq!(native.diffuse_map.blend_factor, 0.75);
    assert_eq!(native.diffuse_map.projection, 2);
    assert_eq!(native.normal_map.blend_factor, 0.25);
    assert_eq!(native.normal_map.file_name, "normal.png");
    let p = projected(&doc, "dxf");
    assert_eq!(p["DiffuseMapBlendFactor"], 0.25);
    assert_eq!(p["DiffuseProjectionMethod"], 3);
    assert_eq!(p["DiffuseMapFileName"], "normal.png");
    assert_eq!(p["BumpProjectionMethod"], 4);
    assert_eq!(p["BumpTilingMethod"], 5);
    assert_eq!(p["BumpAutoTransform"], 6);
    assert_eq!(p["RefractionMapSource"], 7);
}

#[test]
fn dwg_material_colors_filenames_and_legacy_channel_defaults_are_source_aware() {
    let mut doc = CadDocument::new();
    let mut value = Material::new();
    value.handle = Handle::new(0xF001);
    value.ambient_color.flag = 1;
    value.ambient_color.rgb = Some(0x123456);
    value.diffuse_map.source = 1;
    value.diffuse_map.texture = Some(MaterialTexture::default());
    doc.objects
        .insert(value.handle, ObjectType::Material(value));
    for (version, channels) in [(DxfVersion::AC1018, 1), (DxfVersion::AC1021, 127)] {
        doc.version = version;
        let p = projected(&doc, "dwg");
        assert_eq!(p["AmbientColor"]["TrueColor"], 0x563412);
        assert_eq!(p["DiffuseColor"]["IsByBlock"], true);
        assert_eq!(p["DiffuseMapFileName"], "");
        assert_eq!(p["RefractionMapFileName"], Value::Null);
        assert_eq!(p["ChannelFlags"], channels);
        assert_eq!(p["DiffuseProjectionMethod"], 2);
        assert!(material(&doc).diffuse_map.texture.is_some());
    }
}

#[test]
fn compatibility_new_document_installs_material_constructor_defaults() {
    let doc = new_document();
    let dump = dump_document(&doc, "", "new");
    let materials: Vec<_> = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["type"] == "Material")
        .collect();
    assert_eq!(materials.len(), 3);
    for material in materials {
        let p = &material["props"];
        assert_eq!(p["ChannelFlags"], 1);
        assert_eq!(p["Description"], Value::Null);
        assert_eq!(p["AmbientColor"]["IsByBlock"], true);
        assert_eq!(p["DiffuseMapSource"], 1);
        assert_eq!(p["DiffuseProjectionMethod"], 1);
        assert_eq!(p["DiffuseMapFileName"], json!(null));
    }
    assert_eq!(Material::new().channel_flags, 127);
}
