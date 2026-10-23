//! Stub object types for DXF objects that need basic round-trip support.
//!
//! These are minimal representations of supported DXF objects
//! that don't require full rich data models for typical usage.

use crate::types::{Color, Handle, Matrix4, Vector2, Vector3};

/// Trait for minimal stub objects that only need handle + owner fields.
/// Used by the generic `read_stub_object` reader.
pub trait StubObject {
    /// Create a new default instance
    fn new_stub() -> Self;
    /// Set the object handle
    fn set_handle(&mut self, handle: Handle);
    /// Set the owner handle
    fn set_owner(&mut self, owner: Handle);
    /// Get the object handle
    fn handle(&self) -> Handle;
}

/// Value of one R2010+ visual-style property.  DWG stores every property
/// together with a short "enabled/inherited" selector.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VisualStylePropertyValue {
    Short(i16),
    Long(i32),
    Double(f64),
    Bool(bool),
    Color(Color),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VisualStyleProperty {
    pub value: VisualStylePropertyValue,
    pub enabled: i16,
}

/// VisualStyle object — named visual rendering style
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct VisualStyle {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    /// Description / name
    pub description: String,
    /// Style type (code 70)
    pub style_type: i16,
    /// Face lighting model (code 71)
    pub face_lighting_model: i16,
    /// Face lighting quality (code 72)
    pub face_lighting_quality: i16,
    /// Face color mode (code 73)
    pub face_color_mode: i16,
    /// Face modifier (code 90)
    pub face_modifier: i32,
    /// Edge model (code 74)
    pub edge_model: i32,
    /// Edge style (code 91)
    pub edge_style: i32,
    /// Internal use only flag (code 291)
    pub internal_use_only: bool,
    /// R2010+ extended lighting model.
    pub extended_lighting_model: i16,
    /// Ordered R2010+ property bag.  Keeping the typed values, rather than the
    /// complete raw object record, makes visual styles editable and portable
    /// across DWG versions without silently resetting the unexposed fields.
    pub properties: Vec<VisualStyleProperty>,
}

impl VisualStyle {
    /// Create a new VisualStyle with defaults
    pub fn new() -> Self {
        VisualStyle {
            handle: Handle::NULL,
            owner: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            description: String::new(),
            style_type: 0,
            face_lighting_model: 0,
            face_lighting_quality: 0,
            face_color_mode: 0,
            face_modifier: 0,
            edge_model: 0,
            edge_style: 0,
            internal_use_only: false,
            extended_lighting_model: 2,
            properties: Vec::new(),
        }
    }

    pub(crate) fn core_properties(&self) -> Vec<VisualStyleProperty> {
        let long = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Long(value),
            enabled: 1,
        };
        let double = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Double(value),
            enabled: 1,
        };
        let bool_value = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Bool(value),
            enabled: 1,
        };
        let color = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Color(value),
            enabled: 1,
        };
        let mut values = vec![
            long(self.face_lighting_model as i32),
            long(self.face_lighting_quality as i32),
            long(self.face_color_mode as i32),
            long(self.face_modifier),
            double(0.6),
            double(30.0),
            color(Color::Index(7)),
            long(self.edge_model),
            long(self.edge_style),
            color(Color::ByLayer),
            color(Color::ByBlock),
            long(1),
            long(1),
            double(1.0),
            long(0),
            color(Color::ByLayer),
            double(1.0),
            long(1),
            long(6),
            long(2),
            color(Color::ByLayer),
            long(5),
            long(0),
            long(0),
            bool_value(false),
            long(1),
            double(0.0),
            long(0),
        ];
        if self.properties.len() >= 28 {
            values.clone_from_slice(&self.properties[..28]);
        } else if self.properties.len() == 24 {
            for (legacy, modern) in [
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 9),
                (4, 10),
                (5, 11),
                (6, 13),
                (7, 14),
                (8, 15),
                (9, 16),
                (10, 17),
                (11, 18),
                (12, 19),
                (13, 20),
                (14, 21),
                (15, 22),
                (16, 23),
                (17, 24),
                (19, 12),
                (20, 25),
                (21, 26),
                (22, 27),
            ] {
                // The R2010+ record writes each property by its own type, so
                // a legacy value takes the modern slot's type (the legacy
                // brightness is a long, the modern one a double).
                let source = &self.properties[legacy];
                let number = match &source.value {
                    VisualStylePropertyValue::Short(v) => Some(*v as f64),
                    VisualStylePropertyValue::Long(v) => Some(*v as f64),
                    VisualStylePropertyValue::Double(v) => Some(*v),
                    VisualStylePropertyValue::Bool(v) => Some(*v as u8 as f64),
                    _ => None,
                };
                let value = match (&values[modern].value, number) {
                    (VisualStylePropertyValue::Long(_), Some(n)) => {
                        VisualStylePropertyValue::Long(n as i32)
                    }
                    // The legacy face opacity and specular are negative while
                    // the face modifier leaves them off; the R2010+ values
                    // are magnitudes and the modifier alone says so.
                    (VisualStylePropertyValue::Double(_), Some(n)) if modern == 4 || modern == 5 => {
                        VisualStylePropertyValue::Double(n.abs())
                    }
                    (VisualStylePropertyValue::Double(_), Some(n)) => {
                        VisualStylePropertyValue::Double(n)
                    }
                    (VisualStylePropertyValue::Bool(_), Some(n)) => {
                        VisualStylePropertyValue::Bool(n != 0.0)
                    }
                    _ => source.value.clone(),
                };
                values[modern] = VisualStyleProperty {
                    value,
                    enabled: source.enabled,
                };
            }
        } else {
            for (index, property) in self.properties.iter().take(28).enumerate() {
                values[index] = property.clone();
            }
        }
        values[0].value = VisualStylePropertyValue::Long(self.face_lighting_model as i32);
        values[1].value = VisualStylePropertyValue::Long(self.face_lighting_quality as i32);
        values[2].value = VisualStylePropertyValue::Long(self.face_color_mode as i32);
        values[3].value = VisualStylePropertyValue::Long(self.face_modifier);
        values[7].value = VisualStylePropertyValue::Long(self.edge_model);
        values[8].value = VisualStylePropertyValue::Long(self.edge_style);
        values
    }

    pub(crate) fn extended_properties(&self) -> Vec<VisualStyleProperty> {
        if self.properties.len() >= 58 {
            return self.properties[28..58].to_vec();
        }
        let long = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Long(value),
            enabled: 1,
        };
        let double = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Double(value),
            enabled: 1,
        };
        let bool_value = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Bool(value),
            enabled: 1,
        };
        let color = |value, enabled| VisualStyleProperty {
            value: VisualStylePropertyValue::Color(value),
            enabled,
        };
        let text = |value: &str| VisualStyleProperty {
            value: VisualStylePropertyValue::Text(value.to_string()),
            enabled: 1,
        };
        vec![
            bool_value(false),
            bool_value(true),
            bool_value(true),
            bool_value(false),
            bool_value(false),
            bool_value(false),
            bool_value(false),
            bool_value(false),
            bool_value(false),
            long(50),
            double(0.0),
            double(1.0),
            long(0),
            color(Color::Rgb { r: 0, g: 0, b: 0 }, 1),
            long(50),
            long(3),
            color(Color::Rgb { r: 0, g: 0, b: 255 }, 1),
            bool_value(false),
            long(50),
            long(50),
            long(50),
            bool_value(false),
            long(50),
            color(Color::ByLayer, 0),
            VisualStyleProperty {
                value: VisualStylePropertyValue::Double(1.0),
                enabled: 0,
            },
            long(2),
            text("strokes_ogs.tif"),
            bool_value(false),
            double(1.0),
            double(1.0),
        ]
    }

    pub(crate) fn legacy_properties(&self) -> Vec<VisualStyleProperty> {
        if self.properties.len() == 24 {
            return self.properties.clone();
        }
        let core = self.core_properties();
        let short = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Short(value),
            enabled: 1,
        };
        let double = |value| VisualStyleProperty {
            value: VisualStylePropertyValue::Double(value),
            enabled: 1,
        };
        // Opacity (modifier bit 1) and specular (bit 2) are stored negative
        // while the modifier leaves them off.
        let signed = |property: &VisualStyleProperty, bit: i32| {
            let mut property = property.clone();
            if let VisualStylePropertyValue::Double(value) = &mut property.value {
                *value = if self.face_modifier & bit != 0 {
                    value.abs()
                } else {
                    -value.abs()
                };
            }
            property
        };
        vec![
            signed(&core[4], 1),
            signed(&core[5], 2),
            core[6].clone(),
            core[9].clone(),
            core[10].clone(),
            core[11].clone(),
            core[13].clone(),
            core[14].clone(),
            core[15].clone(),
            core[16].clone(),
            core[17].clone(),
            core[18].clone(),
            core[19].clone(),
            core[20].clone(),
            core[21].clone(),
            core[22].clone(),
            core[23].clone(),
            core[24].clone(),
            short(0),
            core[12].clone(),
            core[25].clone(),
            core[26].clone(),
            core[27].clone(),
            double(0.0),
        ]
    }

    pub(crate) fn restore_post_2010_properties(
        &mut self,
        entries: &[crate::objects::XRecordEntry],
    ) -> bool {
        use crate::objects::XRecordValue;
        // A legacy (24 properties) or R2010 (28) style; one that holds all 58 has nothing to restore.
        if !matches!(self.properties.len(), 24 | 28) {
            return false;
        }
        let mut properties = self.core_properties();
        properties.extend(self.extended_properties());
        let mut color_index = None;
        let mut found = false;
        for pair in entries.windows(2) {
            let (Some(key), value) = (pair[0].value.as_string(), &pair[1].value) else {
                continue;
            };
            let flag = match value {
                XRecordValue::Int16(v) => Some(*v),
                _ => None,
            };
            if let Some(index) = key.strip_prefix("RTVSPropertyOp") {
                if let (Ok(index), Some(flag)) = (index.parse::<usize>(), flag) {
                    if let Some(property) = properties.get_mut(index) {
                        property.enabled = flag;
                        found = true;
                    }
                }
                continue;
            }
            if let Some(index) = key.strip_prefix("RTVSPost2010PropOp") {
                if let (Ok(index), Some(flag)) = (index.parse::<usize>(), flag) {
                    if let Some(property) = properties.get_mut(index) {
                        property.enabled = flag;
                        found = true;
                    }
                }
                continue;
            }
            let Some(rest) = key.strip_prefix("RTVSPost2010Prop") else {
                continue;
            };
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            let Ok(index) = rest[..digits].parse::<usize>() else {
                continue;
            };
            let Some(property) = properties.get_mut(index) else {
                continue;
            };
            let number = match value {
                XRecordValue::Byte(v) => Some(*v as f64),
                XRecordValue::Int16(v) => Some(*v as f64),
                XRecordValue::Int32(v) => Some(*v as f64),
                XRecordValue::Double(v) => Some(*v),
                XRecordValue::Bool(v) => Some(*v as u8 as f64),
                _ => None,
            };
            let new_value = match (&rest[digits..], &property.value, number) {
                ("ColorIndex", _, Some(n)) => {
                    color_index = Some(n as i16);
                    continue;
                }
                ("ColorRGB", _, Some(n)) => {
                    let raw = n as i64 as i32;
                    let index = color_index.take().unwrap_or(256);
                    VisualStylePropertyValue::Color(match index {
                        0 | 256 | 257 => Color::from_index(index),
                        _ if (raw as u32) >> 24 == 0 => Color::from_true_color_value(raw),
                        _ => Color::from_index(index),
                    })
                }
                ("", VisualStylePropertyValue::Bool(_), Some(n)) => {
                    VisualStylePropertyValue::Bool(n != 0.0)
                }
                ("", VisualStylePropertyValue::Long(_), Some(n)) => {
                    VisualStylePropertyValue::Long(n as i32)
                }
                ("", VisualStylePropertyValue::Double(_), Some(n)) => {
                    VisualStylePropertyValue::Double(n)
                }
                ("", VisualStylePropertyValue::Text(_), _) => match value.as_string() {
                    Some(text) => VisualStylePropertyValue::Text(text.to_string()),
                    None => continue,
                },
                _ => continue,
            };
            property.value = new_value;
            found = true;
        }
        if found {
            self.properties = properties;
        }
        found
    }
}

pub(crate) fn store_visual_style_roundtrip(document: &mut crate::document::CadDocument) {
    use crate::objects::{ObjectType, XRecordEntry, XRecordValue};
    use crate::types::DxfVersion;
    if !(DxfVersion::AC1018..DxfVersion::AC1024).contains(&document.version) {
        return;
    }
    let mut styles: Vec<_> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            // An R2010 style states 28 properties and keeps the later ones in its own round-trip record; a style
            // of 58 properties holds them all.
            ObjectType::VisualStyle(style) if style.properties.len() >= 28 => {
                Some((*handle, style.properties.clone()))
            }
            _ => None,
        })
        .collect();
    // Objects are created in handle order so the output is deterministic.
    styles.sort_by_key(|style| style.0);
    let key = |text: String| XRecordEntry {
        code: 102,
        value: XRecordValue::String(text),
    };
    for (handle, properties) in styles {
        let full = properties.len() >= 58;
        let mut group = Vec::new();
        // The flags of the properties after the 28th, for the style that keeps them in its record.
        let carried_flags: std::collections::HashMap<usize, i16> = document
            .extension_dictionary_handle(handle)
            .and_then(|dictionary| match document.objects.get(&dictionary) {
                Some(ObjectType::Dictionary(entries)) => entries.get("ACAD_XREC_ROUNDTRIP"),
                _ => None,
            })
            .and_then(|record| match document.objects.get(&record) {
                Some(ObjectType::XRecord(record)) => Some(record.entries.clone()),
                _ => None,
            })
            .map(|entries| {
                entries
                    .windows(2)
                    .filter_map(|pair| {
                        let index = pair[0].value.as_string()?.strip_prefix("RTVSPost2010PropOp")?.parse().ok()?;
                        match pair[1].value {
                            XRecordValue::Int16(flag) => Some((index, flag)),
                            _ => None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (index, property) in properties.iter().enumerate().take(58).skip(28).filter(|_| full) {
            let name = format!("RTVSPost2010Prop{index}");
            match &property.value {
                VisualStylePropertyValue::Color(color) => {
                    let (aci, rgb) = match color {
                        Color::Rgb { .. } => (
                            color.approximate_index() as i32,
                            color.to_true_color_value().unwrap_or_default(),
                        ),
                        _ => (color.approximate_index() as i32, 0xFF00_0000_u32 as i32),
                    };
                    group.push(key(format!("{name}ColorIndex")));
                    group.push(XRecordEntry {
                        code: 90,
                        value: XRecordValue::Int32(aci),
                    });
                    group.push(key(format!("{name}ColorRGB")));
                    group.push(XRecordEntry {
                        code: 90,
                        value: XRecordValue::Int32(rgb),
                    });
                }
                value => {
                    group.push(key(name));
                    group.push(match value {
                        VisualStylePropertyValue::Bool(v) => XRecordEntry {
                            code: 280,
                            value: XRecordValue::Byte(*v as u8),
                        },
                        VisualStylePropertyValue::Short(v) => XRecordEntry {
                            code: 90,
                            value: XRecordValue::Int32(*v as i32),
                        },
                        VisualStylePropertyValue::Long(v) => XRecordEntry {
                            code: 90,
                            value: XRecordValue::Int32(*v),
                        },
                        VisualStylePropertyValue::Double(v) => XRecordEntry {
                            code: 140,
                            value: XRecordValue::Double(*v),
                        },
                        VisualStylePropertyValue::Text(v) => XRecordEntry {
                            code: 1,
                            value: XRecordValue::String(v.clone()),
                        },
                        VisualStylePropertyValue::Color(_) => unreachable!(),
                    });
                }
            }
            group.push(key(format!("RTVSPost2010PropOp{index}")));
            group.push(XRecordEntry {
                code: 70,
                value: XRecordValue::Int16(property.enabled),
            });
        }
        // One flag per property id; the ids after the 58 properties are on.
        for index in 0..63 {
            group.push(key(format!("RTVSPropertyOp{index}")));
            group.push(XRecordEntry {
                code: 70,
                value: XRecordValue::Int16(
                    properties
                        .get(index)
                        .map(|p| p.enabled)
                        .or_else(|| carried_flags.get(&index).copied())
                        .unwrap_or(1),
                ),
            });
        }
        let record = document.ensure_xrecord(handle, "ACAD_XREC_ROUNDTRIP");
        let Some(ObjectType::XRecord(record)) = document.objects.get_mut(&record) else {
            continue;
        };
        // Replace an earlier copy of these groups, keep anything else.
        let mut kept = Vec::with_capacity(record.entries.len());
        let mut entries = std::mem::take(&mut record.entries).into_iter();
        while let Some(entry) = entries.next() {
            if entry.code == 102
                && entry.value.as_string().is_some_and(|text| {
                    text.starts_with("RTVSPropertyOp") || (full && text.starts_with("RTVS"))
                })
            {
                entries.next();
                continue;
            }
            kept.push(entry);
        }
        kept.extend(group);
        record.entries = kept;
    }
}

pub(crate) fn restore_visual_style_roundtrip(document: &mut crate::document::CadDocument) {
    restore_visual_style_roundtrip_of(document, &[24]);
}

/// Give the R2010 styles (28 properties) the properties after the 28th that their round-trip record states, for a
/// file of R2013 or later, whose record states all 58 itself.
pub(crate) fn upgrade_r2010_visual_styles(document: &mut crate::document::CadDocument) {
    restore_visual_style_roundtrip_of(document, &[28]);
}

fn restore_visual_style_roundtrip_of(document: &mut crate::document::CadDocument, lengths: &[usize]) {
    use crate::objects::ObjectType;
    let records: Vec<_> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| {
            let ObjectType::VisualStyle(style) = object else {
                return None;
            };
            if !lengths.contains(&style.properties.len()) {
                return None;
            }
            let Some(ObjectType::Dictionary(dictionary)) =
                document.objects.get(&style.xdictionary_handle?)
            else {
                return None;
            };
            let (_, record) = dictionary
                .entries
                .iter()
                .find(|(name, _)| name == "ACAD_XREC_ROUNDTRIP")?;
            let Some(ObjectType::XRecord(record)) = document.objects.get(record) else {
                return None;
            };
            Some((*handle, record.entries.clone()))
        })
        .collect();
    for (handle, entries) in records {
        if let Some(ObjectType::VisualStyle(style)) = document.objects.get_mut(&handle) {
            style.restore_post_2010_properties(&entries);
        }
    }
}

impl Default for VisualStyle {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaterialColor {
    pub flag: u8,
    pub factor: f64,
    pub rgb: Option<i32>,
}

impl Default for MaterialColor {
    fn default() -> Self {
        Self {
            flag: 0,
            factor: 1.0,
            rgb: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MaterialProceduralValue {
    Bool(bool),
    Integer(i16),
    Real(f64),
    Color(Color),
    Text(String),
    Table(Vec<(String, MaterialTexture)>),
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaterialTexture {
    pub mode: i16,
    pub color1: MaterialColor,
    pub color2: MaterialColor,
    pub procedural: Option<MaterialProceduralValue>,
    pub table_end: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaterialMap {
    pub blend_factor: f64,
    pub projection: u8,
    pub tiling: u8,
    pub auto_transform: u8,
    pub transform: [f64; 16],
    pub source: u8,
    pub file_name: String,
    pub texture: Option<MaterialTexture>,
}

impl Default for MaterialMap {
    fn default() -> Self {
        let mut transform = [0.0; 16];
        transform[0] = 1.0;
        transform[5] = 1.0;
        transform[10] = 1.0;
        transform[15] = 1.0;
        Self {
            blend_factor: 1.0,
            projection: 2,
            tiling: 1,
            auto_transform: 1,
            transform,
            source: 0,
            file_name: String::new(),
            texture: None,
        }
    }
}

/// Source presence for a DXF material. Values and textures remain in the native fields.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaterialDxfSource {
    /// Present material group codes (bounded to the DXF scalar code range).
    pub codes: [u64; 8],
    /// Reused group codes present in the advanced/normal-map tail.
    pub advanced_codes: [u64; 8],
    /// Last group 90, which can have a different native semantic role.
    pub last_color_90: Option<i32>,
}

impl MaterialDxfSource {
    pub fn contains(&self, code: usize) -> bool {
        code < 512 && self.codes[code / 64] & (1 << (code % 64)) != 0
    }

    pub fn contains_advanced(&self, code: usize) -> bool {
        code < 512 && self.advanced_codes[code / 64] & (1 << (code % 64)) != 0
    }

    pub(crate) fn record(&mut self, code: i32, advanced: bool) {
        if let Ok(code) = usize::try_from(code) {
            if code < 512 {
                self.codes[code / 64] |= 1 << (code % 64);
                if advanced {
                    self.advanced_codes[code / 64] |= 1 << (code % 64);
                }
            }
        }
    }
}

/// Material object — named material for 3D rendering
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Material {
    /// DXF field presence; excluded from native material equality.
    pub dxf_source: Option<Box<MaterialDxfSource>>,
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    /// Material name
    pub name: String,
    /// Description
    pub description: String,
    pub ambient_color: MaterialColor,
    pub diffuse_color: MaterialColor,
    pub diffuse_map: MaterialMap,
    pub specular_color: MaterialColor,
    pub specular_map: MaterialMap,
    pub specular_gloss_factor: f64,
    pub reflection_map: MaterialMap,
    pub opacity_percent: f64,
    pub opacity_map: MaterialMap,
    pub bump_map: MaterialMap,
    pub refraction_index: f64,
    pub refraction_map: MaterialMap,
    pub translucence: f64,
    pub self_illumination: f64,
    pub reflectivity: f64,
    pub illumination_model: i32,
    pub channel_flags: i32,
    pub mode: i32,
    pub indirect_bump_scale: f64,
    pub reflectance_scale: f64,
    pub transmittance_scale: f64,
    pub two_sided_material: bool,
    pub luminance: f64,
    pub luminance_mode: i16,
    pub normal_map_method: i16,
    pub normal_map_strength: f64,
    pub normal_map: MaterialMap,
    pub is_anonymous: bool,
    pub global_illumination: i16,
    pub final_gather: i16,
    pub color_bleed_scale: f64,
    /// Whether the optional advanced-material tail existed in the source.
    pub advanced_data_present: bool,
}

impl Material {
    /// Create a new Material with defaults
    pub fn new() -> Self {
        Material {
            dxf_source: None,
            handle: Handle::NULL,
            owner: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            name: String::new(),
            description: String::new(),
            ambient_color: MaterialColor::default(),
            diffuse_color: MaterialColor::default(),
            diffuse_map: MaterialMap::default(),
            specular_color: MaterialColor::default(),
            specular_map: MaterialMap::default(),
            specular_gloss_factor: 0.5,
            reflection_map: MaterialMap::default(),
            opacity_percent: 1.0,
            opacity_map: MaterialMap::default(),
            bump_map: MaterialMap::default(),
            refraction_index: 1.0,
            refraction_map: MaterialMap::default(),
            translucence: 0.0,
            self_illumination: 0.0,
            reflectivity: 0.0,
            illumination_model: 0,
            channel_flags: 127,
            mode: 0,
            indirect_bump_scale: 1.0,
            reflectance_scale: 1.0,
            transmittance_scale: 1.0,
            two_sided_material: false,
            luminance: 0.0,
            luminance_mode: 0,
            normal_map_method: 0,
            normal_map_strength: 1.0,
            normal_map: MaterialMap::default(),
            is_anonymous: false,
            global_illumination: 0,
            final_gather: 0,
            color_bleed_scale: 1.0,
            advanced_data_present: false,
        }
    }

    pub(crate) fn has_advanced_data(&self) -> bool {
        self.advanced_data_present
            || self.indirect_bump_scale != 1.0
            || self.reflectance_scale != 1.0
            || self.transmittance_scale != 1.0
            || self.two_sided_material
            || self.luminance != 0.0
            || self.luminance_mode != 0
            || self.normal_map_method != 0
            || self.normal_map_strength != 1.0
            || self.normal_map != MaterialMap::default()
            || self.is_anonymous
            || self.global_illumination != 0
            || self.final_gather != 0
            || self.color_bleed_scale != 1.0
    }
}

impl PartialEq for Material {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
            && self.owner == other.owner
            && self.reactors == other.reactors
            && self.xdictionary_handle == other.xdictionary_handle
            && self.name == other.name
            && self.description == other.description
            && self.ambient_color == other.ambient_color
            && self.diffuse_color == other.diffuse_color
            && self.diffuse_map == other.diffuse_map
            && self.specular_color == other.specular_color
            && self.specular_map == other.specular_map
            && self.specular_gloss_factor == other.specular_gloss_factor
            && self.reflection_map == other.reflection_map
            && self.opacity_percent == other.opacity_percent
            && self.opacity_map == other.opacity_map
            && self.bump_map == other.bump_map
            && self.refraction_index == other.refraction_index
            && self.refraction_map == other.refraction_map
            && self.translucence == other.translucence
            && self.self_illumination == other.self_illumination
            && self.reflectivity == other.reflectivity
            && self.illumination_model == other.illumination_model
            && self.channel_flags == other.channel_flags
            && self.mode == other.mode
            && self.indirect_bump_scale == other.indirect_bump_scale
            && self.reflectance_scale == other.reflectance_scale
            && self.transmittance_scale == other.transmittance_scale
            && self.two_sided_material == other.two_sided_material
            && self.luminance == other.luminance
            && self.luminance_mode == other.luminance_mode
            && self.normal_map_method == other.normal_map_method
            && self.normal_map_strength == other.normal_map_strength
            && self.normal_map == other.normal_map
            && self.is_anonymous == other.is_anonymous
            && self.global_illumination == other.global_illumination
            && self.final_gather == other.final_gather
            && self.color_bleed_scale == other.color_bleed_scale
            && self.advanced_data_present == other.advanced_data_present
    }
}

impl Default for Material {
    fn default() -> Self {
        Self::new()
    }
}

/// GeoData — geographic location data for a drawing (AcDbGeoData).
///
/// Carries the drawing's georeference, most importantly the coordinate-system
/// definition (a MapGuide coordinate-system XML string on R2010+; a WKT PROJCS
/// string on R2009).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeoDataMeshPoint {
    pub source: Vector2,
    pub destination: Vector2,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeoDataMeshFace {
    pub first: i32,
    pub second: i32,
    pub third: i32,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct GeoData {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Persistent reactor handles.
    pub reactors: Vec<Handle>,
    /// Extension dictionary handle.
    pub xdictionary_handle: Option<Handle>,
    /// Object version (code 90): 1 = R2009, 2 = R2010, 3 = R2013
    pub version: i32,
    /// Soft pointer to the host block record
    pub host_block: Handle,
    /// Coordinate type (code 70): 0 = unknown, 1 = local grid, 2 = projected grid, 3 = geographic
    pub coordinate_type: i16,
    /// Design point (WCS) (codes 10/20/30)
    pub design_point: Vector3,
    /// Reference point (geographic/projected) (codes 11/21/31)
    pub reference_point: Vector3,
    /// R2009 observation point retained for binary round-trip.
    pub obsolete_observation_point: Vector3,
    /// R2009 scale vector retained for binary round-trip.
    pub obsolete_scale_vector: Vector3,
    /// North direction vector (codes 12/22)
    pub north_direction: Vector2,
    /// Up direction (codes 210/220/230)
    pub up_direction: Vector3,
    /// Horizontal unit scale (code 40)
    pub horizontal_unit_scale: f64,
    /// Vertical unit scale (code 41)
    pub vertical_unit_scale: f64,
    /// Horizontal units (code 91)
    pub horizontal_units: i32,
    /// Vertical units (code 92)
    pub vertical_units: i32,
    /// Scale estimation method (code 95)
    pub scale_estimation_method: i32,
    /// User-specified scale factor (code 141)
    pub user_scale_factor: f64,
    /// Enable sea-level correction (code 294)
    pub sea_level_correction: bool,
    /// Sea-level elevation (code 142)
    pub sea_level_elevation: f64,
    /// Coordinate projection radius (code 143)
    pub coordinate_projection_radius: f64,
    /// Coordinate system definition (code 301): MapGuide XML (R2010+) or WKT (R2009)
    pub coordinate_system_definition: String,
    /// Source DXF codes whose presence distinguishes omitted values from defaults.
    /// Mesh coordinate codes are excluded; `None` means no DXF source metadata.
    pub dxf_present_codes: Option<Vec<i32>>,
    /// Source 301/303 text before caret-escape decoding, in source order.
    /// Kept separately from the native decoded definition and R2009 datum.
    pub raw_dxf_definition: Option<String>,
    /// R2009 obsolete coordinate-system datum string (code 303).
    pub coordinate_system_datum: String,
    /// R2009 obsolete coordinate-system WKT string (code 304).
    pub coordinate_system_wkt: String,
    /// Geo RSS tag (code 302)
    pub geo_rss_tag: String,
    /// Observation-from tag (code 305)
    pub observation_from_tag: String,
    /// Observation-to tag (code 306)
    pub observation_to_tag: String,
    /// Observation-coverage tag (code 307)
    pub observation_coverage_tag: String,
    /// Geographic transformation mesh control points (codes 13/14).
    pub mesh_points: Vec<GeoDataMeshPoint>,
    /// Geographic transformation mesh triangle indices (codes 97/98/99).
    pub mesh_faces: Vec<GeoDataMeshFace>,
    /// R2009 Civil3D extension data following the geographic mesh.
    pub civil_data_present: bool,
    pub civil_obsolete_flag: bool,
    pub civil_reference_point1: Vector2,
    pub civil_reference_point2: Vector2,
    pub civil_unknown1: i32,
    pub civil_unknown2: i32,
    pub civil_unknown_flag1: bool,
    pub civil_zero_point1: Vector2,
    pub civil_zero_point2: Vector2,
    pub civil_unknown_flag2: bool,
    pub civil_north_angle_degrees: f64,
    pub civil_north_angle_radians: f64,
}

impl GeoData {
    /// Create a new GeoData
    pub fn new() -> Self {
        GeoData {
            handle: Handle::NULL,
            owner: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            version: 2,
            host_block: Handle::NULL,
            coordinate_type: 0,
            design_point: Vector3::default(),
            reference_point: Vector3::default(),
            obsolete_observation_point: Vector3::default(),
            obsolete_scale_vector: Vector3::new(1.0, 1.0, 1.0),
            north_direction: Vector2::default(),
            up_direction: Vector3::default(),
            horizontal_unit_scale: 1.0,
            vertical_unit_scale: 1.0,
            horizontal_units: 0,
            vertical_units: 0,
            scale_estimation_method: 0,
            user_scale_factor: 1.0,
            sea_level_correction: false,
            sea_level_elevation: 0.0,
            coordinate_projection_radius: 0.0,
            coordinate_system_definition: String::new(),
            dxf_present_codes: None,
            raw_dxf_definition: None,
            coordinate_system_datum: String::new(),
            coordinate_system_wkt: String::new(),
            geo_rss_tag: String::new(),
            observation_from_tag: String::new(),
            observation_to_tag: String::new(),
            observation_coverage_tag: String::new(),
            mesh_points: Vec::new(),
            mesh_faces: Vec::new(),
            civil_data_present: false,
            civil_obsolete_flag: false,
            civil_reference_point1: Vector2::default(),
            civil_reference_point2: Vector2::default(),
            civil_unknown1: 0,
            civil_unknown2: 0,
            civil_unknown_flag1: false,
            civil_zero_point1: Vector2::default(),
            civil_zero_point2: Vector2::default(),
            civil_unknown_flag2: false,
            civil_north_angle_degrees: 0.0,
            civil_north_angle_radians: 0.0,
        }
    }
}

impl Default for GeoData {
    fn default() -> Self {
        Self::new()
    }
}

/// SpatialFilter — the clip boundary (XCLIP) attached to a block reference.
///
/// Stored under the INSERT's extension dictionary as the `SPATIAL` entry of
/// the `ACAD_FILTER` sub-dictionary. The boundary points are 2D coordinates in
/// the clip boundary's local coordinate system; two transforms relate that
/// system to the block reference and to the world.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpatialFilter {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Clip boundary definition points (code 10/20), in the boundary's local
    /// 2D coordinate system. Two points = rectangular clip (min/max corners);
    /// three or more = polygonal clip.
    pub boundary_points: Vec<Vector2>,
    /// Normal to the plane of the clip boundary (code 210/220/230).
    pub normal: Vector3,
    /// Origin of the clip boundary local coordinate system (code 11/21/31).
    pub origin: Vector3,
    /// Clip boundary display enabled flag (code 71).
    pub display_enabled: bool,
    /// Front clipping plane distance (code 40), `Some` when the front clip
    /// flag (code 72) is set.
    pub front_clip: Option<f64>,
    /// Back clipping plane distance (code 41), `Some` when the back clip
    /// flag (code 73) is set.
    pub back_clip: Option<f64>,
    /// 4×3 matrix (column-major in DXF) transforming WCS points into the
    /// block-definition coordinate system — the inverse block transform.
    pub inverse_block_transform: Matrix4,
    /// 4×3 matrix transforming clip boundary points into the block reference
    /// coordinate system.
    pub clip_bound_transform: Matrix4,
}

impl SpatialFilter {
    /// Create a new SpatialFilter
    pub fn new() -> Self {
        SpatialFilter {
            handle: Handle::NULL,
            owner: Handle::NULL,
            boundary_points: Vec::new(),
            normal: Vector3::new(0.0, 0.0, 1.0),
            origin: Vector3::new(0.0, 0.0, 0.0),
            display_enabled: true,
            front_clip: None,
            back_clip: None,
            inverse_block_transform: Matrix4::identity(),
            clip_bound_transform: Matrix4::identity(),
        }
    }
}

impl Default for SpatialFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// RasterVariables — global raster image settings
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RasterVariables {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Class version (code 90)
    pub class_version: i32,
    /// Image frame mode (code 70): 0 = hidden, 1 = plotted, 2 = display only
    pub display_image_frame: i16,
    /// Image quality (code 71): 0 = draft, 1 = high
    pub image_quality: i16,
    /// Units (code 72): 0 = none, 1 = mm, 2 = cm, 3 = m, 4 = km, 5 = in, 6 = ft, 7 = yd, 8 = mi
    pub units: i16,
}

impl RasterVariables {
    /// Create new RasterVariables
    pub fn new() -> Self {
        RasterVariables {
            handle: Handle::NULL,
            owner: Handle::NULL,
            class_version: 0,
            display_image_frame: 1,
            image_quality: 1,
            units: 0,
        }
    }
}

impl Default for RasterVariables {
    fn default() -> Self {
        Self::new()
    }
}

/// BookColor (DBCOLOR) — named color definition
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BookColor {
    /// DXF color-code presence; None for constructed or DWG records.
    /// Provenance does not participate in native drawing-state equality.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dxf_color_present: Option<bool>,
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// RGB/ACI value stored by the DBCOLOR record
    #[cfg_attr(feature = "serde", serde(default))]
    pub color: Color,
    /// Color name (code 1)
    pub color_name: String,
    /// Book name (code 2)
    pub book_name: String,
}

impl BookColor {
    /// Create a new BookColor
    pub fn new() -> Self {
        BookColor {
            dxf_color_present: None,
            handle: Handle::NULL,
            owner: Handle::NULL,
            color: Color::default(),
            color_name: String::new(),
            book_name: String::new(),
        }
    }
}

impl PartialEq for BookColor {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
            && self.owner == other.owner
            && self.color == other.color
            && self.color_name == other.color_name
            && self.book_name == other.book_name
    }
}

impl Default for BookColor {
    fn default() -> Self {
        Self::new()
    }
}

/// AcDbPlaceHolder — placeholder object (no data beyond handle)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlaceHolder {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
}

impl PlaceHolder {
    /// Create a new PlaceHolder
    pub fn new() -> Self {
        PlaceHolder {
            handle: Handle::NULL,
            owner: Handle::NULL,
        }
    }
}

impl Default for PlaceHolder {
    fn default() -> Self {
        Self::new()
    }
}

/// DictionaryWithDefault — dictionary with a default entry handle
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DictionaryWithDefault {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Dictionary entries (key -> handle)
    pub entries: Vec<(String, Handle)>,
    /// Default entry handle (code 340)
    pub default_handle: Handle,
    /// Duplicate record cloning flag (code 281)
    pub duplicate_cloning: i16,
    /// Hard owner flag (code 280)
    pub hard_owner: bool,
}

impl DictionaryWithDefault {
    /// Create a new DictionaryWithDefault
    pub fn new() -> Self {
        DictionaryWithDefault {
            handle: Handle::NULL,
            owner: Handle::NULL,
            entries: Vec::new(),
            default_handle: Handle::NULL,
            duplicate_cloning: 1,
            hard_owner: false,
        }
    }
}

impl Default for DictionaryWithDefault {
    fn default() -> Self {
        Self::new()
    }
}

/// WipeoutVariables — global wipeout display settings
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WipeoutVariables {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Wipeout frame mode (code 70): 0 = hidden, 1 = displayed and plotted,
    /// 2 = displayed but not plotted.
    pub display_frame: i16,
}

impl WipeoutVariables {
    /// Create new WipeoutVariables
    pub fn new() -> Self {
        WipeoutVariables {
            handle: Handle::NULL,
            owner: Handle::NULL,
            display_frame: 1,
        }
    }
}

impl Default for WipeoutVariables {
    fn default() -> Self {
        Self::new()
    }
}

// StubObject implementations for types that only need handle + owner parsing

macro_rules! impl_stub_object {
    ($ty:ident) => {
        impl StubObject for $ty {
            fn new_stub() -> Self {
                Self::new()
            }
            fn set_handle(&mut self, handle: Handle) {
                self.handle = handle;
            }
            fn set_owner(&mut self, owner: Handle) {
                self.owner = owner;
            }
            fn handle(&self) -> Handle {
                self.handle
            }
        }
    };
}

impl_stub_object!(GeoData);
impl_stub_object!(PlaceHolder);
