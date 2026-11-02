//! Vectors, matrices, quaternions and planes with the conventions of
//! Direct3D, which the original engine used for everything in 3D.
//!
//! - **Left-handed**, **Y up**. A camera looks along `+Z`.
//! - Matrices are **row-major and applied to row vectors**: `v' = v * M`,
//!   the translation sits in the fourth row (`m[3][0..3]`). `A * B` means
//!   "A, then B".
//! - Angles are radians here; files and scripts carry degrees, converted
//!   with [`deg_to_rad`].
//! - Everything is `f32`, like the `D3DX` library, so that numbers match
//!   the original where it matters (walkability and placement).
//!
//! The functions are those of `D3DX` the engine calls (`LookAtLH`,
//! `PerspectiveFovLH`, `RotationYawPitchRoll`, `Inverse`, `Vec3Project`,
//! `PlaneFromPoints`, `PlaneIntersectLine`, `QuaternionRotationMatrix`,
//! `QuaternionSlerp`...), written from their public documentation, plus
//! the three helpers the engine wrote itself ([`intersect_triangle`],
//! [`pick_get_intersect`], [`decompose_simple`]), written with reference
//! to the MIT-licensed Wintermute Engine 1.x sources (`3DUtils.cpp`,
//! Copyright (c) 2003-2013 Jan Nedoma). See `NOTICE`.

use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// Pi as the `D3DX_PI` constant of the original (`f32`).
pub const PI: f32 = std::f32::consts::PI;

/// Degrees to radians, as the original's `DegToRad` macro computes it
/// (`value * pi * (1 / 180)`, in that order).
pub fn deg_to_rad(degrees: f32) -> f32 {
    degrees * PI * (1.0 / 180.0)
}

/// Radians to degrees (`value * (180 / pi)`).
pub fn rad_to_deg(radians: f32) -> f32 {
    radians * (180.0 / PI)
}

/// A point or direction in 3D.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vec3 {
    /// Right.
    pub x: f32,
    /// Up.
    pub y: f32,
    /// Away from a camera looking along `+Z`.
    pub z: f32,
}

impl Vec3 {
    /// The origin.
    pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
    /// The up axis.
    pub const UNIT_Y: Vec3 = Vec3::new(0.0, 1.0, 0.0);

    /// A vector from its components.
    pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// Dot product.
    pub fn dot(self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product (left-handed space: `x × y = z`, as in Direct3D).
    pub fn cross(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// The vector scaled to length one; a zero vector stays zero (the
    /// `D3DXVec3Normalize` behaviour for a null vector).
    pub fn normalized(self) -> Vec3 {
        let len = self.length();
        if len == 0.0 { self } else { self * (1.0 / len) }
    }

    /// Linear interpolation, `t = 0` gives `self`.
    pub fn lerp(self, other: Vec3, t: f32) -> Vec3 {
        self + (other - self) * t
    }

    /// Component-wise minimum.
    pub fn min(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.x.min(other.x),
            self.y.min(other.y),
            self.z.min(other.z),
        )
    }

    /// Component-wise maximum.
    pub fn max(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.x.max(other.x),
            self.y.max(other.y),
            self.z.max(other.z),
        )
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// A 4x4 matrix, row-major, applied to row vectors (`v' = v * M`).
///
/// `m[row][col]`; Direct3D's `_11` is `m[0][0]`, `_41` (the translation's
/// `x`) is `m[3][0]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    /// The rows.
    pub m: [[f32; 4]; 4],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    /// The identity.
    pub const IDENTITY: Mat4 = Mat4 {
        m: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// A matrix from its rows.
    pub const fn from_rows(m: [[f32; 4]; 4]) -> Mat4 {
        Mat4 { m }
    }

    /// `D3DXMatrixTranslation`.
    pub fn translation(t: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.m[3][0] = t.x;
        m.m[3][1] = t.y;
        m.m[3][2] = t.z;
        m
    }

    /// `D3DXMatrixScaling`.
    pub fn scaling(s: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.m[0][0] = s.x;
        m.m[1][1] = s.y;
        m.m[2][2] = s.z;
        m
    }

    /// A uniform scale.
    pub fn uniform_scaling(s: f32) -> Mat4 {
        Mat4::scaling(Vec3::new(s, s, s))
    }

    /// `D3DXMatrixRotationX`, `angle` in radians.
    pub fn rotation_x(angle: f32) -> Mat4 {
        let (s, c) = angle.sin_cos();
        Mat4::from_rows([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, c, s, 0.0],
            [0.0, -s, c, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// `D3DXMatrixRotationY`, `angle` in radians.
    pub fn rotation_y(angle: f32) -> Mat4 {
        let (s, c) = angle.sin_cos();
        Mat4::from_rows([
            [c, 0.0, -s, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [s, 0.0, c, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// `D3DXMatrixRotationZ`, `angle` in radians.
    pub fn rotation_z(angle: f32) -> Mat4 {
        let (s, c) = angle.sin_cos();
        Mat4::from_rows([
            [c, s, 0.0, 0.0],
            [-s, c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// `D3DXMatrixRotationYawPitchRoll`: roll about Z, then pitch about X,
    /// then yaw about Y (radians). The engine only ever passes a yaw: an
    /// object's heading is a rotation about the up axis.
    pub fn rotation_yaw_pitch_roll(yaw: f32, pitch: f32, roll: f32) -> Mat4 {
        Mat4::rotation_z(roll) * Mat4::rotation_x(pitch) * Mat4::rotation_y(yaw)
    }

    /// `D3DXMatrixLookAtLH`: the view matrix of a camera at `eye` looking
    /// at `at`, with `up` as the approximate up direction.
    pub fn look_at_lh(eye: Vec3, at: Vec3, up: Vec3) -> Mat4 {
        let zaxis = (at - eye).normalized();
        let xaxis = up.cross(zaxis).normalized();
        let yaxis = zaxis.cross(xaxis);
        Mat4::from_rows([
            [xaxis.x, yaxis.x, zaxis.x, 0.0],
            [xaxis.y, yaxis.y, zaxis.y, 0.0],
            [xaxis.z, yaxis.z, zaxis.z, 0.0],
            [-xaxis.dot(eye), -yaxis.dot(eye), -zaxis.dot(eye), 1.0],
        ])
    }

    /// `D3DXMatrixPerspectiveFovLH`: a perspective projection from the
    /// vertical field of view (radians), the aspect ratio (width over
    /// height) and the near and far planes; depth maps to `0..1`.
    pub fn perspective_fov_lh(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let y_scale = 1.0 / (fov_y / 2.0).tan();
        let x_scale = y_scale / aspect;
        Mat4::from_rows([
            [x_scale, 0.0, 0.0, 0.0],
            [0.0, y_scale, 0.0, 0.0],
            [0.0, 0.0, far / (far - near), 1.0],
            [0.0, 0.0, -near * far / (far - near), 0.0],
        ])
    }

    /// `D3DXMatrixTranspose`.
    pub fn transpose(&self) -> Mat4 {
        let mut t = [[0.0; 4]; 4];
        for (r, row) in self.m.iter().enumerate() {
            for (c, v) in row.iter().enumerate() {
                t[c][r] = *v;
            }
        }
        Mat4 { m: t }
    }

    /// The matrix as the 16 floats of a column-major GPU matrix for column
    /// vectors (`M * v`, as WGSL's `mat4x4<f32>`). That matrix is the
    /// transpose of this one, so its columns are these rows, in order: the
    /// translation lands in the last four floats.
    pub fn to_cols_array(&self) -> [f32; 16] {
        let mut out = [0.0; 16];
        for (r, row) in self.m.iter().enumerate() {
            out[r * 4..r * 4 + 4].copy_from_slice(row);
        }
        out
    }

    /// `D3DXMatrixDeterminant`.
    pub fn determinant(&self) -> f32 {
        let m = &self.m;
        let s0 = m[0][0] * m[1][1] - m[1][0] * m[0][1];
        let s1 = m[0][0] * m[1][2] - m[1][0] * m[0][2];
        let s2 = m[0][0] * m[1][3] - m[1][0] * m[0][3];
        let s3 = m[0][1] * m[1][2] - m[1][1] * m[0][2];
        let s4 = m[0][1] * m[1][3] - m[1][1] * m[0][3];
        let s5 = m[0][2] * m[1][3] - m[1][2] * m[0][3];
        let c5 = m[2][2] * m[3][3] - m[3][2] * m[2][3];
        let c4 = m[2][1] * m[3][3] - m[3][1] * m[2][3];
        let c3 = m[2][1] * m[3][2] - m[3][1] * m[2][2];
        let c2 = m[2][0] * m[3][3] - m[3][0] * m[2][3];
        let c1 = m[2][0] * m[3][2] - m[3][0] * m[2][2];
        let c0 = m[2][0] * m[3][1] - m[3][0] * m[2][1];
        s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0
    }

    /// `D3DXMatrixInverse`: `None` when the matrix is singular.
    pub fn inverse(&self) -> Option<Mat4> {
        let m = &self.m;
        let s0 = m[0][0] * m[1][1] - m[1][0] * m[0][1];
        let s1 = m[0][0] * m[1][2] - m[1][0] * m[0][2];
        let s2 = m[0][0] * m[1][3] - m[1][0] * m[0][3];
        let s3 = m[0][1] * m[1][2] - m[1][1] * m[0][2];
        let s4 = m[0][1] * m[1][3] - m[1][1] * m[0][3];
        let s5 = m[0][2] * m[1][3] - m[1][2] * m[0][3];
        let c5 = m[2][2] * m[3][3] - m[3][2] * m[2][3];
        let c4 = m[2][1] * m[3][3] - m[3][1] * m[2][3];
        let c3 = m[2][1] * m[3][2] - m[3][1] * m[2][2];
        let c2 = m[2][0] * m[3][3] - m[3][0] * m[2][3];
        let c1 = m[2][0] * m[3][2] - m[3][0] * m[2][2];
        let c0 = m[2][0] * m[3][1] - m[3][0] * m[2][1];
        let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
        if det == 0.0 {
            return None;
        }
        let inv = 1.0 / det;
        Some(Mat4::from_rows([
            [
                (m[1][1] * c5 - m[1][2] * c4 + m[1][3] * c3) * inv,
                (-m[0][1] * c5 + m[0][2] * c4 - m[0][3] * c3) * inv,
                (m[3][1] * s5 - m[3][2] * s4 + m[3][3] * s3) * inv,
                (-m[2][1] * s5 + m[2][2] * s4 - m[2][3] * s3) * inv,
            ],
            [
                (-m[1][0] * c5 + m[1][2] * c2 - m[1][3] * c1) * inv,
                (m[0][0] * c5 - m[0][2] * c2 + m[0][3] * c1) * inv,
                (-m[3][0] * s5 + m[3][2] * s2 - m[3][3] * s1) * inv,
                (m[2][0] * s5 - m[2][2] * s2 + m[2][3] * s1) * inv,
            ],
            [
                (m[1][0] * c4 - m[1][1] * c2 + m[1][3] * c0) * inv,
                (-m[0][0] * c4 + m[0][1] * c2 - m[0][3] * c0) * inv,
                (m[3][0] * s4 - m[3][1] * s2 + m[3][3] * s0) * inv,
                (-m[2][0] * s4 + m[2][1] * s2 - m[2][3] * s0) * inv,
            ],
            [
                (-m[1][0] * c3 + m[1][1] * c1 - m[1][2] * c0) * inv,
                (m[0][0] * c3 - m[0][1] * c1 + m[0][2] * c0) * inv,
                (-m[3][0] * s3 + m[3][1] * s1 - m[3][2] * s0) * inv,
                (m[2][0] * s3 - m[2][1] * s1 + m[2][2] * s0) * inv,
            ],
        ]))
    }

    /// `D3DXVec3Transform`: the row vector `(x, y, z, 1)` times the matrix,
    /// as four components.
    pub fn transform(&self, v: Vec3) -> [f32; 4] {
        let m = &self.m;
        [
            v.x * m[0][0] + v.y * m[1][0] + v.z * m[2][0] + m[3][0],
            v.x * m[0][1] + v.y * m[1][1] + v.z * m[2][1] + m[3][1],
            v.x * m[0][2] + v.y * m[1][2] + v.z * m[2][2] + m[3][2],
            v.x * m[0][3] + v.y * m[1][3] + v.z * m[2][3] + m[3][3],
        ]
    }

    /// `D3DXVec3TransformCoord`: transforms a point and divides by `w`.
    pub fn transform_coord(&self, v: Vec3) -> Vec3 {
        let [x, y, z, w] = self.transform(v);
        Vec3::new(x / w, y / w, z / w)
    }

    /// `D3DXVec3TransformNormal`: transforms a direction by the upper 3x3
    /// part only (no translation, no perspective divide).
    pub fn transform_normal(&self, v: Vec3) -> Vec3 {
        let m = &self.m;
        Vec3::new(
            v.x * m[0][0] + v.y * m[1][0] + v.z * m[2][0],
            v.x * m[0][1] + v.y * m[1][1] + v.z * m[2][1],
            v.x * m[0][2] + v.y * m[1][2] + v.z * m[2][2],
        )
    }

    /// The translation row.
    pub fn translation_part(&self) -> Vec3 {
        Vec3::new(self.m[3][0], self.m[3][1], self.m[3][2])
    }
}

impl Mul for Mat4 {
    type Output = Mat4;

    /// `D3DXMatrixMultiply`: `a * b` applies `a` first, then `b`.
    fn mul(self, b: Mat4) -> Mat4 {
        let mut out = [[0.0; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = self.m[r][0] * b.m[0][c]
                    + self.m[r][1] * b.m[1][c]
                    + self.m[r][2] * b.m[2][c]
                    + self.m[r][3] * b.m[3][c];
            }
        }
        Mat4 { m: out }
    }
}

impl MulAssign for Mat4 {
    fn mul_assign(&mut self, b: Mat4) {
        *self = *self * b;
    }
}

/// A rotation quaternion `(x, y, z, w)`, as `D3DXQUATERNION`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    /// Vector part, x.
    pub x: f32,
    /// Vector part, y.
    pub y: f32,
    /// Vector part, z.
    pub z: f32,
    /// Scalar part.
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl Quat {
    /// No rotation.
    pub const IDENTITY: Quat = Quat::new(0.0, 0.0, 0.0, 1.0);

    /// A quaternion from its components.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Quat {
        Quat { x, y, z, w }
    }

    /// Dot product.
    pub fn dot(self, o: Quat) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w
    }

    /// `D3DXQuaternionConjugate`: the vector part negated, the inverse
    /// rotation for a unit quaternion.
    pub fn conjugate(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Scaled to unit length; a zero quaternion stays zero.
    pub fn normalized(self) -> Quat {
        let len = self.dot(self).sqrt();
        if len == 0.0 {
            self
        } else {
            let inv = 1.0 / len;
            Quat::new(self.x * inv, self.y * inv, self.z * inv, self.w * inv)
        }
    }

    /// `D3DXQuaternionRotationMatrix`: the rotation of a matrix (its upper
    /// 3x3 part is expected to be a pure rotation).
    pub fn from_rotation_matrix(m: &Mat4) -> Quat {
        let m = &m.m;
        let trace = m[0][0] + m[1][1] + m[2][2];
        if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Quat::new(
                (m[1][2] - m[2][1]) / s,
                (m[2][0] - m[0][2]) / s,
                (m[0][1] - m[1][0]) / s,
                0.25 * s,
            )
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            Quat::new(
                0.25 * s,
                (m[0][1] + m[1][0]) / s,
                (m[0][2] + m[2][0]) / s,
                (m[1][2] - m[2][1]) / s,
            )
        } else if m[1][1] > m[2][2] {
            let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            Quat::new(
                (m[0][1] + m[1][0]) / s,
                0.25 * s,
                (m[1][2] + m[2][1]) / s,
                (m[2][0] - m[0][2]) / s,
            )
        } else {
            let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            Quat::new(
                (m[0][2] + m[2][0]) / s,
                (m[1][2] + m[2][1]) / s,
                0.25 * s,
                (m[0][1] - m[1][0]) / s,
            )
        }
    }

    /// `D3DXMatrixRotationQuaternion`: the rotation matrix of a unit
    /// quaternion, for row vectors.
    pub fn to_rotation_matrix(self) -> Mat4 {
        let Quat { x, y, z, w } = self;
        Mat4::from_rows([
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y + z * w),
                2.0 * (x * z - y * w),
                0.0,
            ],
            [
                2.0 * (x * y - z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z + x * w),
                0.0,
            ],
            [
                2.0 * (x * z + y * w),
                2.0 * (y * z - x * w),
                1.0 - 2.0 * (x * x + y * y),
                0.0,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    /// `D3DXQuaternionSlerp`: spherical interpolation along the shorter
    /// arc, `t = 0` gives `self`. Nearly parallel quaternions fall back to
    /// a linear interpolation.
    pub fn slerp(self, other: Quat, t: f32) -> Quat {
        let mut cos = self.dot(other);
        let mut b = other;
        if cos < 0.0 {
            cos = -cos;
            b = Quat::new(-b.x, -b.y, -b.z, -b.w);
        }
        let (ka, kb) = if 1.0 - cos > 0.001 {
            let theta = cos.acos();
            let sin = theta.sin();
            (((1.0 - t) * theta).sin() / sin, (t * theta).sin() / sin)
        } else {
            (1.0 - t, t)
        };
        Quat::new(
            ka * self.x + kb * b.x,
            ka * self.y + kb * b.y,
            ka * self.z + kb * b.z,
            ka * self.w + kb * b.w,
        )
    }
}

/// A plane `normal · p + d = 0`, as `D3DXPLANE`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    /// Unit normal.
    pub normal: Vec3,
    /// Signed distance term.
    pub d: f32,
}

impl Plane {
    /// `D3DXPlaneFromPoints`: the plane through three points, the normal
    /// being `normalize((v1 - v0) × (v2 - v0))`.
    pub fn from_points(v0: Vec3, v1: Vec3, v2: Vec3) -> Plane {
        let normal = (v1 - v0).cross(v2 - v0).normalized();
        Plane {
            normal,
            d: -normal.dot(v0),
        }
    }

    /// `D3DXPlaneDotCoord`: the signed distance of a point.
    pub fn dot_coord(&self, p: Vec3) -> f32 {
        self.normal.dot(p) + self.d
    }

    /// `D3DXPlaneIntersectLine`: where the **line** through `a` and `b`
    /// (not the segment) meets the plane; `None` when parallel.
    pub fn intersect_line(&self, a: Vec3, b: Vec3) -> Option<Vec3> {
        let dir = b - a;
        let denom = self.normal.dot(dir);
        if denom == 0.0 {
            return None;
        }
        let t = -self.dot_coord(a) / denom;
        Some(a + dir * t)
    }
}

/// The engine's ray/triangle test (`C3DUtils::IntersectTriangle`): a
/// Moeller-Trumbore test **without** the `t >= 0` check and
/// **single-sided**, hitting only when `dir` opposes the triangle's front
/// normal `(v1 - v0) × (v2 - v0)` (`det < 0.0001` fails, which also rejects
/// near-parallel rays). On a hit the result is the point where the **line**
/// through `orig` along `dir` meets the triangle's plane, which may lie
/// behind the origin.
///
/// This is the test under picking, the floor height and the path tests;
/// its quirks are part of the behaviour games were built on.
pub fn intersect_triangle(orig: Vec3, dir: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<Vec3> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let pvec = dir.cross(edge2);
    let det = edge1.dot(pvec);
    if det < 0.0001 {
        return None;
    }
    let tvec = orig - v0;
    let u = tvec.dot(pvec);
    if u < 0.0 || u > det {
        return None;
    }
    let qvec = tvec.cross(edge1);
    let v = dir.dot(qvec);
    if v < 0.0 || u + v > det {
        return None;
    }
    Plane::from_points(v0, v1, v2).intersect_line(orig, orig + dir)
}

/// The engine's segment/plane test (`C3DUtils::PickGetIntersect`): does the
/// segment `a..b` cross the **plane** of the triangle? Containment in the
/// triangle is not tested. Returns the crossing point and its position
/// along the segment (`0..1`); `None` when the segment is parallel to the
/// plane (`|dot| < 0.00001`) or does not reach it. The plane normal here is
/// `(v1 - v0) × (v2 - v1)`, normalised.
pub fn pick_get_intersect(a: Vec3, b: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<(Vec3, f32)> {
    let normal = (v1 - v0).cross(v2 - v1).normalized();
    let direction = b - a;
    let line_length = direction.dot(normal);
    if line_length.abs() < 0.00001 {
        return None;
    }
    let dist_from_plane = (v0 - a).dot(normal);
    let percentage = dist_from_plane / line_length;
    if !(0.0..=1.0).contains(&percentage) {
        return None;
    }
    Some((a + direction * percentage, percentage))
}

/// The engine's `C3DUtils::DecomposeMatrixSimple`: the translation row,
/// the scale as the lengths of the three **columns** of the upper 3x3 part
/// (as written in the original, not the rows), and the rotation quaternion
/// of the matrix taken as is (not divided by the scale).
pub fn decompose_simple(m: &Mat4) -> (Vec3, Vec3, Quat) {
    let t = m.translation_part();
    let r = &m.m;
    let scale = Vec3::new(
        (r[0][0] * r[0][0] + r[1][0] * r[1][0] + r[2][0] * r[2][0]).sqrt(),
        (r[0][1] * r[0][1] + r[1][1] * r[1][1] + r[2][1] * r[2][1]).sqrt(),
        (r[0][2] * r[0][2] + r[1][2] * r[1][2] + r[2][2] * r[2][2]).sqrt(),
    );
    (t, scale, Quat::from_rotation_matrix(m))
}

/// A viewport in pixels, as `D3DVIEWPORT` without the depth range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Viewport {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub width: i32,
    /// Height.
    pub height: i32,
}

impl Viewport {
    /// A viewport from its position and size.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Viewport {
        Viewport {
            x,
            y,
            width,
            height,
        }
    }
}

/// `D3DXVec3Project`: a world point to viewport pixels. The point goes
/// through `world * view * projection`, is divided by `w`, then mapped to
/// `x = vp.x + (1 + x) * width / 2`, `y = vp.y + (1 - y) * height / 2`; the
/// returned `z` is the depth in `0..1`.
pub fn project(v: Vec3, viewport: Viewport, projection: &Mat4, view: &Mat4, world: &Mat4) -> Vec3 {
    let clip = (*world * *view * *projection).transform_coord(v);
    Vec3::new(
        viewport.x as f32 + (1.0 + clip.x) * viewport.width as f32 / 2.0,
        viewport.y as f32 + (1.0 - clip.y) * viewport.height as f32 / 2.0,
        clip.z,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-4 * (1.0 + a.abs().max(b.abs()))
    }

    fn vec_close(a: Vec3, b: Vec3) -> bool {
        close(a.x, b.x) && close(a.y, b.y) && close(a.z, b.z)
    }

    #[test]
    fn degrees_convert_like_the_macro() {
        assert!(close(deg_to_rad(180.0), PI));
        assert!(close(rad_to_deg(PI / 2.0), 90.0));
        assert!(close(deg_to_rad(45.0), std::f32::consts::FRAC_PI_4));
    }

    #[test]
    fn the_cross_product_is_left_handed() {
        let x = Vec3::new(1.0, 0.0, 0.0);
        let y = Vec3::new(0.0, 1.0, 0.0);
        assert_eq!(x.cross(y), Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(Vec3::new(3.0, 4.0, 0.0).length(), 5.0);
        assert_eq!(Vec3::ZERO.normalized(), Vec3::ZERO);
        assert_eq!(x.lerp(y, 0.5), Vec3::new(0.5, 0.5, 0.0));
    }

    #[test]
    fn translation_sits_in_the_fourth_row_for_row_vectors() {
        let m = Mat4::translation(Vec3::new(10.0, 20.0, 30.0));
        assert_eq!(m.m[3][0], 10.0);
        let p = m.transform_coord(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(p, Vec3::new(11.0, 22.0, 33.0));
        // A normal ignores the translation.
        assert_eq!(m.transform_normal(Vec3::UNIT_Y), Vec3::UNIT_Y);
    }

    #[test]
    fn a_product_applies_the_left_factor_first() {
        let scale = Mat4::uniform_scaling(2.0);
        let shift = Mat4::translation(Vec3::new(1.0, 0.0, 0.0));
        // Scale, then shift: (1,0,0) -> (2,0,0) -> (3,0,0).
        let p = (scale * shift).transform_coord(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(p, Vec3::new(3.0, 0.0, 0.0));
        // Shift, then scale: (1,0,0) -> (2,0,0) -> (4,0,0).
        let p = (shift * scale).transform_coord(Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(p, Vec3::new(4.0, 0.0, 0.0));
    }

    #[test]
    fn rotations_follow_direct3d() {
        // Yaw by 90 degrees about Y turns +X into -Z in a left-handed space
        // (RotationY: _13 = -sin).
        let yaw = Mat4::rotation_yaw_pitch_roll(deg_to_rad(90.0), 0.0, 0.0);
        let p = yaw.transform_coord(Vec3::new(1.0, 0.0, 0.0));
        assert!(vec_close(p, Vec3::new(0.0, 0.0, -1.0)));
        // RotationZ by 90 degrees turns +X into +Y.
        let p = Mat4::rotation_z(deg_to_rad(90.0)).transform_coord(Vec3::new(1.0, 0.0, 0.0));
        assert!(vec_close(p, Vec3::new(0.0, 1.0, 0.0)));
        // RotationX by 90 degrees turns +Y into +Z.
        let p = Mat4::rotation_x(deg_to_rad(90.0)).transform_coord(Vec3::new(0.0, 1.0, 0.0));
        assert!(vec_close(p, Vec3::new(0.0, 0.0, 1.0)));
        // The heading convention of actors: at angle 0 a step is (0, 0, -1)
        // in world space, so a model facing -Z faces its walking direction.
        let angle = 0.0f32;
        let step = Vec3::new(-angle.sin(), 0.0, -angle.cos());
        assert_eq!(step, Vec3::new(0.0, 0.0, -1.0));
    }

    #[test]
    fn look_at_lh_puts_the_target_on_the_positive_z_axis() {
        let eye = Vec3::new(0.0, 100.0, -500.0);
        let at = Vec3::new(0.0, 0.0, 0.0);
        let view = Mat4::look_at_lh(eye, at, Vec3::UNIT_Y);
        let p = view.transform_coord(at);
        assert!(close(p.x, 0.0) && close(p.y, 0.0));
        assert!(close(p.z, (at - eye).length()));
        // The eye maps to the origin.
        assert!(vec_close(view.transform_coord(eye), Vec3::ZERO));
        // A point to the right of the camera has positive x.
        let p = view.transform_coord(Vec3::new(10.0, 0.0, -500.0));
        assert!(p.x > 0.0);
        // The inverse holds the camera position in its fourth row.
        let inv = view.inverse().unwrap();
        assert!(vec_close(inv.translation_part(), eye));
    }

    #[test]
    fn perspective_fov_lh_matches_the_documented_matrix() {
        let fov = deg_to_rad(90.0);
        let m = Mat4::perspective_fov_lh(fov, 2.0, 1.0, 11.0);
        assert!(close(m.m[1][1], 1.0)); // cot(45 deg)
        assert!(close(m.m[0][0], 0.5)); // divided by the aspect ratio
        assert!(close(m.m[2][2], 1.1)); // zf / (zf - zn)
        assert!(close(m.m[3][2], -1.1)); // -zn zf / (zf - zn)
        assert_eq!(m.m[2][3], 1.0);
        // The near plane maps to depth 0, the far plane to 1.
        assert!(close(m.transform_coord(Vec3::new(0.0, 0.0, 1.0)).z, 0.0));
        assert!(close(m.transform_coord(Vec3::new(0.0, 0.0, 11.0)).z, 1.0));
    }

    #[test]
    fn inverse_and_determinant() {
        let m = Mat4::rotation_y(0.7)
            * Mat4::uniform_scaling(3.0)
            * Mat4::translation(Vec3::new(1.0, 2.0, 3.0));
        assert!(close(m.determinant(), 27.0));
        let id = m * m.inverse().unwrap();
        for r in 0..4 {
            for c in 0..4 {
                assert!(close(id.m[r][c], Mat4::IDENTITY.m[r][c]), "{id:?}");
            }
        }
        assert!(Mat4::uniform_scaling(0.0).inverse().is_none());
        assert_eq!(m.transpose().transpose(), m);
        let cols = Mat4::translation(Vec3::new(5.0, 6.0, 7.0)).to_cols_array();
        assert_eq!(&cols[12..15], &[5.0, 6.0, 7.0]);
    }

    #[test]
    fn quaternions_round_trip_through_matrices() {
        let m = Mat4::rotation_yaw_pitch_roll(0.4, -0.3, 1.2);
        let q = Quat::from_rotation_matrix(&m);
        let back = q.to_rotation_matrix();
        for r in 0..3 {
            for c in 0..3 {
                assert!(close(m.m[r][c], back.m[r][c]), "{back:?}");
            }
        }
        // A quarter turn about Y.
        let q = Quat::from_rotation_matrix(&Mat4::rotation_y(PI / 2.0));
        assert!(close(q.y, (PI / 4.0).sin()) && close(q.w, (PI / 4.0).cos()));
        // Halfway slerp between identity and a half turn is a quarter turn.
        let half = Quat::from_rotation_matrix(&Mat4::rotation_y(PI));
        let mid = Quat::IDENTITY.slerp(half, 0.5).to_rotation_matrix();
        let p = mid.transform_coord(Vec3::new(1.0, 0.0, 0.0));
        assert!(vec_close(p, Vec3::new(0.0, 0.0, -1.0)) || vec_close(p, Vec3::new(0.0, 0.0, 1.0)));
        assert_eq!(Quat::IDENTITY.conjugate(), Quat::IDENTITY);
        assert!(close(Quat::new(0.0, 3.0, 0.0, 4.0).normalized().w, 0.8));
    }

    #[test]
    fn planes_and_lines() {
        let floor = Plane::from_points(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
        );
        assert!(vec_close(floor.normal, Vec3::UNIT_Y));
        assert!(close(floor.dot_coord(Vec3::new(5.0, 3.0, 5.0)), 3.0));
        let hit = floor
            .intersect_line(Vec3::new(1.0, 10.0, 1.0), Vec3::new(1.0, 9.0, 1.0))
            .unwrap();
        assert!(vec_close(hit, Vec3::new(1.0, 0.0, 1.0)));
        // The line, not the segment: the hit may be beyond the second point.
        let hit = floor
            .intersect_line(Vec3::new(0.0, 10.0, 0.0), Vec3::new(1.0, 9.0, 0.0))
            .unwrap();
        assert!(vec_close(hit, Vec3::new(10.0, 0.0, 0.0)));
        assert!(
            floor
                .intersect_line(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 0.0))
                .is_none()
        );
    }

    /// A floor triangle whose front normal points up: (0,0,0), (0,0,10),
    /// (10,0,0) gives (v1-v0) x (v2-v0) = (0,0,10) x (10,0,0) = (0,100,0).
    fn up_facing() -> (Vec3, Vec3, Vec3) {
        (
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 10.0),
            Vec3::new(10.0, 0.0, 0.0),
        )
    }

    #[test]
    fn intersect_triangle_is_single_sided_and_ignores_t() {
        let (v0, v1, v2) = up_facing();
        let down = Vec3::new(0.0, -1.0, 0.0);
        // From above, looking down: hit, at the plane point.
        let hit = intersect_triangle(Vec3::new(2.0, 5.0, 3.0), down, v0, v1, v2).unwrap();
        assert!(vec_close(hit, Vec3::new(2.0, 0.0, 3.0)));
        // From below, looking up: the direction follows the normal, no hit.
        assert!(intersect_triangle(Vec3::new(2.0, -5.0, 3.0), -down, v0, v1, v2).is_none());
        // From below, looking down: the line still crosses the triangle, and
        // the original does not check t, so this is a hit behind the origin.
        let hit = intersect_triangle(Vec3::new(2.0, -5.0, 3.0), down, v0, v1, v2).unwrap();
        assert!(vec_close(hit, Vec3::new(2.0, 0.0, 3.0)));
        // Outside the triangle: miss.
        assert!(intersect_triangle(Vec3::new(8.0, 5.0, 8.0), down, v0, v1, v2).is_none());
        // Flipped winding faces down: a downward ray misses it.
        assert!(intersect_triangle(Vec3::new(2.0, 5.0, 3.0), down, v0, v2, v1).is_none());
        // Nearly parallel: miss.
        assert!(
            intersect_triangle(
                Vec3::new(2.0, 5.0, 3.0),
                Vec3::new(1.0, 0.0, 0.0),
                v0,
                v1,
                v2
            )
            .is_none()
        );
    }

    #[test]
    fn pick_get_intersect_tests_the_plane_only() {
        let (v0, v1, v2) = up_facing();
        // Crosses the plane far outside the triangle: still a crossing.
        let (p, t) = pick_get_intersect(
            Vec3::new(50.0, 2.0, 50.0),
            Vec3::new(50.0, -2.0, 50.0),
            v0,
            v1,
            v2,
        )
        .unwrap();
        assert!(vec_close(p, Vec3::new(50.0, 0.0, 50.0)));
        assert!(close(t, 0.5));
        // Both ends above the plane: no crossing.
        assert!(
            pick_get_intersect(
                Vec3::new(1.0, 2.0, 1.0),
                Vec3::new(1.0, 1.0, 1.0),
                v0,
                v1,
                v2
            )
            .is_none()
        );
        // Parallel to the plane: none.
        assert!(
            pick_get_intersect(
                Vec3::new(1.0, 2.0, 1.0),
                Vec3::new(5.0, 2.0, 1.0),
                v0,
                v1,
                v2
            )
            .is_none()
        );
    }

    #[test]
    fn decompose_simple_reads_translation_scale_and_rotation() {
        let m = Mat4::uniform_scaling(2.0)
            * Mat4::rotation_y(0.5)
            * Mat4::translation(Vec3::new(1.0, 2.0, 3.0));
        let (t, s, q) = decompose_simple(&m);
        assert_eq!(t, Vec3::new(1.0, 2.0, 3.0));
        assert!(vec_close(s, Vec3::new(2.0, 2.0, 2.0)));
        // The quaternion is taken from the scaled matrix, as in the original.
        let expected = Quat::from_rotation_matrix(&m);
        assert_eq!(q, expected);
    }

    #[test]
    fn project_maps_clip_space_to_pixels() {
        let vp = Viewport::new(10, 20, 200, 100);
        let p = project(
            Vec3::ZERO,
            vp,
            &Mat4::IDENTITY,
            &Mat4::IDENTITY,
            &Mat4::IDENTITY,
        );
        assert_eq!(p, Vec3::new(110.0, 70.0, 0.0));
        // Clip (1, 1) is the top-right corner.
        let p = project(
            Vec3::new(1.0, 1.0, 0.5),
            vp,
            &Mat4::IDENTITY,
            &Mat4::IDENTITY,
            &Mat4::IDENTITY,
        );
        assert_eq!(p, Vec3::new(210.0, 20.0, 0.5));
    }
}
