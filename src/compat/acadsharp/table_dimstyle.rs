// SPDX-License-Identifier: MPL-2.0
//! ACadSharp projection for dimension style table records.
//!
//! The native DIMSTYLE model deliberately uses the values needed by the DXF
//! and DWG readers.  ACadSharp's constructor defaults are different for a
//! handful of those values, and its DXF reader also has a few historical
//! group-code quirks.  Keep those compatibility decisions here so they do not
//! change the native drawing model.

use super::common::appearance_color;
use super::{character, number, optional_handle};
use crate::tables::DimStyle;
use crate::types::{Color, DxfVersion, Handle};
use crate::CadDocument;
use serde_json::{json, Value};

/// Project the type-specific members of one DIMSTYLE record.
pub(super) fn map(doc: &CadDocument, value: &DimStyle) -> Vec<(&'static str, Value)> {
    // A native constructor default that survives registration stands for the
    // "Standard" style ACadSharp creates from `DimensionStyle.Default`: its
    // constructor values, attached to the document's Standard text style.
    let created;
    let (value, dwg_version) = if super::registration::source_document(doc) && !value.source_present
    {
        let mut style = DimStyle::standard();
        style.name = value.name.clone();
        style.handle = value.handle;
        style.dimpost.clear();
        style.dimsah = false;
        style.dimtxsty_handle = doc
            .text_styles
            .get("Standard")
            .map_or(Handle::NULL, |text| text.handle);
        created = style;
        (&created, None)
    } else {
        (value, doc.dwg_source_version)
    };
    let source_dxf = value.dxf_present_codes.is_some();
    let source_dwg = dwg_version.is_some();
    let mut defaults = DimStyle::new(value.name.clone());
    // These two native constructor values are writer-oriented conveniences;
    // ACadSharp's DimensionStyle constructor starts with an empty postfix
    // and DIMSAH disabled.
    defaults.dimpost.clear();
    defaults.dimsah = false;

    // ACadSharp stores these as object references.  Its table-template build
    // step resolves a handle first and then (for legacy DIMBLK names) retries
    // by name; an unresolved reference remains null in the public property.
    let block = |handle: Option<Handle>, name: &str| {
        handle
            .filter(|handle| {
                doc.block_records
                    .iter()
                    .any(|entry| entry.handle == *handle)
            })
            .or_else(|| {
                (!name.is_empty()).then(|| {
                    doc.block_records
                        .iter()
                        .find(|entry| entry.name.eq_ignore_ascii_case(name))
                        .map(|entry| entry.handle)
                })?
            })
    };
    let text_style = |handle: Option<Handle>| {
        handle.filter(|handle| doc.text_styles.iter().any(|entry| entry.handle == *handle))
    };
    let line_type = |handle: Option<Handle>| {
        handle.filter(|handle| doc.line_types.iter().any(|entry| entry.handle == *handle))
    };

    // A DXF reader starts at the format defaults, whereas ACadSharp starts at
    // DimensionStyle's constructor defaults.  Use the latter only for groups
    // that the source record omitted.  Objects created in memory have no
    // source mask and retain their native values exactly.
    macro_rules! field {
        ($name:ident, $code:expr) => {{
            if source_dxf && !value.dxf_code_present($code) {
                defaults.$name.clone()
            } else {
                value.$name.clone()
            }
        }};
    }

    let has = |code: i32| value.dxf_code_present(code);
    let mut inside_horizontal = field!(dimtih, 73);
    let mut outside_horizontal = field!(dimtoh, 74);
    let mut suppress_first_extension = field!(dimse1, 75);
    let mut suppress_second_extension = field!(dimse2, 76);

    // ACadSharp's R13/R14 DWG reader consumes these four bits in a different
    // order from the native reader.  Reinterpret the native fields only for
    // an actual DWG source; constructed styles must keep their values.
    if let Some(version) = dwg_version {
        if matches!(version, DxfVersion::AC1012 | DxfVersion::AC1014) {
            outside_horizontal = value.dimtih;
            suppress_first_extension = value.dimtoh;
            suppress_second_extension = value.dimse1;
            inside_horizontal = value.dimse2;
        }
    }

    let mut scale_factor = field!(dimscale, 40);
    if source_dxf && has(40) && scale_factor < 0.0 {
        // DimensionStyle.ScaleFactor catches an invalid negative value and
        // assigns MathHelper.Epsilon.  The native reader has already stored
        // the source number, so repair it only in this compatibility view.
        scale_factor = 1.0e-12;
    }

    let mut arrow_size = field!(dimasz, 41);
    if source_dxf && has(41) && arrow_size < 0.0 {
        // ACadSharp leaves its constructor value in place after the failed
        // assignment.
        arrow_size = defaults.dimasz;
    }

    let mut text_height = field!(dimtxt, 140);
    if source_dxf && has(140) && text_height <= 0.0 {
        text_height = defaults.dimtxt;
    }

    let mut jog_angle = field!(dimjogang, 50);
    if source_dxf && has(50) {
        let rounded = (jog_angle * 1_000_000.0).round() / 1_000_000.0;
        if rounded <= 5.0_f64.to_radians() || rounded >= std::f64::consts::FRAC_PI_2 {
            jog_angle = defaults.dimjogang;
        }
    }

    // DWG records before R2007 do not carry DIMFXL or DIMJOGANG.  The native
    // reader's zero placeholders differ from ACadSharp's constructor values.
    if let Some(version) = dwg_version {
        if version < DxfVersion::AC1021 {
            jog_angle = defaults.dimjogang;
        }
    }

    let fixed_extension_length = match dwg_version {
        Some(version) if version < DxfVersion::AC1021 => defaults.dimfxl,
        _ => field!(dimfxl, 49),
    };
    let dimension_line_weight = match dwg_version {
        Some(version) if version < DxfVersion::AC1015 => defaults.dimlwd,
        _ => field!(dimlwd, 371),
    };
    let extension_line_weight = match dwg_version {
        Some(version) if version < DxfVersion::AC1015 => defaults.dimlwe,
        _ => field!(dimlwe, 372),
    };

    let postfix = field!(dimpost, 3);

    // In ACadSharp code 270 is (historically) assigned to LinearUnitFormat,
    // while DimensionUnit remains at its constructor default.  Code 277 is
    // assigned to the same property, so the last group wins when both occur.
    let linear_unit_format = if source_dxf {
        match value.dxf_last_linear_unit_code {
            Some(270) => value.dimunit,
            Some(277) => value.dimlunit,
            _ if has(277) => value.dimlunit,
            _ if has(270) => value.dimunit,
            _ => defaults.dimlunit,
        }
    } else {
        value.dimlunit
    };
    let dimension_unit = if source_dxf {
        defaults.dimunit
    } else {
        value.dimunit
    };
    let dimension_fit = if source_dwg {
        // The DWG reader's post-DIMUPT bit-short is labelled DIMFIT by
        // ACadSharp, although the native model stores it in DIMATFIT for
        // R2000 and newer records.  R13/R14 already retain the old byte in
        // the native DIMFIT field.
        match dwg_version {
            Some(version) if version >= DxfVersion::AC1015 => value.dimatfit,
            _ => value.dimfit,
        }
    } else {
        field!(dimfit, 287)
    };
    let text_arrow_fit = if source_dwg {
        // ACadSharp does not assign DimensionTextArrowFit while reading DWG;
        // it keeps the constructor's BestFit value.
        defaults.dimatfit
    } else {
        field!(dimatfit, 289)
    };

    // A second group 70 carries DIMTFILLCLR.  The C# reader ignores negative
    // values; native Color::from_index would otherwise turn one into an ACI
    // color.  True-color overrides are retained for in-memory/DWG records.
    let mut text_background_index = field!(dimtfillclr, 70);
    if text_background_index < 0 {
        text_background_index = defaults.dimtfillclr;
    }

    let (prefix, suffix) = postfix
        .split_once("<>")
        .map(|(prefix, suffix)| (prefix.to_string(), suffix.to_string()))
        .unwrap_or_else(|| (String::new(), postfix.clone()));

    let flags = if source_dwg {
        // ACadSharp's DWG reader exposes only the dependant and reference
        // bits.  The resolved bit is retained by the native model but is not
        // part of its compatibility StandardFlags projection.
        (value.xref_dependent as i16) * 16 + (value.xref_reference as i16) * 64
    } else {
        (value.xref_dependent as i16) * 16
            + (value.xref_resolved as i16) * 32
            + (value.xref_reference as i16) * 64
    };

    let dim_line_color = value
        .dimclrd_true_color
        .unwrap_or_else(|| Color::from_index(field!(dimclrd, 176)));
    let extension_line_color = value
        .dimclre_true_color
        .unwrap_or_else(|| Color::from_index(field!(dimclre, 177)));
    let text_color = value
        .dimclrt_true_color
        .unwrap_or_else(|| Color::from_index(field!(dimclrt, 178)));
    let text_background_color = value
        .dimtfillclr_true_color
        .unwrap_or_else(|| Color::from_index(text_background_index));

    vec![
        ("AlternateDimensioningSuffix", json!(field!(dimapost, 4))),
        ("AlternateUnitDecimalPlaces", json!(field!(dimaltd, 171))),
        ("AlternateUnitDimensioning", json!(field!(dimalt, 170))),
        ("AlternateUnitFormat", json!(field!(dimaltu, 273))),
        ("AlternateUnitRounding", number(field!(dimaltrnd, 148))),
        ("AlternateUnitScaleFactor", number(field!(dimaltf, 143))),
        (
            "AlternateUnitToleranceDecimalPlaces",
            json!(field!(dimalttd, 274)),
        ),
        (
            "AlternateUnitToleranceZeroHandling",
            json!(field!(dimalttz, 286)),
        ),
        ("AlternateUnitZeroHandling", json!(field!(dimaltz, 285))),
        ("AngularDecimalPlaces", json!(field!(dimadec, 179))),
        ("AngularUnit", json!(field!(dimaunit, 275))),
        ("AngularZeroHandling", json!(field!(dimazin, 79))),
        ("ArcLengthSymbolPosition", json!(field!(dimarcsym, 90))),
        (
            "ArrowBlock",
            optional_handle(block(Some(value.dimblk), &value.dimblk_name)),
        ),
        ("ArrowSize", number(arrow_size)),
        ("CenterMarkSize", number(field!(dimcen, 141))),
        ("CursorUpdate", json!(field!(dimupt, 288))),
        ("DecimalPlaces", json!(field!(dimdec, 271))),
        (
            "DecimalSeparator",
            character(char::from_u32((field!(dimdsep, 278) as u16) as u32).unwrap_or('\0')),
        ),
        (
            "DimArrow1",
            optional_handle(block(Some(value.dimblk1), &value.dimblk1_name)),
        ),
        (
            "DimArrow2",
            optional_handle(block(Some(value.dimblk2), &value.dimblk2_name)),
        ),
        ("DimensionFit", json!(dimension_fit)),
        ("DimensionLineColor", appearance_color(dim_line_color)),
        ("DimensionLineExtension", number(field!(dimdle, 46))),
        ("DimensionLineGap", number(field!(dimgap, 147))),
        ("DimensionLineIncrement", number(field!(dimdli, 43))),
        ("DimensionLineWeight", json!(dimension_line_weight)),
        ("DimensionTextArrowFit", json!(text_arrow_fit)),
        ("DimensionUnit", json!(dimension_unit)),
        ("ExtensionLineColor", appearance_color(extension_line_color)),
        ("ExtensionLineExtension", number(field!(dimexe, 44))),
        ("ExtensionLineOffset", number(field!(dimexo, 42))),
        ("ExtensionLineWeight", json!(extension_line_weight)),
        ("FixedExtensionLineLength", number(fixed_extension_length)),
        ("Flags", json!(flags)),
        ("FractionFormat", json!(field!(dimfrac, 276))),
        ("GenerateTolerances", json!(field!(dimtol, 71))),
        ("IsExtensionLineLengthFixed", json!(field!(dimfxlon, 290))),
        (
            "JoggedRadiusDimensionTransverseSegmentAngle",
            number(jog_angle),
        ),
        (
            "LeaderArrow",
            optional_handle(block(Some(value.dimldrblk), "")),
        ),
        ("LimitsGeneration", json!(field!(dimlim, 72))),
        (
            "LineType",
            optional_handle(line_type(Some(value.dimltex_handle))),
        ),
        (
            "LineTypeExt1",
            optional_handle(line_type(Some(value.dimltex1_handle))),
        ),
        (
            "LineTypeExt2",
            optional_handle(line_type(Some(value.dimltex2_handle))),
        ),
        ("LinearScaleFactor", number(field!(dimlfac, 144))),
        ("LinearUnitFormat", json!(linear_unit_format)),
        ("MinusTolerance", number(field!(dimtm, 48))),
        ("Name", json!(value.name)),
        ("PlusTolerance", number(field!(dimtp, 47))),
        ("PostFix", json!(postfix)),
        ("Prefix", json!(prefix)),
        ("Rounding", number(field!(dimrnd, 45))),
        ("ScaleFactor", number(scale_factor)),
        ("SeparateArrowBlocks", json!(field!(dimsah, 173))),
        (
            "Style",
            optional_handle(text_style(Some(value.dimtxsty_handle))),
        ),
        ("SuppressFirstDimensionLine", json!(field!(dimsd1, 281))),
        (
            "SuppressFirstExtensionLine",
            json!(suppress_first_extension),
        ),
        ("SuppressOutsideExtensions", json!(field!(dimsoxd, 175))),
        ("SuppressSecondDimensionLine", json!(field!(dimsd2, 282))),
        (
            "SuppressSecondExtensionLine",
            json!(suppress_second_extension),
        ),
        ("TextBackgroundColor", appearance_color(text_background_color)),
        ("TextBackgroundFillMode", json!(field!(dimtfill, 69))),
        ("TextColor", appearance_color(text_color)),
        ("TextDirection", json!(field!(dimtxtdirection, 295) as i16)),
        ("TextHeight", number(text_height)),
        ("TextHorizontalAlignment", json!(field!(dimjust, 280))),
        ("TextInsideExtensions", json!(field!(dimtix, 174))),
        ("TextInsideHorizontal", json!(inside_horizontal)),
        ("TextMovement", json!(field!(dimtmove, 279))),
        ("TextOutsideExtensions", json!(field!(dimtofl, 172))),
        ("TextOutsideHorizontal", json!(outside_horizontal)),
        ("TextVerticalAlignment", json!(field!(dimtad, 77))),
        ("TextVerticalPosition", number(field!(dimtvp, 145))),
        ("TickSize", number(field!(dimtsz, 142))),
        ("ToleranceAlignment", json!(field!(dimtolj, 283))),
        ("ToleranceDecimalPlaces", json!(field!(dimtdec, 272))),
        ("ToleranceScaleFactor", number(field!(dimtfac, 146))),
        ("ToleranceZeroHandling", json!(field!(dimtzin, 284))),
        ("Suffix", json!(suffix)),
        ("ZeroHandling", json!(field!(dimzin, 78))),
    ]
}
