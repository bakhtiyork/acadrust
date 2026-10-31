// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/IO/SVG/SvgConfiguration.cs) by Albert Domenech.
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

//! Configuration for writing SVG files (`SvgConfiguration`).

use super::converter::UnitsType;

/// Configuration for writing SVG files. The defaults are ACadSharp's.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgConfiguration {
    /// The line weight is divided by this value to get the stroke width when the units are unitless
    /// (default 100, which makes the weight the width in mm).
    pub line_weight_ratio: f64,
    /// Weight (in mm) for the default line weight.
    pub default_line_weight: f64,
    /// Radius of the circle drawn for a point.
    pub point_radius: f64,
    /// Points used for arcs and bulges.
    pub arc_points: i32,
    /// Not in ACadSharp. Read the document the way ACadSharp reads the same file where that differs from the
    /// native model, through the rules of the ACadSharp dump (`compat::acadsharp`): text spellings, colours and line
    /// weights, block record units and entity lists, a hatch spline edge's missing weights, the R12 DXF reading, and
    /// ACadSharp's ACI palette and MTEXT plain text, so the output is byte for byte what ACadSharp writes. Off by
    /// default: the native values are the correct ones.
    #[cfg(feature = "acadsharp-compat")]
    pub acadsharp_view: bool,
}

impl Default for SvgConfiguration {
    fn default() -> Self {
        SvgConfiguration {
            line_weight_ratio: 100.0,
            default_line_weight: 0.01,
            point_radius: 0.1,
            arc_points: 256,
            #[cfg(feature = "acadsharp-compat")]
            acadsharp_view: false,
        }
    }
}

impl SvgConfiguration {
    /// The configuration of ACadSharp's `IOTestsBase` (`DefaultLineWeight = 0.15`, `LineWeightRatio = 50`).
    pub fn test() -> Self {
        SvgConfiguration {
            default_line_weight: 0.15,
            line_weight_ratio: 50.0,
            ..Default::default()
        }
    }

    /// The stroke width in mm. `line_weight` is the `LineWeightType` number (-3 is `Default`, 0 is `W0`).
    pub fn line_weight_value(&self, line_weight: i16, units: UnitsType) -> f64 {
        let value = f64::from(line_weight).abs();
        if units == UnitsType::UNITLESS {
            return value / self.line_weight_ratio;
        }
        match line_weight {
            -3 => self.default_line_weight,
            0 => 0.001,
            _ => line_weight_mm(line_weight),
        }
    }
}

/// `LineWeightType.GetLineWeightValue()`: the weight in mm.
pub fn line_weight_mm(line_weight: i16) -> f64 {
    if line_weight == 0 {
        return 0.001;
    }
    f64::from(line_weight).abs() / 100.0
}
