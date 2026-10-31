// SPDX-License-Identifier: MPL-2.0
// Portions adapted from ACadSharp (src/CSUtilities/CSMath/*.cs) by Albert Domenech.
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

//! The subset of ACadSharp's CSMath that the SVG writer uses, with the same operation order.
//!
//! The SVG output prints doubles in their shortest round-trip form, so a one-ulp difference in a
//! transformed coordinate changes the file. Every routine here therefore evaluates its operations
//! in the order the C# source does (dot products start from `0.0`, matrices multiply row by column,
//! `Math.Min` propagates NaN, and so on).

use super::error::SvgError;

/// `MathHelper.Epsilon`.
pub const EPSILON: f64 = 1e-12;
/// `MathHelper.TwoPI`.
pub const TWO_PI: f64 = std::f64::consts::PI * 2.0;
/// `MathHelper.RadToDegFactor`.
const RAD_TO_DEG_FACTOR: f64 = 180.0 / std::f64::consts::PI;

/// `DoubleExtensions.IsZero(threshold)`.
pub fn is_zero(value: f64, threshold: f64) -> bool {
    value >= -threshold && value <= threshold
}

/// `MathHelper.IsEqual(a, b)`.
pub fn is_equal(a: f64, b: f64) -> bool {
    is_zero(a - b, EPSILON)
}

/// `MathHelper.Cos`.
pub fn cos(value: f64) -> f64 {
    let result = value.cos();
    if is_zero(result, EPSILON) {
        0.0
    } else {
        result
    }
}

/// `MathHelper.Sin`.
pub fn sin(value: f64) -> f64 {
    let result = value.sin();
    if is_zero(result, EPSILON) {
        0.0
    } else {
        result
    }
}

fn normalize_angle(angle: f64, full_circle: f64) -> f64 {
    if is_equal(angle.abs(), full_circle) {
        return full_circle;
    }
    let normalized = angle % full_circle;
    if is_zero(normalized, EPSILON) {
        return 0.0;
    }
    if normalized < 0.0 {
        return full_circle + normalized;
    }
    normalized
}

/// `MathHelper.RadToDeg(value)` with its default normalisation to `[0, 360]`.
pub fn rad_to_deg(value: f64) -> f64 {
    normalize_angle(value * RAD_TO_DEG_FACTOR, 360.0)
}

/// `Math.Min(double, double)` of .NET: NaN propagates and `-0.0` is smaller than `0.0`.
pub fn dmin(a: f64, b: f64) -> f64 {
    if a != b {
        if !a.is_nan() {
            return if a < b { a } else { b };
        }
        return a;
    }
    if a.is_sign_negative() {
        a
    } else {
        b
    }
}

/// `Math.Max(double, double)` of .NET: NaN propagates and `0.0` is larger than `-0.0`.
pub fn dmax(a: f64, b: f64) -> f64 {
    if a != b {
        if !a.is_nan() {
            return if a > b { a } else { b };
        }
        return a;
    }
    if b.is_sign_negative() {
        a
    } else {
        b
    }
}

/// `Math.Round(value, digits)` of .NET (midpoints to even, scaled by a power of ten).
pub fn round_digits(value: f64, digits: i32) -> f64 {
    if value.abs() < 1e16 {
        let power10 = 10f64.powi(digits);
        let scaled = value * power10;
        return scaled.round_ties_even() / power10;
    }
    value
}

/// ACadSharp `XY`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Xy {
    pub x: f64,
    pub y: f64,
}

impl Xy {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn get_angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    pub fn sub(self, o: Xy) -> Xy {
        Xy::new(self.x - o.x, self.y - o.y)
    }
    pub fn add(self, o: Xy) -> Xy {
        Xy::new(self.x + o.x, self.y + o.y)
    }
    pub fn length(self) -> f64 {
        let mut length = 0.0;
        length += self.x * self.x;
        length += self.y * self.y;
        f64::sqrt(length)
    }
    pub fn distance_from(self, o: Xy) -> f64 {
        self.sub(o).length()
    }
    pub fn round(self, digits: i32) -> Xy {
        Xy::new(round_digits(self.x, digits), round_digits(self.y, digits))
    }
    pub fn to_xyz(self) -> Xyz {
        Xyz::new(self.x, self.y, 0.0)
    }
}

/// ACadSharp `XYZ`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Xyz {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Xyz {
    pub const ZERO: Xyz = Xyz::new(0.0, 0.0, 0.0);
    pub const AXIS_X: Xyz = Xyz::new(1.0, 0.0, 0.0);
    pub const AXIS_Y: Xyz = Xyz::new(0.0, 1.0, 0.0);
    pub const AXIS_Z: Xyz = Xyz::new(0.0, 0.0, 1.0);

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub const fn splat(v: f64) -> Self {
        Self::new(v, v, v)
    }
    pub fn xy(self) -> Xy {
        Xy::new(self.x, self.y)
    }
    pub fn add(self, o: Xyz) -> Xyz {
        Xyz::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    pub fn sub(self, o: Xyz) -> Xyz {
        Xyz::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    pub fn neg(self) -> Xyz {
        Xyz::ZERO.sub(self)
    }
    pub fn mul(self, s: f64) -> Xyz {
        Xyz::new(self.x * s, self.y * s, self.z * s)
    }
    pub fn div(self, s: f64) -> Xyz {
        Xyz::new(self.x / s, self.y / s, self.z / s)
    }
    /// `VectorExtensions.Dot`: a left fold starting from `0.0`.
    pub fn dot(self, o: Xyz) -> f64 {
        let mut result = 0.0;
        result += self.x * o.x;
        result += self.y * o.y;
        result += self.z * o.z;
        result
    }
    pub fn length_squared(self) -> f64 {
        let mut length = 0.0;
        length += self.x * self.x;
        length += self.y * self.y;
        length += self.z * self.z;
        length
    }
    pub fn length(self) -> f64 {
        f64::sqrt(self.length_squared())
    }
    pub fn normalize(self) -> Xyz {
        let length = self.length();
        Xyz::new(self.x / length, self.y / length, self.z / length)
    }
    pub fn cross(a: Xyz, b: Xyz) -> Xyz {
        Xyz::new(
            a.y * b.z - a.z * b.y,
            a.z * b.x - a.x * b.z,
            a.x * b.y - a.y * b.x,
        )
    }
    pub fn distance_from(self, o: Xyz) -> f64 {
        self.sub(o).length()
    }
    /// `VectorExtensions.RoundZero` with the default threshold.
    pub fn round_zero(self) -> Xyz {
        let z = |v: f64| if is_zero(v, EPSILON) { 0.0 } else { v };
        Xyz::new(z(self.x), z(self.y), z(self.z))
    }
    pub fn round(self, digits: i32) -> Xyz {
        Xyz::new(
            round_digits(self.x, digits),
            round_digits(self.y, digits),
            round_digits(self.z, digits),
        )
    }
    pub fn is_nan(self) -> bool {
        self.x.is_nan() || self.y.is_nan() || self.z.is_nan()
    }
    pub fn component(self, i: usize) -> f64 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}

/// ACadSharp `XYZM` (homogeneous vector used by the matrix products).
#[derive(Clone, Copy)]
struct Xyzm([f64; 4]);

impl Xyzm {
    fn dot(self, o: Xyzm) -> f64 {
        let mut result = 0.0;
        for i in 0..4 {
            result += self.0[i] * o.0[i];
        }
        result
    }
}

/// ACadSharp `Matrix4`; `m[i][j]` is `Mij`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix4 {
    pub m: [[f64; 4]; 4],
}

impl Matrix4 {
    pub const IDENTITY: Matrix4 = Matrix4 {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// The C# constructor: arguments in the order `M00, M10, M20, M30, M01, ...`.
    #[allow(clippy::too_many_arguments)]
    fn from_args(
        m00: f64, m10: f64, m20: f64, m30: f64,
        m01: f64, m11: f64, m21: f64, m31: f64,
        m02: f64, m12: f64, m22: f64, m32: f64,
        m03: f64, m13: f64, m23: f64, m33: f64,
    ) -> Matrix4 {
        Matrix4 {
            m: [
                [m00, m01, m02, m03],
                [m10, m11, m12, m13],
                [m20, m21, m22, m23],
                [m30, m31, m32, m33],
            ],
        }
    }

    fn row(&self, r: usize) -> Xyzm {
        Xyzm([self.m[0][r], self.m[1][r], self.m[2][r], self.m[3][r]])
    }

    fn col(&self, c: usize) -> Xyzm {
        Xyzm(self.m[c])
    }

    /// `Matrix4 * Matrix4`.
    pub fn mul(&self, b: &Matrix4) -> Matrix4 {
        let mut result = Matrix4 { m: [[0.0; 4]; 4] };
        for r in 0..4 {
            let row = self.row(r);
            for c in 0..4 {
                result.m[c][r] = row.dot(b.col(c));
            }
        }
        result
    }

    /// `Matrix4 * XYZ`.
    pub fn mul_xyz(&self, v: Xyz) -> Xyz {
        let h = Xyzm([v.x, v.y, v.z, 1.0]);
        Xyz::new(self.row(0).dot(h), self.row(1).dot(h), self.row(2).dot(h))
    }

    pub fn create_translation(p: Xyz) -> Matrix4 {
        let mut r = Matrix4::IDENTITY;
        r.m[3][0] = p.x;
        r.m[3][1] = p.y;
        r.m[3][2] = p.z;
        r
    }

    pub fn create_scale(s: Xyz) -> Matrix4 {
        // CreateScale(scale, XYZ.Zero): the centre terms are `0 * (1 - s)`.
        let tx = 0.0 * (1.0 - s.x);
        let ty = 0.0 * (1.0 - s.y);
        let tz = 0.0 * (1.0 - s.z);
        let mut r = Matrix4::IDENTITY;
        r.m[0][0] = s.x;
        r.m[1][1] = s.y;
        r.m[2][2] = s.z;
        r.m[3][0] = tx;
        r.m[3][1] = ty;
        r.m[3][2] = tz;
        r
    }

    pub fn create_from_quaternion(q: Quaternion) -> Matrix4 {
        let xx = q.x * q.x;
        let yy = q.y * q.y;
        let zz = q.z * q.z;
        let xy = q.x * q.y;
        let wz = q.z * q.w;
        let xz = q.z * q.x;
        let wy = q.y * q.w;
        let yz = q.y * q.z;
        let wx = q.x * q.w;
        let mut r = Matrix4::IDENTITY;
        r.m[0][0] = 1.0 - 2.0 * (yy + zz);
        r.m[0][1] = 2.0 * (xy + wz);
        r.m[0][2] = 2.0 * (xz - wy);
        r.m[1][0] = 2.0 * (xy - wz);
        r.m[1][1] = 1.0 - 2.0 * (zz + xx);
        r.m[1][2] = 2.0 * (yz + wx);
        r.m[2][0] = 2.0 * (xz + wy);
        r.m[2][1] = 2.0 * (yz - wx);
        r.m[2][2] = 1.0 - 2.0 * (yy + xx);
        r
    }

    pub fn create_from_axis_angle(axis: Xyz, angle: f64) -> Matrix4 {
        let axis = axis.normalize();
        let (x, y, z) = (axis.x, axis.y, axis.z);
        let sa = angle.sin();
        let ca = angle.cos();
        let (xx, yy, zz) = (x * x, y * y, z * z);
        let (xy, xz, yz) = (x * y, x * z, y * z);
        let mut r = Matrix4::IDENTITY;
        r.m[0][0] = xx + ca * (1.0 - xx);
        r.m[0][1] = xy - ca * xy + sa * z;
        r.m[0][2] = xz - ca * xz - sa * y;
        r.m[1][0] = xy - ca * xy - sa * z;
        r.m[1][1] = yy + ca * (1.0 - yy);
        r.m[1][2] = yz - ca * yz + sa * x;
        r.m[2][0] = xz - ca * xz + sa * y;
        r.m[2][1] = yz - ca * yz - sa * x;
        r.m[2][2] = zz + ca * (1.0 - zz);
        r
    }

    /// `Matrix4.GetArbitraryAxis(zaxis)`. The C# test `Math.Abs(z.X) < (1 / 64)` uses integer
    /// division, so the threshold is zero and the second branch is never taken.
    pub fn arbitrary_axis(zaxis: Xyz) -> Matrix4 {
        let zaxis = zaxis.normalize();
        if zaxis == Xyz::AXIS_Z {
            return Matrix4::IDENTITY;
        }
        if zaxis == Xyz::AXIS_Z.neg() {
            return Matrix4::arbitrary_axis_xz(Xyz::AXIS_X.neg(), zaxis);
        }
        let xaxis = if zaxis.x.abs() < 0.0 && zaxis.y.abs() < 0.0 {
            Xyz::cross(Xyz::AXIS_Y, zaxis)
        } else {
            Xyz::cross(Xyz::AXIS_Z, zaxis)
        };
        Matrix4::arbitrary_axis_xz(xaxis.normalize(), zaxis)
    }

    fn arbitrary_axis_xz(xaxis: Xyz, zaxis: Xyz) -> Matrix4 {
        let yaxis = Xyz::cross(zaxis, xaxis);
        Matrix4::from_args(
            xaxis.x, yaxis.x, zaxis.x, 0.0,
            xaxis.y, yaxis.y, zaxis.y, 0.0,
            xaxis.z, yaxis.z, zaxis.z, 0.0,
            0.0, 0.0, 0.0, 1.0,
        )
    }

    /// `Matrix4.GetDeterminant`.
    pub fn determinant(&self) -> f64 {
        let (a, b, c, d) = (self.m[0][0], self.m[1][0], self.m[2][0], self.m[3][0]);
        let (e, f, g, h) = (self.m[0][1], self.m[1][1], self.m[2][1], self.m[3][1]);
        let (i, j, k, l) = (self.m[0][2], self.m[1][2], self.m[2][2], self.m[3][2]);
        let (m, n, o, p) = (self.m[0][3], self.m[1][3], self.m[2][3], self.m[3][3]);
        let kp_lo = k * p - l * o;
        let jp_ln = j * p - l * n;
        let jo_kn = j * o - k * n;
        let ip_lm = i * p - l * m;
        let io_km = i * o - k * m;
        let in_jm = i * n - j * m;
        a * (f * kp_lo - g * jp_ln + h * jo_kn) - b * (e * kp_lo - g * ip_lm + h * io_km)
            + c * (e * jp_ln - f * ip_lm + h * in_jm)
            - d * (e * jo_kn - f * io_km + g * in_jm)
    }

    /// Whether `Matrix4.Inverse` succeeds (its determinant is not zero).
    fn invertible(&self) -> bool {
        let (a, b, c, d) = (self.m[0][0], self.m[0][1], self.m[0][2], self.m[0][3]);
        let (e, f, g, h) = (self.m[1][0], self.m[1][1], self.m[1][2], self.m[1][3]);
        let (i, j, k, l) = (self.m[2][0], self.m[2][1], self.m[2][2], self.m[2][3]);
        let (m, n, o, p) = (self.m[3][0], self.m[3][1], self.m[3][2], self.m[3][3]);
        let kp_lo = k * p - l * o;
        let jp_ln = j * p - l * n;
        let jo_kn = j * o - k * n;
        let ip_lm = i * p - l * m;
        let io_km = i * o - k * m;
        let in_jm = i * n - j * m;
        let a11 = f * kp_lo - g * jp_ln + h * jo_kn;
        let a12 = -(e * kp_lo - g * ip_lm + h * io_km);
        let a13 = e * jp_ln - f * ip_lm + h * in_jm;
        let a14 = -(e * jo_kn - f * io_km + g * in_jm);
        let det = a * a11 + b * a12 + c * a13 + d * a14;
        // `Math.Abs(det) < double.Epsilon` (the smallest subnormal) is `det == 0`.
        det.abs() >= f64::from_bits(1)
    }
}

/// ACadSharp `Quaternion`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quaternion {
    /// `CreateFromYawPitchRoll(XYZ)`: the parameters are `(pitch, yaw, roll)` = `(X, Y, Z)`.
    pub fn from_euler(angles: Xyz) -> Quaternion {
        let (pitch, yaw, roll) = (angles.x, angles.y, angles.z);
        let half_pitch = pitch * 0.5;
        let (sp, cp) = (half_pitch.sin(), half_pitch.cos());
        let half_yaw = yaw * 0.5;
        let (sy, cy) = (half_yaw.sin(), half_yaw.cos());
        let half_roll = roll * 0.5;
        let (sr, cr) = (half_roll.sin(), half_roll.cos());
        Quaternion {
            x: cy * sp * cr + sy * cp * sr,
            y: sy * cp * cr - cy * sp * sr,
            z: cy * cp * sr - sy * sp * cr,
            w: cy * cp * cr + sy * sp * sr,
        }
    }

    fn normalize(self) -> Quaternion {
        let mut length = 0.0;
        length += self.x * self.x;
        length += self.y * self.y;
        length += self.z * self.z;
        length += self.w * self.w;
        let length = f64::sqrt(length);
        Quaternion {
            x: self.x / length,
            y: self.y / length,
            z: self.z / length,
            w: self.w / length,
        }
    }

    /// `Quaternion.ToEulerAngles`.
    pub fn to_euler_angles(self) -> Xyz {
        let q = self.normalize();
        let sinr_cosp = 2.0 * (q.w * q.x + q.y * q.z);
        let cosr_cosp = 1.0 - 2.0 * (q.x * q.x + q.y * q.y);
        let ax = sinr_cosp.atan2(cosr_cosp);
        let sinp = 2.0 * (q.w * q.y - q.z * q.x);
        let ay = if sinp.abs() >= 1.0 {
            // `Math.Sign(NaN)` would throw in C#, but `|NaN| >= 1` is false, so NaN never gets here.
            std::f64::consts::PI / 2.0 * if sinp > 0.0 { 1.0 } else if sinp < 0.0 { -1.0 } else { 0.0 }
        } else {
            sinp.asin()
        };
        let siny_cosp = 2.0 * (q.w * q.z + q.x * q.y);
        let cosy_cosp = 1.0 - 2.0 * (q.y * q.y + q.z * q.z);
        let az = siny_cosp.atan2(cosy_cosp);
        Xyz::new(ax, ay, az)
    }
}

/// ACadSharp `Transform`.
#[derive(Debug, Clone, Copy)]
pub struct Transform {
    matrix: Matrix4,
    rotation: Xyz,
    scale: Xyz,
    translation: Xyz,
}

impl Default for Transform {
    fn default() -> Self {
        Transform::new()
    }
}

impl Transform {
    /// `new Transform()`.
    pub fn new() -> Transform {
        let mut t = Transform {
            matrix: Matrix4::IDENTITY,
            rotation: Xyz::ZERO,
            scale: Xyz::splat(1.0),
            translation: Xyz::ZERO,
        };
        t.update_matrix();
        t
    }

    /// `new Transform(translation, scale, rotation)`; the scale setter rejects a zero component.
    pub fn from_trs(translation: Xyz, scale: Xyz, rotation: Xyz) -> Result<Transform, SvgError> {
        let mut t = Transform::new();
        t.translation = translation;
        t.update_matrix();
        if scale.x == 0.0 || scale.y == 0.0 || scale.z == 0.0 {
            return Err(SvgError::argument("Scale value cannot be 0"));
        }
        t.scale = scale;
        t.update_matrix();
        t.rotation = rotation;
        t.update_matrix();
        Ok(t)
    }

    /// `new Transform(matrix)`: decomposes the matrix into translation, scale and Euler rotation.
    pub fn from_matrix(matrix: Matrix4) -> Transform {
        let (translation, scale, rotation) = match try_decompose(&matrix) {
            Some((t, s, q)) => (t, s, q),
            None => (Xyz::ZERO, Xyz::ZERO, Quaternion::default()),
        };
        Transform {
            matrix,
            translation,
            scale,
            rotation: rotation.to_euler_angles(),
        }
    }

    pub fn matrix(&self) -> &Matrix4 {
        &self.matrix
    }
    pub fn translation(&self) -> Xyz {
        self.translation
    }
    pub fn scale(&self) -> Xyz {
        self.scale
    }
    pub fn euler_rotation(&self) -> Xyz {
        self.rotation
    }

    /// `Transform.ApplyTransform(xyz)` with `roundZero` true.
    pub fn apply_transform(&self, v: Xyz) -> Xyz {
        self.matrix.mul_xyz(v).round_zero()
    }

    fn update_matrix(&mut self) {
        let translation = Matrix4::create_translation(self.translation);
        let rotation = Matrix4::create_from_quaternion(Quaternion::from_euler(self.rotation));
        let scale = Matrix4::create_scale(self.scale);
        self.matrix = translation.mul(&rotation).mul(&scale);
    }
}

/// `Transform.TryDecompose`.
fn try_decompose(source: &Matrix4) -> Option<(Xyz, Xyz, Quaternion)> {
    let mut matrix = *source;
    if matrix.m[3][3] == 0.0 {
        return None;
    }
    let mut matrix4_3 = matrix;
    matrix4_3.m[0][3] = 0.0;
    matrix4_3.m[1][3] = 0.0;
    matrix4_3.m[2][3] = 0.0;
    matrix4_3.m[3][3] = 1.0;
    if matrix4_3.determinant() == 0.0 {
        return None;
    }
    if matrix.m[0][3] != 0.0 || matrix.m[1][3] != 0.0 || matrix.m[2][3] != 0.0 {
        if !matrix.invertible() {
            return None;
        }
        matrix.m[0][3] = 0.0;
        matrix.m[1][3] = 0.0;
        matrix.m[2][3] = 0.0;
        matrix.m[3][3] = 1.0;
    }

    let mut translation = Xyz::ZERO;
    translation.x = matrix.m[3][0];
    matrix.m[3][0] = 0.0;
    translation.y = matrix.m[3][1];
    matrix.m[3][1] = 0.0;
    translation.z = matrix.m[3][2];
    matrix.m[3][2] = 0.0;

    let mut cols = [
        Xyz::new(matrix.m[0][0], matrix.m[0][1], matrix.m[0][2]),
        Xyz::new(matrix.m[1][0], matrix.m[1][1], matrix.m[1][2]),
        Xyz::new(matrix.m[2][0], matrix.m[2][1], matrix.m[2][2]),
    ];
    let mut scaling = Xyz::ZERO;
    let mut shear = Xyz::ZERO;

    scaling.x = cols[0].length();
    cols[0] = cols[0].normalize();
    shear.x = cols[0].dot(cols[1]);
    cols[1] = cols[1].mul(1.0).add(cols[0].mul(-shear.x));

    scaling.y = cols[1].length();
    cols[1] = cols[1].normalize();
    shear.y = cols[0].dot(cols[2]);
    cols[2] = cols[2].mul(1.0).add(cols[0].mul(-shear.y));

    shear.z = cols[1].dot(cols[2]);
    cols[2] = cols[2].mul(1.0).add(cols[1].mul(-shear.z));
    scaling.z = cols[2].length();
    cols[2] = cols[2].normalize();

    let rhs = Xyz::cross(cols[1], cols[2]);
    if cols[0].dot(rhs) < 0.0 {
        for col in cols.iter_mut() {
            scaling.x *= -1.0;
            col.x *= -1.0;
            col.y *= -1.0;
            col.z *= -1.0;
        }
    }

    let trace = cols[0].x + cols[1].y + cols[2].z + 1.0;
    let (qx, qy, qz, qw);
    if trace > 0.0 {
        let s = 0.5 / f64::sqrt(trace);
        qx = (cols[2].y - cols[1].z) * s;
        qy = (cols[0].z - cols[2].x) * s;
        qz = (cols[1].x - cols[0].y) * s;
        qw = 0.25 / s;
    } else if cols[0].x > cols[1].y && cols[0].x > cols[2].z {
        let s = f64::sqrt(1.0 + cols[0].x - cols[1].y - cols[2].z) * 2.0;
        qx = 0.25 * s;
        qy = (cols[0].y + cols[1].x) / s;
        qz = (cols[0].z + cols[2].x) / s;
        qw = (cols[2].y - cols[1].z) / s;
    } else if cols[1].y > cols[2].z {
        let s = f64::sqrt(1.0 + cols[1].y - cols[0].x - cols[2].z) * 2.0;
        qx = (cols[0].y + cols[1].x) / s;
        qy = 0.25 * s;
        qz = (cols[1].z + cols[2].y) / s;
        qw = (cols[0].z - cols[2].x) / s;
    } else {
        let s = f64::sqrt(1.0 + cols[2].z - cols[0].x - cols[1].y) * 2.0;
        qx = (cols[0].z + cols[2].x) / s;
        qy = (cols[1].z + cols[2].y) / s;
        qz = 0.25 * s;
        qw = (cols[1].x - cols[0].y) / s;
    }
    Some((
        translation,
        scaling,
        Quaternion {
            x: -qx,
            y: -qy,
            z: -qz,
            w: qw,
        },
    ))
}

/// `BoundingBoxExtent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    Null,
    Point,
    Finite,
    Infinite,
}

/// ACadSharp `BoundingBox`.
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub extent: Extent,
    pub min: Xyz,
    pub max: Xyz,
}

impl BoundingBox {
    pub const NULL: BoundingBox = BoundingBox {
        extent: Extent::Null,
        min: Xyz::splat(f64::NAN),
        max: Xyz::splat(f64::NAN),
    };
    pub const INFINITE: BoundingBox = BoundingBox {
        extent: Extent::Infinite,
        min: Xyz::splat(f64::NEG_INFINITY),
        max: Xyz::splat(f64::INFINITY),
    };

    pub fn from_point(p: Xyz) -> BoundingBox {
        BoundingBox {
            extent: Extent::Point,
            min: p,
            max: p,
        }
    }

    pub fn from_min_max(min: Xyz, max: Xyz) -> BoundingBox {
        BoundingBox {
            extent: if min == max { Extent::Point } else { Extent::Finite },
            min: Xyz::new(dmin(min.x, max.x), dmin(min.y, max.y), dmin(min.z, max.z)),
            max: Xyz::new(dmax(min.x, max.x), dmax(min.y, max.y), dmax(min.z, max.z)),
        }
    }

    pub fn from_points<I: IntoIterator<Item = Xyz>>(points: I) -> BoundingBox {
        let mut b = BoundingBox::NULL;
        for p in points {
            b = b.merge(BoundingBox::from_point(p));
        }
        b
    }

    pub fn length_x(&self) -> f64 {
        (self.max.x - self.min.x).abs()
    }

    pub fn length_y(&self) -> f64 {
        (self.max.y - self.min.y).abs()
    }

    pub fn merge(&self, other: BoundingBox) -> BoundingBox {
        if self.extent == Extent::Infinite || other.extent == Extent::Infinite {
            return BoundingBox::INFINITE;
        }
        if self.extent == Extent::Null {
            return other;
        }
        if other.extent == Extent::Null {
            return *self;
        }
        let min = Xyz::new(
            dmin(self.min.x, other.min.x),
            dmin(self.min.y, other.min.y),
            dmin(self.min.z, other.min.z),
        );
        let max = Xyz::new(
            dmax(self.max.x, other.max.x),
            dmax(self.max.y, other.max.y),
            dmax(self.max.z, other.max.z),
        );
        BoundingBox::from_min_max(min, max)
    }
}
