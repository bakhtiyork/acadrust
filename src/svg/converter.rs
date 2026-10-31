// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/IO/SVG/SvgConverter.cs) by Albert Domenech.
// Copyright (c) 2021 Albert Domenech
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! Number and unit conversions of the SVG writer (`SvgConverter`), plus .NET's `double.ToString()`.

use super::csmath::Xyz;
use super::error::SvgError;

/// ACadSharp `UnitsType` (the INSUNITS values), kept as the raw number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UnitsType(pub i16);

impl UnitsType {
    pub const UNITLESS: UnitsType = UnitsType(0);
    pub const INCHES: UnitsType = UnitsType(1);
    pub const MILLIMETERS: UnitsType = UnitsType(4);
    pub const CENTIMETERS: UnitsType = UnitsType(5);
}

/// `double.ToString(CultureInfo.InvariantCulture)` of .NET Core 3.0 and later: the shortest string that
/// round-trips, in scientific form (`1E-05`, `1.5E+20`) when the decimal exponent is below -4 or at least 17.
pub fn double_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_owned();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let sci = format!("{:e}", value.abs());
    let (mantissa, exponent) = sci.split_once('e').expect("exponent form");
    let exponent: i32 = exponent.parse().expect("exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let scale = exponent + 1;
    let mut out = String::with_capacity(digits.len() + 8);
    out.push_str(sign);
    if !(-3..=17).contains(&scale) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
    } else if scale <= 0 {
        out.push_str("0.");
        for _ in 0..(-scale) {
            out.push('0');
        }
        out.push_str(&digits);
    } else if scale as usize >= digits.len() {
        out.push_str(&digits);
        for _ in 0..(scale as usize - digits.len()) {
            out.push('0');
        }
    } else {
        out.push_str(&digits[..scale as usize]);
        out.push('.');
        out.push_str(&digits[scale as usize..]);
    }
    out
}

/// `double.ToSvg()`.
pub fn to_svg(value: f64) -> String {
    double_to_string(value)
}

/// `double.ToSvg(units)`: the number followed by `cm`, `mm` or `in`.
pub fn to_svg_units(value: f64, units: UnitsType) -> String {
    let suffix = match units {
        UnitsType::CENTIMETERS => "cm",
        UnitsType::MILLIMETERS => "mm",
        UnitsType::INCHES => "in",
        _ => "",
    };
    format!("{}{}", double_to_string(value), suffix)
}

/// `vector.ToSvg()` for a two-component vector: `x,y`.
pub fn xy_to_svg(x: f64, y: f64) -> String {
    format!("{},{}", to_svg(x), to_svg(y))
}

/// `double.ToPixelSize(units)`; unsupported units throw `InvalidEnumArgumentException`.
pub fn to_pixel_size(value: f64, units: UnitsType) -> Result<f64, SvgError> {
    match units {
        UnitsType::INCHES => Ok(value * 96.0),
        UnitsType::MILLIMETERS => Ok(value * 96.0 / 25.4),
        UnitsType::UNITLESS => Ok(value),
        UnitsType(n) => Err(SvgError::invalid_enum(format!(
            "The value of argument 'units' ({n}) is invalid for Enum type 'UnitsType'. (Parameter 'units')"
        ))),
    }
}

/// `XYZ.ToPixelSize(units)`.
pub fn xyz_to_pixel(v: Xyz, units: UnitsType) -> Result<Xyz, SvgError> {
    Ok(Xyz::new(
        to_pixel_size(v.x, units)?,
        to_pixel_size(v.y, units)?,
        to_pixel_size(v.z, units)?,
    ))
}
