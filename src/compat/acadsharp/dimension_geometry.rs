// SPDX-License-Identifier: MPL-2.0
// Adapted from ACadSharp and CSUtilities (MIT), Copyright (c) 2020, 2021 Albert Domenech.
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

use super::{number, vec3};
use crate::entities::{DimensionAngular2Ln, DimensionArc};
use crate::types::Vector3;
use serde_json::{json, Value};

/// CSMath uses exact coordinate equality, even for numerically computed
/// intersection points. Keep that quirk out of native geometric predicates.
/// `Err` is the exception the reference's `Center` getter throws; a NaN
/// vector is its "no exact intersection" result.
fn angular2_center_point(value: &DimensionAngular2Ln) -> Result<Vector3, &'static str> {
    let first = value.angle_vertex - value.definition_point;
    let second = value.second_point - value.first_point;
    if first == Vector3::ZERO || second == Vector3::ZERO {
        // Activator.CreateInstance wraps the Line3D constructor's exception.
        return Err("!throws:TargetInvocationException");
    }
    let u = first / first.length();
    let v = second / second.length();
    let offset = value.definition_point - value.first_point;
    let (a, b, c, d, e) = (
        u.dot(&u),
        u.dot(&v),
        v.dot(&v),
        u.dot(&offset),
        v.dot(&offset),
    );
    let denominator = a * c - b * b;
    let p = value.definition_point + u * ((b * e - c * d) / denominator);
    let q = value.first_point + v * ((a * e - b * d) / denominator);
    Ok(if p == q {
        p
    } else {
        Vector3::new(f64::NAN, f64::NAN, f64::NAN)
    })
}

pub(super) fn angular2_center(value: &DimensionAngular2Ln) -> Value {
    match angular2_center_point(value) {
        Ok(center) if center.x.is_nan() => json!(["NaN", "NaN", "NaN"]),
        Ok(center) => vec3(center),
        Err(thrown) => json!(thrown),
    }
}

/// `DimensionAngular2Line.Measurement`: the angle between the two direction
/// vectors, replaced by its supplement when the dimension arc point is not in
/// the sector between them (judged with one-sided cross-product signs, not an
/// intersection of the actual lines).
pub(super) fn angular2_measurement(value: &DimensionAngular2Ln) -> Value {
    const EPSILON: f64 = 1e-12;
    let v1 = value.second_point - value.first_point;
    let v2 = value.definition_point - value.angle_vertex;
    let (l1, l2) = (v1.length(), v2.length());
    if l1 <= EPSILON || l2 <= EPSILON {
        return json!("!throws:InvalidOperationException");
    }
    let mut angle = (v1.dot(&v2) / (l1 * l2)).acos();
    let center = match angular2_center_point(value) {
        Ok(center) => center,
        Err(thrown) => return json!(thrown),
    };
    let arc = value.dimension_arc - center;
    // NaN lengths are not "zero", so a NaN center falls through to NaN signs.
    if arc.length() <= EPSILON {
        return number(angle);
    }
    let dot1 = v1.cross(&arc).dot(&value.base.normal);
    let dot2 = arc.cross(&v2).dot(&value.base.normal);
    if (dot1 < 0.0 && dot2 > 0.0) || (dot1 > 0.0 && dot2 < 0.0) {
        angle = std::f64::consts::PI - angle;
    }
    number(angle)
}

/// Arc measurement is endpoint-based, with the reference's ratio-based
/// parallel test (which differs from both a cross-product and a dot-product test).
pub(super) fn arc_measurement(value: &DimensionArc) -> Value {
    let first = value.first_extension_point - value.center_point;
    let second = value.second_extension_point - value.center_point;
    if first == second {
        return number(0.0);
    }
    let radius = first.length();
    let other_length = second.length();
    if radius <= 1e-12 || other_length <= 1e-12 {
        return json!("!throws:InvalidOperationException");
    }
    let ratios = [second.x / first.x, second.y / first.y, second.z / first.z];
    // Double.Equals considers two NaNs equal (unlike the == operator).
    let equal = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
    let angle = if equal(ratios[0], ratios[1]) && equal(ratios[0], ratios[2]) {
        std::f64::consts::PI
    } else {
        (first.dot(&second) / (radius * other_length)).acos()
    };
    number(radius * angle)
}
