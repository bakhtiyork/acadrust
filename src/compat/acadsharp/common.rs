//! Source-aware common CadObject/Entity projection. Native values stay intact.
use super::*;
use crate::entities::EntityCommon;
use std::collections::{HashMap, HashSet};

fn id(value: &Value) -> Option<u64> {
    u64::from_str_radix(value.as_str()?.strip_prefix('#')?, 16).ok()
}

/// Color(byte r, byte g, byte b) exposes a little-endian RGB integer in C#.
pub(super) fn appearance_color(value: Color) -> Value {
    let mut result = color(value);
    if let Some((r, g, b)) = value.true_color_rgb() {
        result["TrueColor"] = json!(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16);
    }
    result
}

pub(super) fn entity_color(common: &EntityCommon) -> Value {
    let mut value = common.color;
    if let Some((word, _)) = common.dwg_color_source {
        if word & 0x4000 != 0 {
            // The oracle retains the book reference but leaves Color ByBlock.
            value = Color::ByBlock;
        } else if word & 0x8000 != 0 {
            // Its ENC reader treats every complex color as RGB, including
            // method-encoded ByLayer and ACI values understood by native Rust.
            value = match value {
                Color::ByLayer | Color::ByBlock => Color::from_rgb(0, 0, 0),
                Color::Index(i) => Color::from_rgb(0, 0, i),
                other => other,
            };
        }
    }
    appearance_color(value)
}

pub(super) fn entity_transparency(common: &EntityCommon) -> Value {
    if let Some((word, packed)) = common.dwg_color_source {
        if word == 0 && common.transparency == Transparency::ByLayer {
            // ACadSharp's zero ENC branch assigns Opaque, even though the
            // source has no transparency field and native inheritance is kept.
            return transparency(Transparency::OPAQUE);
        }
        if word & 0x2000 != 0 && common.transparency == Transparency::from_alpha_value(packed) {
            // FromAlphaValue ignores the method byte and uses the low byte.
            return transparency(Transparency::Explicit(255 - packed as u8));
        }
    }
    transparency(common.transparency)
}

/// Resolve template references once, before registration changes handles.
pub(super) fn resolve(doc: &CadDocument, reader: &str, records: &mut [Value]) {
    if !registration::source_document(doc) {
        return;
    }
    let mut types = HashMap::with_capacity(records.len());
    for record in records
        .iter()
        .filter(|r| registration::supported(r, reader))
    {
        if let Some(h) = id(&record["handle"]).filter(|&h| h != 0) {
            types
                .entry(h)
                .or_insert_with(|| record["type"].as_str().unwrap_or("").to_owned());
        }
    }
    let mut groups = HashMap::<u64, Vec<u64>>::new();
    for record in records.iter().filter(|r| r["type"] == "Group") {
        let Some(group) = id(&record["handle"]) else {
            continue;
        };
        let mut seen = HashSet::new();
        if let Some(members) = record["props"]["Entities"].as_array() {
            for h in members.iter().filter_map(id).filter(|h| seen.insert(*h)) {
                groups.entry(h).or_default().push(group);
            }
        }
    }
    for record in records {
        let h = id(&record["handle"]).unwrap_or(0);
        let layer = record["type"] == "Layer";
        let entity = record["props"].get("ProxyGeometries").is_some();
        let Some(props) = record["props"].as_object_mut() else {
            continue;
        };
        if let Some(reactors) = props.get_mut("Reactors").and_then(Value::as_array_mut) {
            let mut seen = HashSet::new();
            reactors.retain(|r| id(r).is_some_and(|h| types.contains_key(&h) && seen.insert(h)));
            if entity {
                if let Some(attachments) = groups.get(&h) {
                    // CadGroupTemplate builds the entity first. Group.Add then
                    // appends its reactor, even when the source already had it.
                    reactors.extend(attachments.iter().map(|&h| handle(Handle::new(h))));
                }
            }
        }
        for (property, expected) in [
            ("XDictionary", "CadDictionary"),
            ("Material", "Material"),
            ("BookColor", "BookColor"),
        ] {
            if let Some(reference) = props.get_mut(property) {
                if id(reference).is_some_and(|h| {
                    !types.get(&h).is_some_and(|ty| {
                        ty == expected
                            || (expected == "CadDictionary" && ty == "CadDictionaryWithDefault")
                    })
                }) {
                    *reference = Value::Null;
                }
            }
        }
        if layer {
            // CadLayerTemplate resolves a material but never assigns it.
            props.insert("Material".into(), Value::Null);
            if let Some(color) = props.get_mut("Color") {
                if color["IsTrueColor"] == true {
                    color["TrueColor"] = json!(
                        color["R"].as_u64().unwrap_or(0)
                            | color["G"].as_u64().unwrap_or(0) << 8
                            | color["B"].as_u64().unwrap_or(0) << 16
                    );
                }
            }
        }
    }
}

/// Assignment to a document resolves table defaults after legacy registration.
pub(super) fn table_defaults(records: &mut [Value]) {
    let mut types = HashMap::with_capacity(records.len());
    let mut zero = Value::Null;
    let mut bylayer = Value::Null;
    for record in records.iter() {
        if let Some(h) = id(&record["handle"]) {
            types
                .entry(h)
                .or_insert_with(|| record["type"].as_str().unwrap_or("").to_owned());
        }
        if record["type"] == "Layer" && record["props"]["Name"] == "0" {
            zero = record["handle"].clone();
        }
        if record["type"] == "LineType"
            && record["props"]["Name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("ByLayer"))
        {
            bylayer = record["handle"].clone();
        }
    }
    for record in records {
        if record["props"].get("ProxyGeometries").is_none() {
            continue;
        }
        for (property, expected, fallback) in [
            ("Layer", "Layer", &zero),
            ("LineType", "LineType", &bylayer),
        ] {
            let reference = &mut record["props"][property];
            if reference.as_str() != Some(UNMAPPED)
                && !id(reference).is_some_and(|h| types.get(&h).is_some_and(|ty| ty == expected))
            {
                *reference = fallback.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enc_projection_preserves_native_methods_and_explicit_edits() {
        let mut common = EntityCommon::new();
        common.color = Color::from_rgb(10, 20, 30);
        common.dwg_color_source = Some((0xC000, 0));
        assert_eq!(entity_color(&common), color(Color::ByBlock));
        assert_eq!(common.color, Color::from_rgb(10, 20, 30));

        common.color = Color::Index(3);
        common.dwg_color_source = Some((0x8000, 0));
        assert_eq!(
            entity_color(&common),
            appearance_color(Color::from_rgb(0, 0, 3))
        );
        common.dwg_color_source = Some((0x2000, 0));
        common.transparency = Transparency::ByLayer;
        assert_eq!(entity_transparency(&common)["Value"], 100);
        common.transparency = Transparency::Explicit(128);
        assert_eq!(entity_transparency(&common)["Value"], 50);
    }

    #[test]
    fn layer_material_resolution_is_an_oracle_omission() {
        let mut doc = CadDocument::new();
        doc.header.source_handle_seed = Some(0x100);
        let mut records = vec![
            json!({"handle":"#10", "type":"Layer", "props":{"Material":"#20", "Color":color(Color::from_rgb(12,34,56))}}),
            json!({"handle":"#20", "type":"Material", "props":{}}),
        ];
        resolve(&doc, "dwg", &mut records);
        assert_eq!(records[0]["props"]["Material"], Value::Null);
        assert_eq!(records[0]["props"]["Color"]["TrueColor"], 0x38220C);
    }
}
