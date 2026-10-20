// SPDX-License-Identifier: MPL-2.0
//! Lazy native decoders for the four AEC records retained as opaque DWG data.
//!
//! AEC payloads are deliberately kept on [`ObjectType::Unknown`] and
//! [`UnknownEntity`] so the document can round-trip them without knowing the
//! proprietary schema.  This module decodes one retained record on demand;
//! it never searches the document or resolves object references.

use crate::entities::UnknownEntity;
use crate::error::{DxfError, Result};
use crate::io::dwg::dwg_stream_readers::object_reader::{
    DwgObjectReader, EntityCommonData, NonEntityCommonData,
};
use crate::io::dxf::code_page::LegacyEncoding;
use crate::objects::ObjectType;
use crate::types::{Color, Handle, Transparency};
use std::collections::HashMap;
use std::sync::Arc;

const AEC_CLEANUP_GROUP: &str = "AEC_CLEANUP_GROUP_DEF";
const AEC_WALL_STYLE: &str = "AEC_WALL_STYLE";
const AEC_BIN_RECORD: &str = "BINRECORD";
const AEC_WALL: &str = "AEC_WALL";

/// Common fields decoded from an AEC non-entity preamble.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AecObjectCommon {
    pub type_code: i16,
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub has_ds_data: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub eed_raw: Vec<(u64, Vec<u8>)>,
}

/// Common fields decoded from an AEC entity preamble.
///
/// The fields mirror [`EntityCommonData`] but use the public model's
/// [`Handle`] type for references.  Keeping the full preamble here lets a
/// caller inspect an AEC wall without needing the original `UnknownEntity`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AecEntityCommon {
    pub type_code: i16,
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub has_ds_data: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub eed_raw: Vec<(u64, Vec<u8>)>,
    pub has_graphic: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub graphic_data: Option<Vec<u8>>,
    pub entity_mode: u8,
    pub color: Color,
    pub transparency: Transparency,
    pub color_source: Option<(u16, u32)>,
    pub line_weight: u8,
    pub linetype_scale: f64,
    pub invisible: bool,
    pub layer_handle: Handle,
    pub linetype_flags: u8,
    pub linetype_handle: Handle,
    pub color_book_handle: Option<Handle>,
    pub prev_entity_handle: Option<Handle>,
    pub next_entity_handle: Option<Handle>,
    pub material_flags: u8,
    pub material_handle: Option<Handle>,
    pub shadow_flags: u8,
    pub plotstyle_flags: u8,
    pub plotstyle_handle: Option<Handle>,
    pub full_visual_style_handle: Option<Handle>,
    pub face_visual_style_handle: Option<Handle>,
    pub edge_visual_style_handle: Option<Handle>,
}

/// AEC cleanup-group object decoded from one retained DWG record.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AecCleanupGroup {
    pub common: AecObjectCommon,
    pub version: i32,
    pub description: String,
    /// Opaque bits after the description, packed MSB-first.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_data: Vec<u8>,
    /// Number of valid bits in `raw_data`; the final byte can be partial.
    pub raw_data_bits: i64,
    pub raw_data_start_bit: i64,
    pub raw_data_end_bit: i64,
}

/// AEC wall-style object decoded from one retained DWG record.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AecWallStyle {
    pub common: AecObjectCommon,
    pub version: i32,
    pub description: String,
    /// Opaque bits after the description, packed MSB-first.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_data: Vec<u8>,
    pub raw_data_bits: i64,
    pub raw_data_start_bit: i64,
    pub raw_data_end_bit: i64,
}

/// AEC binary-record object decoded from one retained DWG record.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AecBinRecord {
    pub common: AecObjectCommon,
    pub version: i32,
    /// Opaque bits after the version, packed MSB-first.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub binary_data: Vec<u8>,
    pub binary_data_bits: i64,
    pub binary_data_start_bit: i64,
    pub binary_data_end_bit: i64,
}

/// AEC wall entity decoded from one retained DWG record.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AecWall {
    pub common: AecEntityCommon,
    pub version: i32,
    /// The first AEC wall handle reference. ACadSharp skips this value.
    pub first_reference: Handle,
    pub style_handle: Handle,
    pub cleanup_group_handle: Handle,
    /// Opaque main/text bits before the handle stream, packed MSB-first.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_data: Vec<u8>,
    pub raw_data_bits: i64,
    pub raw_data_start_bit: i64,
    pub raw_data_end_bit: i64,
    /// Handle-stream range consumed by the three AEC references.
    pub references_start_bit: i64,
    pub references_end_bit: i64,
}

/// One of the four AEC types with a native lazy decoder.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AecData {
    CleanupGroup(AecCleanupGroup),
    WallStyle(AecWallStyle),
    BinRecord(AecBinRecord),
    Wall(AecWall),
}

impl AecData {
    /// Decode a retained AEC object using Windows-1252 for legacy text.
    pub fn from_object(object: &ObjectType) -> Result<Option<Self>> {
        Self::from_object_with_encoding(object, encoding_rs::WINDOWS_1252)
    }

    /// Decode a retained AEC object using an explicit legacy text encoding.
    pub fn from_object_with_encoding(
        object: &ObjectType,
        encoding: impl Into<LegacyEncoding>,
    ) -> Result<Option<Self>> {
        let (name, raw, handle_bits, version) = match object {
            ObjectType::Unknown {
                type_name,
                raw_dwg_data,
                raw_dwg_handle_bits,
                raw_dwg_version,
                ..
            } => (
                type_name.as_str(),
                raw_dwg_data.as_deref(),
                *raw_dwg_handle_bits,
                *raw_dwg_version,
            ),
            _ => return Ok(None),
        };
        let Some(kind) = object_kind(name) else {
            return Ok(None);
        };
        let (Some(raw), Some(version)) = (raw, version) else {
            return Ok(None);
        };
        validate_record_inputs(raw, handle_bits, version)?;
        let encoding = encoding.into();
        let reader = DwgObjectReader::with_encoding(Vec::new(), version, HashMap::new(), encoding)?;
        let merged_data: Arc<[u8]> = Arc::from(raw);
        let (type_code, mut merged) =
            reader.read_merged_record(Arc::clone(&merged_data), handle_bits)?;
        match kind {
            AecKind::CleanupGroup => {
                decode_cleanup_group(&reader, &merged_data, &mut merged, type_code)
            }
            AecKind::WallStyle => decode_wall_style(&reader, &merged_data, &mut merged, type_code),
            AecKind::BinRecord => decode_bin_record(&reader, &merged_data, &mut merged, type_code),
            AecKind::Wall => Ok(None),
        }
    }

    /// Decode a retained AEC wall entity using Windows-1252 for legacy text.
    pub fn from_entity(entity: &UnknownEntity) -> Result<Option<Self>> {
        Self::from_entity_with_encoding(entity, encoding_rs::WINDOWS_1252)
    }

    /// Decode a retained AEC wall entity using an explicit legacy encoding.
    pub fn from_entity_with_encoding(
        entity: &UnknownEntity,
        encoding: impl Into<LegacyEncoding>,
    ) -> Result<Option<Self>> {
        if !entity.dxf_name.eq_ignore_ascii_case(AEC_WALL) {
            return Ok(None);
        }
        let (Some(raw), Some(version)) =
            (entity.raw_dwg_data.as_deref(), entity.dwg_source_version)
        else {
            return Ok(None);
        };
        validate_record_inputs(raw, entity.dwg_handle_bits, version)?;
        let reader =
            DwgObjectReader::with_encoding(Vec::new(), version, HashMap::new(), encoding.into())?;
        let merged_data: Arc<[u8]> = Arc::from(raw);
        let (type_code, mut merged) =
            reader.read_merged_record(Arc::clone(&merged_data), entity.dwg_handle_bits)?;
        decode_wall(&reader, &merged_data, &mut merged, type_code)
            .map(|wall| Some(AecData::Wall(wall)))
    }
}

#[derive(Clone, Copy)]
enum AecKind {
    CleanupGroup,
    WallStyle,
    BinRecord,
    Wall,
}

fn object_kind(name: &str) -> Option<AecKind> {
    if name.eq_ignore_ascii_case(AEC_CLEANUP_GROUP) {
        Some(AecKind::CleanupGroup)
    } else if name.eq_ignore_ascii_case(AEC_WALL_STYLE) {
        Some(AecKind::WallStyle)
    } else if name.eq_ignore_ascii_case(AEC_BIN_RECORD) {
        Some(AecKind::BinRecord)
    } else if name.eq_ignore_ascii_case(AEC_WALL) {
        Some(AecKind::Wall)
    } else {
        None
    }
}

fn validate_record_inputs(
    raw: &[u8],
    handle_bits: i64,
    version: crate::types::DxfVersion,
) -> Result<()> {
    if version == crate::types::DxfVersion::AC1009 {
        return Err(DxfError::UnsupportedVersion(
            "AEC lazy decoding does not support the AC1009 object layout".into(),
        ));
    }
    if version >= crate::types::DxfVersion::AC1024 {
        let total_bits = i64::try_from(raw.len())
            .map_err(|_| DxfError::Parse("DWG object data length overflows bit count".into()))?
            .checked_mul(8)
            .ok_or_else(|| DxfError::Parse("DWG object data length overflows bit count".into()))?;
        if handle_bits < 0 || handle_bits > total_bits {
            return Err(DxfError::Parse(format!(
                "Invalid AEC handle-stream bit count {handle_bits} for {total_bits}-bit record"
            )));
        }
    }
    Ok(())
}

fn decode_cleanup_group(
    reader: &DwgObjectReader,
    raw: &[u8],
    merged: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
    type_code: i16,
) -> Result<Option<AecData>> {
    let common = reader.read_common_non_entity_data(merged, type_code);
    let version = if reader.version().r2000_plus() {
        merged.read_bit_long()
    } else {
        0
    };
    let description = merged.read_variable_text();
    let start = merged.position_in_bits();
    let end = record_end_bits(merged)?;
    let (raw_data, raw_data_bits, raw_data_start_bit, raw_data_end_bit) =
        copy_bits(raw, start, end)?;
    merged.check_error()?;
    Ok(Some(AecData::CleanupGroup(AecCleanupGroup {
        common: object_common(common),
        version,
        description,
        raw_data,
        raw_data_bits,
        raw_data_start_bit,
        raw_data_end_bit,
    })))
}

fn decode_wall_style(
    reader: &DwgObjectReader,
    raw: &[u8],
    merged: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
    type_code: i16,
) -> Result<Option<AecData>> {
    let common = reader.read_common_non_entity_data(merged, type_code);
    let version = if reader.version().r2000_plus() {
        merged.read_bit_long()
    } else {
        0
    };
    let description = merged.read_variable_text();
    let start = merged.position_in_bits();
    let end = record_end_bits(merged)?;
    let (raw_data, raw_data_bits, raw_data_start_bit, raw_data_end_bit) =
        copy_bits(raw, start, end)?;
    merged.check_error()?;
    Ok(Some(AecData::WallStyle(AecWallStyle {
        common: object_common(common),
        version,
        description,
        raw_data,
        raw_data_bits,
        raw_data_start_bit,
        raw_data_end_bit,
    })))
}

fn decode_bin_record(
    reader: &DwgObjectReader,
    raw: &[u8],
    merged: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
    type_code: i16,
) -> Result<Option<AecData>> {
    let common = reader.read_common_non_entity_data(merged, type_code);
    let version = if reader.version().r2000_plus() {
        merged.read_bit_long()
    } else {
        0
    };
    let start = merged.position_in_bits();
    let end = record_end_bits(merged)?;
    let (binary_data, binary_data_bits, binary_data_start_bit, binary_data_end_bit) =
        copy_bits(raw, start, end)?;
    merged.check_error()?;
    Ok(Some(AecData::BinRecord(AecBinRecord {
        common: object_common(common),
        version,
        binary_data,
        binary_data_bits,
        binary_data_start_bit,
        binary_data_end_bit,
    })))
}

fn decode_wall(
    reader: &DwgObjectReader,
    raw: &[u8],
    merged: &mut crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
    type_code: i16,
) -> Result<AecWall> {
    let common = reader.read_common_entity_data(merged, type_code);
    let version = if reader.version().r2000_plus() {
        merged.read_bit_long()
    } else {
        0
    };

    // Native DWG handle references are relative to the current object's
    // handle. The first value is deliberately skipped by ACadSharp but is
    // retained here so a caller can reproduce that compatibility quirk.
    let total_bits = record_end_bits(merged)?;
    let references_start = total_bits
        .checked_sub(merged.handle_remaining_bits())
        .ok_or_else(|| DxfError::Parse("AEC wall handle cursor underflows record".into()))?;
    let first_reference = merged.read_handle();
    let style_handle = merged.read_handle();
    let cleanup_group_handle = merged.read_handle();
    let references_end = total_bits
        .checked_sub(merged.handle_remaining_bits())
        .ok_or_else(|| DxfError::Parse("AEC wall handle cursor underflows record".into()))?;
    let start = merged.position_in_bits();
    let end = merged.handle_start();
    let (raw_data, raw_data_bits, raw_data_start_bit, raw_data_end_bit) =
        copy_bits(raw, start, end)?;
    merged.check_error()?;

    Ok(AecWall {
        common: entity_common(common),
        version,
        first_reference: Handle::from(first_reference),
        style_handle: Handle::from(style_handle),
        cleanup_group_handle: Handle::from(cleanup_group_handle),
        raw_data,
        raw_data_bits,
        raw_data_start_bit,
        raw_data_end_bit,
        references_start_bit: references_start,
        references_end_bit: references_end,
    })
}

fn object_common(common: NonEntityCommonData) -> AecObjectCommon {
    AecObjectCommon {
        type_code: common.common.type_code,
        handle: Handle::from(common.common.handle),
        owner: Handle::from(common.owner_handle),
        reactors: common.reactors.into_iter().map(Handle::from).collect(),
        xdictionary_handle: common.xdictionary_handle.map(Handle::from),
        has_ds_data: common.has_ds_data,
        eed_raw: common.common.eed_raw,
    }
}

fn entity_common(common: EntityCommonData) -> AecEntityCommon {
    AecEntityCommon {
        type_code: common.common.type_code,
        handle: Handle::from(common.common.handle),
        owner: Handle::from(common.owner_handle),
        reactors: common.reactors.into_iter().map(Handle::from).collect(),
        xdictionary_handle: common.xdictionary_handle.map(Handle::from),
        has_ds_data: common.has_ds_data,
        eed_raw: common.common.eed_raw,
        has_graphic: common.has_graphic,
        graphic_data: common.graphic_data,
        entity_mode: common.entity_mode,
        color: common.color,
        transparency: common.transparency,
        color_source: common.color_source,
        line_weight: common.line_weight,
        linetype_scale: common.linetype_scale,
        invisible: common.invisible,
        layer_handle: Handle::from(common.layer_handle),
        linetype_flags: common.linetype_flags,
        linetype_handle: Handle::from(common.linetype_handle),
        color_book_handle: common.color_book_handle.map(Handle::from),
        prev_entity_handle: common.prev_entity_handle.map(Handle::from),
        next_entity_handle: common.next_entity_handle.map(Handle::from),
        material_flags: common.material_flags,
        material_handle: common.material_handle.map(Handle::from),
        shadow_flags: common.shadow_flags,
        plotstyle_flags: common.plotstyle_flags,
        plotstyle_handle: common.plotstyle_handle.map(Handle::from),
        full_visual_style_handle: common.full_visual_style_handle.map(Handle::from),
        face_visual_style_handle: common.face_visual_style_handle.map(Handle::from),
        edge_visual_style_handle: common.edge_visual_style_handle.map(Handle::from),
    }
}

fn record_end_bits(
    merged: &crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader,
) -> Result<i64> {
    let bytes = i64::try_from(merged.main().data_len())
        .map_err(|_| DxfError::Parse("DWG object data length overflows bit count".into()))?;
    bytes
        .checked_mul(8)
        .ok_or_else(|| DxfError::Parse("DWG object data length overflows bit count".into()))
}

/// Copy an arbitrary bit range from the retained merged bytes.
///
/// The stream readers are MSB-first.  Copying by bit rather than rounding to
/// bytes preserves a trailing partial byte exactly and keeps malformed bounds
/// from turning into an allocation or slice panic.
fn copy_bits(data: &[u8], start: i64, end: i64) -> Result<(Vec<u8>, i64, i64, i64)> {
    let total = i64::try_from(data.len())
        .map_err(|_| DxfError::Parse("DWG object data length overflows bit count".into()))?
        .checked_mul(8)
        .ok_or_else(|| DxfError::Parse("DWG object data length overflows bit count".into()))?;
    if start < 0 || end < start || end > total {
        return Err(DxfError::Parse(format!(
            "Invalid AEC raw-data bit range {start}..{end} for {total}-bit record"
        )));
    }
    let bit_count = end - start;
    let byte_count = usize::try_from(
        bit_count
            .checked_add(7)
            .ok_or_else(|| DxfError::Parse("AEC raw-data bit count overflows".into()))?
            / 8,
    )
    .map_err(|_| DxfError::Parse("AEC raw-data byte count overflows".into()))?;
    let mut out = vec![0u8; byte_count];
    for bit in 0..bit_count {
        let source = usize::try_from(start + bit)
            .map_err(|_| DxfError::Parse("AEC raw-data bit offset overflows".into()))?;
        let source_byte = data[source / 8];
        let value = (source_byte >> (7 - (source % 8))) & 1;
        if value != 0 {
            let target = usize::try_from(bit)
                .map_err(|_| DxfError::Parse("AEC raw-data bit offset overflows".into()))?;
            out[target / 8] |= 1 << (7 - (target % 8));
        }
    }
    Ok((out, bit_count, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::UnknownEntity;
    use crate::io::dwg::dwg_stream_writers::merged_writer::DwgMergedWriter;
    use crate::io::dwg::dwg_version::DwgVersion;
    use crate::types::DxfVersion;

    fn synthetic_object(
        type_name: &str,
        dxf_version: DxfVersion,
        version: i32,
        tail_bits: &[bool],
    ) -> ObjectType {
        let dwg_version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        let handle = Handle::new(0x123);
        let mut writer = DwgMergedWriter::new(dwg_version, dxf_version);
        writer.write_object_type(500);
        if dwg_version.r2000_plus() && !dwg_version.r2010_plus() {
            writer.save_position_for_size();
        }
        writer.main_mut().write_handle_undefined(handle.value());
        writer.write_bit_short(0); // no EED
        writer.write_handle_undefined(0); // owner
        writer.write_bit_long(0); // no reactors
        if dwg_version.r2004_plus() {
            writer.write_bit(true); // no extension dictionary
        } else {
            writer.write_handle_undefined(0);
        }
        if dxf_version >= DxfVersion::AC1027 {
            writer.write_bit(false); // no AcDs data
        }
        writer.write_bit_long(version);
        if type_name != AEC_BIN_RECORD {
            writer.write_variable_text("AEC description");
        }
        for &bit in tail_bits {
            writer.main_mut().write_bit(bit);
        }
        let raw = writer.merge();
        let handle_bits = if dwg_version.r2010_plus() {
            raw.len() as i64 * 8 - writer.handle_start_bits()
        } else {
            0
        };
        ObjectType::Unknown {
            type_name: type_name.into(),
            handle,
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: Some(raw),
            raw_dwg_handle_bits: handle_bits,
            raw_dwg_version: Some(dxf_version),
        }
    }

    fn truncated_text_object(dxf_version: DxfVersion, claimed_length: i16) -> ObjectType {
        let dwg_version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        let handle = Handle::new(0x123);
        let mut writer = DwgMergedWriter::new(dwg_version, dxf_version);
        writer.write_object_type(500);
        if dwg_version.r2000_plus() && !dwg_version.r2010_plus() {
            writer.save_position_for_size();
        }
        writer.main_mut().write_handle_undefined(handle.value());
        writer.write_bit_short(0); // no EED
        writer.write_handle_undefined(0); // owner
        writer.write_bit_long(0); // no reactors
        if dwg_version.r2004_plus() {
            writer.write_bit(true); // no extension dictionary
        } else {
            writer.write_handle_undefined(0);
        }
        if dxf_version >= DxfVersion::AC1027 {
            writer.write_bit(false); // no AcDs data
        }
        writer.write_bit_long(80);
        if dwg_version.r2007_plus() {
            writer.text_mut().write_bit_short(claimed_length);
        } else {
            writer.main_mut().write_bit_short(claimed_length);
        }
        let raw = writer.merge();
        let handle_bits = if dwg_version.r2010_plus() {
            raw.len() as i64 * 8 - writer.handle_start_bits()
        } else {
            0
        };
        ObjectType::Unknown {
            type_name: AEC_WALL_STYLE.into(),
            handle,
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: Some(raw),
            raw_dwg_handle_bits: handle_bits,
            raw_dwg_version: Some(dxf_version),
        }
    }

    #[test]
    fn source_version_controls_aec_version_and_text_layout() {
        let legacy = synthetic_object(
            AEC_CLEANUP_GROUP,
            DxfVersion::AC1015,
            17,
            &[true, false, true, true, false],
        );
        let AecData::CleanupGroup(legacy) = AecData::from_object(&legacy).unwrap().unwrap() else {
            panic!("expected cleanup group");
        };
        assert_eq!(legacy.version, 17);
        assert_eq!(legacy.description, "AEC description");
        assert_eq!(
            legacy.raw_data_bits,
            legacy.raw_data_end_bit - legacy.raw_data_start_bit
        );
        assert!(legacy.raw_data_bits > 0);

        let unicode = synthetic_object(
            AEC_WALL_STYLE,
            DxfVersion::AC1021,
            80,
            &[false, true, false],
        );
        let AecData::WallStyle(unicode) = AecData::from_object(&unicode).unwrap().unwrap() else {
            panic!("expected wall style");
        };
        assert_eq!(unicode.version, 80);
        assert_eq!(unicode.description, "AEC description");
    }

    #[test]
    fn wall_resolves_relative_handles_against_object_handle() {
        let dxf_version = DxfVersion::AC1032;
        let dwg_version = DwgVersion::from_dxf_version(dxf_version).unwrap();
        let object = 0x100;
        let owner = 0x200;
        let style = 0x80;
        let cleanup = 0x120;
        let mut writer = DwgMergedWriter::new(dwg_version, dxf_version);
        writer.write_object_type(500);
        writer.main_mut().write_handle_undefined(object);
        writer.write_bit_short(0); // no EED
        writer.write_bit(false); // no graphic data
        writer.write_2bits(0); // entity mode: owned
        writer.write_handle_undefined(owner);
        writer.write_bit_long(0); // no reactors
        writer.write_bit(true); // no extension dictionary
        writer.write_bit(false); // no AcDs data
        writer.write_en_color(&Color::ByLayer, &Transparency::default());
        writer.write_bit_double(1.0);
        writer.write_handle_undefined(0x10); // layer
        writer.write_2bits(0); // linetype by layer
        writer.write_2bits(0); // material by layer
        writer.write_byte(0); // shadow flags
        writer.write_2bits(0); // plotstyle by layer
        writer.write_bit(false);
        writer.write_bit(false);
        writer.write_bit(false);
        writer.write_bit_short(0); // visible
        writer.write_byte(0); // lineweight
        writer.write_bit_long(80);
        writer.write_handle_relative(object, object + 1);
        writer.write_handle_relative(object, style);
        writer.write_handle_relative(object, cleanup);
        let raw = writer.merge();
        let handle_bits = raw.len() as i64 * 8 - writer.handle_start_bits();

        let mut entity = UnknownEntity::new(AEC_WALL);
        entity.raw_dwg_data = Some(raw);
        entity.dwg_handle_bits = handle_bits;
        entity.dwg_source_version = Some(dxf_version);
        let AecData::Wall(wall) = AecData::from_entity(&entity).unwrap().unwrap() else {
            panic!("expected wall");
        };
        assert_eq!(wall.common.handle, Handle::new(object));
        assert_eq!(wall.first_reference, Handle::new(object + 1));
        assert_eq!(wall.style_handle, Handle::new(style));
        assert_eq!(wall.cleanup_group_handle, Handle::new(cleanup));
        assert!(wall.references_end_bit > wall.references_start_bit);
    }

    #[test]
    fn invalid_r2010_handle_bits_are_rejected_before_stream_split() {
        let mut object = synthetic_object(AEC_BIN_RECORD, DxfVersion::AC1032, 0, &[]);
        if let ObjectType::Unknown {
            raw_dwg_handle_bits,
            ..
        } = &mut object
        {
            *raw_dwg_handle_bits = i64::MIN;
        }
        let error = AecData::from_object(&object).unwrap_err();
        assert!(error.to_string().contains("handle-stream bit count"));
    }

    #[test]
    fn ac1009_aec_records_are_rejected() {
        let object = ObjectType::Unknown {
            type_name: AEC_BIN_RECORD.into(),
            handle: Handle::new(1),
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: Some(vec![0; 8]),
            raw_dwg_handle_bits: 0,
            raw_dwg_version: Some(DxfVersion::AC1009),
        };
        assert!(matches!(
            AecData::from_object(&object),
            Err(DxfError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn r14_aec_records_use_zero_version_without_desynchronizing_handles() {
        let dxf_version = DxfVersion::AC1014;
        let mut writer = DwgMergedWriter::new(DwgVersion::AC12, dxf_version);
        writer.write_object_type(500);
        writer.main_mut().write_handle_undefined(0x123);
        writer.write_bit_short(0); // no EED
        writer.save_position_for_size(); // R14 main-size field before owner
        writer.write_handle_undefined(0); // owner
        writer.write_bit_long(0); // no reactors
        writer.write_handle_undefined(0); // no extension dictionary
        writer.write_variable_text("R14 AEC");
        let object = ObjectType::Unknown {
            type_name: AEC_CLEANUP_GROUP.into(),
            handle: Handle::new(0x123),
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: Some(writer.merge()),
            raw_dwg_handle_bits: 0,
            raw_dwg_version: Some(dxf_version),
        };
        let AecData::CleanupGroup(value) = AecData::from_object(&object).unwrap().unwrap() else {
            panic!("expected cleanup group");
        };
        assert_eq!(value.version, 0);
        assert_eq!(value.description, "R14 AEC");
    }

    #[test]
    fn truncated_and_oversized_text_is_reported_as_parse_error() {
        for (version, claimed_length) in [
            (DxfVersion::AC1015, 3),
            (DxfVersion::AC1021, 3),
            (DxfVersion::AC1021, i16::MAX),
        ] {
            let object = truncated_text_object(version, claimed_length);
            assert!(matches!(
                AecData::from_object(&object),
                Err(DxfError::Parse(_))
            ));
        }
    }

    #[test]
    fn copy_bits_rejects_invalid_ranges() {
        for (start, end) in [(-1, 0), (4, 3), (0, 17)] {
            assert!(matches!(
                copy_bits(&[0; 2], start, end),
                Err(DxfError::Parse(_))
            ));
        }
    }

    #[test]
    fn copy_bits_preserves_unaligned_partial_ranges() {
        let (data, bits, start, end) = copy_bits(&[0b1011_0011, 0b0110_0000], 3, 13).unwrap();
        assert_eq!(bits, 10);
        assert_eq!((start, end), (3, 13));
        assert_eq!(data, vec![0b1001_1011, 0]);
    }

    #[test]
    fn non_aec_objects_and_entities_are_ignored() {
        let object = ObjectType::Unknown {
            type_name: "ACAD_PROXY_OBJECT".into(),
            handle: Handle::new(1),
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: None,
            raw_dwg_handle_bits: 0,
            raw_dwg_version: None,
        };
        assert_eq!(AecData::from_object(&object).unwrap(), None);
        assert_eq!(
            AecData::from_entity(&UnknownEntity::new("LINE")).unwrap(),
            None
        );
    }
}
