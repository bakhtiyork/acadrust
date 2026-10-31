// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/ACadSharp/Entities/{Arc,Ellipse,Circle,Spline}.cs, src/ACadSharp/Extensions/PolylineExtensions.cs, src/CSUtilities/CSMath/Geometry/CurveExtensions.cs) by Albert Domenech.
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

//! Curve geometry that the SVG writer takes from ACadSharp's entities: arc and ellipse polygons,
//! bulge arcs, polyline points and spline evaluation. The code follows the C# operation order so the
//! doubles come out identical.

use super::csmath::{cos, is_equal, sin, Matrix4, Xy, Xyz, TWO_PI};
use super::error::SvgError;

fn precision_error(what: &str, precision: i32) -> SvgError {
    SvgError::out_of_range(format!(
        "{what} must be equal or greater than two. (Parameter 'precision')\nActual value was {precision}."
    ))
}

/// `CurveExtensions.PolarCoordinate`.
pub fn polar_coordinate(angle: f64, center: Xyz, normal: Xyz, start_point: Xyz, ratio: f64) -> Xyz {
    let prep = Xyz::cross(normal, start_point);
    start_point
        .mul(angle.cos())
        .add(prep.mul(ratio * angle.sin()))
        .add(center)
}

/// `CurveExtensions.PolygonalVertexes(precision, center, start, end, radius, normal)` (circles and arcs).
pub fn circle_polygonal_vertexes(
    precision: i32,
    center: Xyz,
    start_angle: f64,
    end_angle: f64,
    radius: f64,
    normal: Xyz,
) -> Result<Vec<Xyz>, SvgError> {
    if precision < 2 {
        return Err(precision_error("The precision", precision));
    }
    let mut start = start_angle;
    let mut end = end_angle;
    if end <= start_angle {
        end += TWO_PI;
    }
    let t = Matrix4::arbitrary_axis(normal);
    let sweep = end - start;
    let denominator = if is_equal(sweep, TWO_PI) {
        precision
    } else {
        precision - 1
    };
    let delta = (end - start) / f64::from(denominator);
    let mut points = Vec::with_capacity(precision as usize);
    for _ in 0..precision {
        let pt = Xyz::new(cos(start), sin(start), 0.0);
        let pt = center.add(pt.mul(radius));
        points.push(t.mul_xyz(pt));
        start += delta;
    }
    Ok(points)
}

/// `CurveExtensions.PolygonalVertexes(precision, center, start, end, normal, majorAxisPoint, ratio)` (ellipses).
pub fn ellipse_polygonal_vertexes(
    precision: i32,
    center: Xyz,
    start_angle: f64,
    end_angle: f64,
    normal: Xyz,
    major_axis_point: Xyz,
    ratio: f64,
) -> Result<Vec<Xyz>, SvgError> {
    if precision < 2 {
        return Err(precision_error("The precision", precision));
    }
    let mut start = start_angle;
    let mut end = end_angle;
    if end <= start {
        end += TWO_PI;
    }
    let relative = major_axis_point.sub(center);
    let perp = Xyz::cross(normal, relative);
    let sweep = start - end;
    let denominator = if is_equal(sweep, TWO_PI) {
        precision
    } else {
        precision - 1
    };
    let delta = (end - start) / f64::from(denominator);
    let mut points = Vec::with_capacity(precision as usize);
    for _ in 0..precision {
        points.push(
            relative
                .mul(cos(start))
                .add(perp.mul(ratio * sin(start)))
                .add(center),
        );
        start += delta;
    }
    Ok(points)
}

/// The parts of an `Arc` that the geometry needs.
#[derive(Debug, Clone, Copy)]
pub struct ArcGeometry {
    pub center: Xyz,
    pub radius: f64,
    pub start_angle: f64,
    pub end_angle: f64,
    pub normal: Xyz,
}

impl ArcGeometry {
    /// `Arc.Sweep` (start minus end, as ACadSharp computes it).
    pub fn sweep(&self) -> f64 {
        let start = self.start_angle;
        let mut end = self.end_angle;
        if end < start {
            end += TWO_PI;
        }
        start - end
    }

    /// `Arc.PolygonalVertexes(precision)`.
    pub fn polygonal_vertexes(&self, precision: i32) -> Result<Vec<Xyz>, SvgError> {
        if precision < 2 {
            return Err(precision_error("The arc precision", precision));
        }
        circle_polygonal_vertexes(
            precision,
            self.center,
            self.start_angle,
            self.end_angle,
            self.radius,
            self.normal.normalize(),
        )
    }

    /// `Arc.GetEndVertices`.
    pub fn end_vertices(&self) -> (Xyz, Xyz) {
        let start = Xyz::new(cos(self.start_angle), sin(self.start_angle), 0.0);
        let start = self.center.add(start.mul(self.radius));
        let end = Xyz::new(cos(self.end_angle), sin(self.end_angle), 0.0);
        let end = self.center.add(end.mul(self.radius));
        let t = Matrix4::arbitrary_axis(self.normal);
        (t.mul_xyz(start), t.mul_xyz(end))
    }

    /// `Arc.CreateFromBulge`: the `Radius` setter rejects a radius of zero or less.
    pub fn from_bulge(p1: Xy, p2: Xy, bulge: f64) -> Result<ArcGeometry, SvgError> {
        let theta = 4.0 * bulge.abs().atan();
        let c = p1.distance_from(p2) / 2.0;
        let radius = c / (theta / 2.0).sin();
        let gamma = (std::f64::consts::PI - theta) / 2.0;
        let sign = if bulge > 0.0 {
            1.0
        } else if bulge < 0.0 {
            -1.0
        } else {
            0.0
        };
        let phi = p2.sub(p1).get_angle() + sign * gamma;
        let center = Xy::new(p1.x + radius * cos(phi), p1.y + radius * sin(phi));
        let (start_angle, end_angle) = if bulge < 0.0 {
            (p2.sub(center).get_angle(), p1.sub(center).get_angle())
        } else {
            (p1.sub(center).get_angle(), p2.sub(center).get_angle())
        };
        if radius <= 0.0 {
            return Err(SvgError::out_of_range(format!(
                "The radius must be greater than 0. (Parameter 'value')\nActual value was {radius}."
            )));
        }
        Ok(ArcGeometry {
            center: Xyz::new(center.x, center.y, 0.0),
            radius,
            start_angle,
            end_angle,
            normal: Xyz::AXIS_Z,
        })
    }
}

/// The parts of an `Ellipse` that the geometry needs.
#[derive(Debug, Clone, Copy)]
pub struct EllipseGeometry {
    pub center: Xyz,
    pub major_axis_end_point: Xyz,
    pub radius_ratio: f64,
    pub start_parameter: f64,
    pub end_parameter: f64,
    pub normal: Xyz,
}

impl EllipseGeometry {
    pub fn is_full_ellipse(&self) -> bool {
        self.start_parameter == 0.0 && self.end_parameter == TWO_PI
    }
    pub fn major_axis(&self) -> f64 {
        2.0 * self.major_axis_end_point.length()
    }
    pub fn minor_axis(&self) -> f64 {
        self.major_axis() * self.radius_ratio
    }
    pub fn rotation(&self) -> f64 {
        self.major_axis_end_point.xy().get_angle()
    }
    pub fn polar_coordinate_relative_to_center(&self, angle: f64) -> Xyz {
        polar_coordinate(
            angle,
            self.center,
            self.normal,
            self.major_axis_end_point,
            self.radius_ratio,
        )
    }
    pub fn polygonal_vertexes(&self, precision: i32) -> Result<Vec<Xyz>, SvgError> {
        ellipse_polygonal_vertexes(
            precision,
            self.center,
            self.start_parameter,
            self.end_parameter,
            self.normal,
            self.major_axis_end_point.add(self.center),
            self.radius_ratio,
        )
    }
    /// `Ellipse.RadiusRatio` setter: zero or less, or more than one, is rejected.
    pub fn check_ratio(ratio: f64) -> Result<(), SvgError> {
        if ratio <= 0.0 || ratio > 1.0 {
            return Err(SvgError::out_of_range(
                "Radius ratio must be a value between 0 (not included) and 1. (Parameter 'value')",
            ));
        }
        Ok(())
    }
}

/// A polyline vertex: its location (`IVertex.Location`) and bulge.
#[derive(Debug, Clone, Copy)]
pub struct VertexPoint {
    pub location: Xyz,
    pub bulge: f64,
}

fn convert(p: Xyz, dim: usize) -> Xyz {
    if dim == 2 {
        Xyz::new(p.x, p.y, 0.0)
    } else {
        p
    }
}

fn same_rounded(a: Xyz, b: Xyz, dim: usize) -> bool {
    let a = convert(a, dim).round(8);
    let b = convert(b, dim).round(8);
    a.x == b.x && a.y == b.y && (dim == 2 || a.z == b.z)
}

/// `PolylineExtensions.GetPoints<T>(precision)`: arcs for the vertices with a bulge.
pub fn polyline_points_with_arcs(
    vertices: &[VertexPoint],
    is_closed: bool,
    precision: i32,
    dim: usize,
) -> Result<Vec<Xyz>, SvgError> {
    if precision < 2 {
        return Err(SvgError::out_of_range(format!(
            "The arc precision must be equal or greater than two. (Parameter 'precision')\nActual value was {precision}."
        )));
    }
    let mut points: Vec<Xyz> = Vec::new();
    for i in 0..vertices.len() {
        let curr = vertices[i];
        let next = if i + 1 < vertices.len() {
            vertices[i + 1]
        } else if is_closed {
            vertices[0]
        } else {
            break;
        };
        if curr.bulge == 0.0 {
            if i == 0 {
                points.push(convert(curr.location, dim));
            }
            points.push(convert(next.location, dim));
        } else {
            let p1 = curr.location.xy();
            let p2 = next.location.xy();
            let arc = ArcGeometry::from_bulge(p1, p2, curr.bulge)?;
            let mut list: Vec<Xyz> = arc
                .polygonal_vertexes(precision)?
                .into_iter()
                .map(|p| convert(p, dim))
                .collect();
            let first = list[0];
            let last = list[list.len() - 1];
            if same_rounded(first, curr.location, dim) {
                points.extend(list.into_iter().skip(1));
            } else if same_rounded(last, curr.location, dim) {
                list.reverse();
                points.extend(list.into_iter().skip(1));
            }
        }
    }
    Ok(points)
}

// ---------------------------------------------------------------------------------------------
// Splines
// ---------------------------------------------------------------------------------------------

/// The data of a `Spline` that `PolygonalVertexes` reads.
#[derive(Debug, Clone)]
pub struct SplineGeometry {
    pub control_points: Vec<Xyz>,
    pub weights: Vec<f64>,
    pub knots: Vec<f64>,
    pub degree: i32,
    pub is_closed: bool,
    pub is_periodic: bool,
}

fn index_error() -> SvgError {
    SvgError::IndexOutOfRange("Index was outside the bounds of the array.".to_owned())
}

fn at<T: Copy>(slice: &[T], index: i64) -> Result<T, SvgError> {
    if index < 0 {
        return Err(index_error());
    }
    slice.get(index as usize).copied().ok_or_else(index_error)
}

fn compute_nurb(knots: &[f64], i: i64, p: i64, u: f64) -> Result<f64, SvgError> {
    if p <= 0 {
        if at(knots, i)? <= u && u < at(knots, i + 1)? {
            return Ok(1.0);
        }
        return Ok(0.0);
    }
    let eps = f64::from_bits(1);
    let mut left = 0.0;
    if !((at(knots, i + p)? - at(knots, i)?).abs() < eps) {
        left = (u - at(knots, i)?) / (at(knots, i + p)? - at(knots, i)?);
    }
    let mut right = 0.0;
    if !((at(knots, i + p + 1)? - at(knots, i + 1)?).abs() < eps) {
        right = (at(knots, i + p + 1)? - u) / (at(knots, i + p + 1)? - at(knots, i + 1)?);
    }
    Ok(left * compute_nurb(knots, i, p - 1, u)? + right * compute_nurb(knots, i + 1, p - 1, u)?)
}

fn create_knot_vector(num_control_points: i64, degree: i64, is_periodic: bool) -> Vec<f64> {
    let mut knots;
    if !is_periodic {
        let num_knots = num_control_points + degree + 1;
        knots = vec![0.0; num_knots.max(0) as usize];
        let mut i = 0i64;
        while i <= degree {
            if let Some(k) = knots.get_mut(i as usize) {
                *k = 0.0;
            }
            i += 1;
        }
        while i < num_control_points {
            if let Some(k) = knots.get_mut(i as usize) {
                *k = (i - degree) as f64;
            }
            i += 1;
        }
        while i < num_knots {
            if let Some(k) = knots.get_mut(i as usize) {
                *k = (num_control_points - degree) as f64;
            }
            i += 1;
        }
    } else {
        let num_knots = num_control_points + 2 * degree + 1;
        knots = vec![0.0; num_knots.max(0) as usize];
        let factor = 1.0 / (num_control_points - degree) as f64;
        for i in 0..num_knots {
            knots[i as usize] = (i - degree) as f64 * factor;
        }
    }
    knots
}

fn c(ctrl: &[Xyz], weights: &[f64], knots: &[f64], degree: i64, u: f64) -> Result<Xyz, SvgError> {
    let mut vector_sum = Xyz::ZERO;
    let mut denominator_sum = 0.0;
    for (i, point) in ctrl.iter().enumerate() {
        let nurb = compute_nurb(knots, i as i64, degree, u)?;
        let w = at(weights, i as i64)?;
        denominator_sum += nurb * w;
        vector_sum = vector_sum.add(point.mul(w * nurb));
    }
    if denominator_sum.abs() < f64::from_bits(1) {
        return Ok(Xyz::ZERO);
    }
    Ok(vector_sum.mul(1.0 / denominator_sum))
}

impl SplineGeometry {
    fn has_valid_knot_count(&self) -> bool {
        let expected = self.control_points.len() as i64
            + (if self.is_closed { 2 } else { 1 }) * i64::from(self.degree)
            + 1;
        self.knots.len() as i64 == expected
    }

    #[allow(clippy::type_complexity)]
    fn prepare(&self) -> Result<(Vec<Xyz>, Vec<f64>, Vec<f64>), SvgError> {
        let ctrl = &self.control_points;
        let mut weights = self.weights.clone();
        let mut knots = self.knots.clone();
        let n = ctrl.len();
        if n == 0 {
            return Err(SvgError::argument(
                "A spline entity with control points is required. (Parameter 'c')",
            ));
        }
        if weights.is_empty() || weights.len() != n {
            weights = vec![1.0; n];
        }
        if knots.is_empty() || !self.has_valid_knot_count() {
            knots = create_knot_vector(n as i64, i64::from(self.degree), self.is_periodic);
        }
        if self.is_periodic {
            let degree = self.degree as i64;
            let total = n as i64 + degree;
            if degree < 0 || degree > n as i64 {
                return Err(index_error());
            }
            let mut cp = vec![Xyz::ZERO; total as usize];
            let mut ws = vec![0.0; total as usize];
            for i in 0..degree {
                let index = (n as i64 - degree + i) as usize;
                cp[i as usize] = ctrl[index];
                ws[i as usize] = weights[index];
            }
            cp[degree as usize..].copy_from_slice(ctrl);
            ws[degree as usize..].copy_from_slice(&weights);
            Ok((cp, ws, knots))
        } else {
            Ok((ctrl.clone(), weights, knots))
        }
    }

    fn start_end_knots(&self, knots: &[f64]) -> Result<(f64, f64), SvgError> {
        if self.is_closed {
            Ok((at(knots, 0)?, at(knots, knots.len() as i64 - 1)?))
        } else if self.is_periodic {
            let degree = i64::from(self.degree);
            Ok((
                at(knots, degree)?,
                at(knots, knots.len() as i64 - degree - 1)?,
            ))
        } else {
            Ok((at(knots, 0)?, at(knots, knots.len() as i64 - 1)?))
        }
    }

    /// `Spline.PolygonalVertexes(precision)`.
    pub fn polygonal_vertexes(&self, precision: i32) -> Result<Vec<Xyz>, SvgError> {
        if precision < 2 {
            return Err(precision_error("The precision", precision));
        }
        let (ctrl, weights, knots) = self.prepare()?;
        let (u_start, u_end) = self.start_end_knots(&knots)?;
        let mut precision = precision;
        if !self.is_closed && !self.is_periodic {
            precision -= 1;
        }
        let u_delta = (u_end - u_start) / f64::from(precision);
        let mut vertexes = Vec::with_capacity(precision as usize + 1);
        for i in 0..precision {
            let u = u_start + u_delta * f64::from(i);
            vertexes.push(c(&ctrl, &weights, &knots, i64::from(self.degree), u)?);
        }
        if !(self.is_closed || self.is_periodic) {
            vertexes.push(ctrl[ctrl.len() - 1]);
        }
        Ok(vertexes)
    }
}
