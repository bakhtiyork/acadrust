// SPDX-License-Identifier: MPL-2.0
//! The values ACadSharp reads for native data, as plain values. The dump projects them into its JSON and the SVG
//! writer draws them when `SvgConfiguration::acadsharp_view` is on, so both read a document the same way through
//! one implementation of each rule. Native values stay intact; these functions only compute the reading.

use crate::document::CadDocument;
use crate::entities::text::TextSource;
use crate::entities::{Dimension, EntityCommon, EntityType, MText, SplineEdge};
use crate::tables::BlockRecord;
use crate::types::{Color, DxfVersion, Handle};

pub(crate) use super::entities::{view_entity_color as entity_color, view_entity_line_weight as entity_line_weight};
pub(crate) use super::palette::INDEX_RGB;
pub(crate) use super::tables::{
    view_entity_handles as entity_handles, view_layer_color as layer_color,
    view_layer_line_weight as layer_line_weight,
};

/// The colour of an entity as ACadSharp reads it from a source file. A colour-book colour of a DWG leaves `Color`
/// ByBlock (the reference is kept apart), and ACadSharp's ENC reader treats every other complex colour as RGB,
/// including the method-encoded ByLayer and ACI values understood by native Rust.
pub(crate) fn source_entity_color(common: &EntityCommon) -> Color {
    let mut value = common.color;
    if let Some((word, _)) = common.dwg_color_source {
        if word & 0x4000 != 0 {
            value = Color::ByBlock;
        } else if word & 0x8000 != 0 {
            value = match value {
                Color::ByLayer | Color::ByBlock => Color::from_rgb(0, 0, 0),
                Color::Index(i) => Color::from_rgb(0, 0, i),
                other => other,
            };
        }
    }
    value
}

/// The text ACadSharp reads for a TEXT or attribute value: the file's spelling (`\U+00B0` escapes rather than the
/// decoded character) while the native value is still the decoded form of that spelling.
pub(crate) fn text_value(value: &str, source: Option<&TextSource>) -> String {
    if let Some(source) = source.filter(|s| s.decoded_value.as_deref() == Some(value)) {
        // .NET's default encoding is UTF-8; preserve native legacy fallback and
        // reproduce replacement decoding only with original undeclared bytes.
        let raw = source
            .undecoded_bytes
            .as_ref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .or_else(|| source.value.clone());
        if let Some(raw) = raw {
            return if source.dxf {
                raw.replace("^J", "\n")
                    .replace("^M", "\r")
                    .replace("^I", "\t")
                    .replace("^ ", "^")
            } else {
                raw
            };
        }
    }
    value.into()
}

/// Pre-R2007 DWG strings are decoded through the drawing's code page. A byte sequence the code page cannot decode
/// becomes U+FFFD natively (the WHATWG decoders restore an ASCII byte that follows an invalid lead byte), but .NET's
/// DBCS decoders, which ACadSharp uses, substitute one '?' for the lead byte and that following ASCII byte together.
pub(crate) fn legacy_replacement(doc: &CadDocument, text: String) -> String {
    if doc.dwg_source_version.is_some() && doc.version < DxfVersion::AC1021 && text.contains('\u{FFFD}') {
        let mut replaced = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{FFFD}' {
                replaced.push('?');
                if chars.peek().is_some_and(char::is_ascii) {
                    chars.next();
                }
            } else {
                replaced.push(c);
            }
        }
        return replaced;
    }
    text
}

/// The value ACadSharp reads for an MTEXT: the file's spelling while the native value is unchanged, and for a
/// drawing before R2004 the original text AutoCAD states in its round-trip record (`ACAD_MTEXT_RT`).
pub(crate) fn mtext_value(doc: &CadDocument, text: &MText) -> String {
    // An edit to the public value supersedes the original spelling.
    let value = text.unchanged_source_value().map_or_else(|| text.value.clone(), str::to_string);
    crate::io::dwg::autocad_records::mtext_file_value(doc, text.common.handle, &value).into_owned()
}

/// A DXF attribute (or attribute definition) without a height reads as 1 in ACadSharp.
pub(crate) fn attribute_height(height: f64, source: Option<&TextSource>) -> f64 {
    if source.is_some_and(|s| s.dxf && !s.has_height) {
        1.0
    } else {
        height
    }
}

/// The weight ACadSharp reads for a control point of a hatch spline edge: its DWG reader uses zero for the absent
/// weight of a non-rational edge; native splines correctly use the stored one.
pub(crate) fn hatch_spline_weight(doc: &CadDocument, edge: &SplineEdge, weight: f64) -> f64 {
    if doc.dwg_source_version.is_some() && !edge.rational {
        0.0
    } else {
        weight
    }
}

/// `BlockRecord.Units`: ACadSharp reads the units of a DWG block record only from R2007 on, and of a DXF record only
/// when the record states them.
pub(crate) fn record_units(doc: &CadDocument, record: &BlockRecord) -> i16 {
    if (doc.dwg_source_version.is_some() && doc.version < DxfVersion::AC1021)
        || record.dxf_present.is_some_and(|p| p & 32 == 0)
    {
        0
    } else {
        record.units
    }
}

/// A document read from an R12 DXF, which ACadSharp reads with its legacy rules: no layouts, every POLYLINE as a
/// `Polyline2D` (a polyface mesh's face records become vertices at the origin), the ATTRIB and SEQEND children of an
/// INSERT promoted into the block collection.
pub(crate) fn legacy_dxf(doc: &CadDocument) -> bool {
    doc.dwg_source_version.is_none() && matches!(doc.version, DxfVersion::AC1009 | DxfVersion::Unknown)
}

/// The R12 DXF reader of ACadSharp rejects a diameter dimension carrying the user-positioned-text bit (its exact enum
/// switch knows only the linear types with that bit); the native entity stays.
pub(crate) fn rejects_legacy_dimension(entity: &EntityType) -> bool {
    matches!(entity, EntityType::Dimension(d) if matches!(d.as_ref(), Dimension::Diameter(_)) && d.base().text_user_positioned)
}

/// An ENTITIES record without a stated owner, which ACadSharp's R12 reader places in the model space even where
/// group 67 puts it in paper space natively. BLOCKS records are absent from this provenance map.
pub(crate) fn ownerless_entities_record(doc: &CadDocument, handle: Handle) -> bool {
    doc.source_entity_owners
        .as_ref()
        .and_then(|owners| owners.get(&handle))
        .is_some_and(|owner| owner.is_null())
}

/// The plain text of an MTEXT value as ACadSharp's `MText.PlainText` computes it, or `None` where it throws
/// (`ArgumentOutOfRangeException`).
pub(crate) fn plain_text(value: &str) -> Option<String> {
    super::mtext_plain_text::try_plain_text(value)
}
