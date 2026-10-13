//! Pure conversions of copied native pose records. No engine update or memory store.
use crate::pose_buffers::PoseRecordBits;
use shikifsrs_runtime::{Error, Result, profile::CodeRange};

pub(crate) const TRANSFORM_EVIDENCE: &[CodeRange] = &[
    CodeRange {
        rva: 0xb459d0,
        length: 0xe1,
    },
    CodeRange {
        rva: 0x1681b70,
        length: 0xbc,
    },
    CodeRange {
        rva: 0x169f7d0,
        length: 0x91,
    },
    CodeRange {
        rva: 0x169f2b0,
        length: 0xb1,
    },
    CodeRange {
        rva: 0x16962a0,
        length: 0x3d,
    },
    CodeRange {
        rva: 0x16551c0,
        length: 0x1b6,
    },
    CodeRange {
        rva: 0x1654c30,
        length: 0x18,
    },
    CodeRange {
        rva: 0xb4b910,
        length: 0x5,
    },
    CodeRange {
        rva: 0xb50d60,
        length: 0xe,
    },
];

/// Coordinate space identifies the array's role, not initialization or freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoseSpace {
    Local,
    Model,
}

/// Native record blocks: translation xyzw, quaternion xyzw, and scale xyzw.
/// Translation/scale fourth lanes are preserved by copies and scaling, without meaning assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoseTransformBits(pub PoseRecordBits);

/// Three packed rows: each row's basis xyz followed by its translation component.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffineTransformBits(pub [[u32; 4]; 3]);

/// Four column vectors. Pure pose conversion emits basis xyz/zero and translation xyz/one.
/// A raw/native-produced value retains its actual words; it is not numerically certified.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixTransformBits(pub [[u32; 4]; 4]);

impl PoseTransformBits {
    pub fn translation(self) -> [f32; 3] {
        std::array::from_fn(|i| f32::from_bits(self.0.0[0][i]))
    }
    pub fn quaternion(self) -> [f32; 4] {
        self.0.0[1].map(f32::from_bits)
    }
    pub fn scale(self) -> [f32; 3] {
        std::array::from_fn(|i| f32::from_bits(self.0.0[2][i]))
    }

    /// Multiply only translation xyz of this copied baseline; quaternion, scale and w unchanged.
    /// Zero/signed-zero are retained; finite nonzero components may not underflow to zero.
    pub fn multiplied_translation(self, factor: f32) -> Result<Self> {
        self.multiply_block(0, factor)
    }

    /// Multiply only scale xyz of this copied baseline. Negative/zero native scales are allowed.
    /// This does not choose any root-motion, weight, collision, cloth or dirty-state behavior.
    pub fn multiplied_scale(self, factor: f32) -> Result<Self> {
        self.multiply_block(2, factor)
    }

    fn multiply_block(mut self, block: usize, factor: f32) -> Result<Self> {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(Error::InvalidProfile("invalid pose multiplier"));
        }
        for bits in &mut self.0.0[block][..3] {
            let original = f32::from_bits(*bits);
            let value = original * factor;
            if !original.is_finite() || !value.is_finite() || (original != 0.0 && value == 0.0) {
                return Err(Error::InvalidProfile("unrepresentable pose component"));
            }
            *bits = value.to_bits();
        }
        Ok(self)
    }

    /// Convert copied components into the native matrix packing. Does not normalize quaternion.
    /// Intended for finite components with the caller's existing native quaternion invariant.
    /// Rounding may differ from native SSE; no bitwise-equivalence or pose-update claim.
    pub fn matrix(self) -> Result<MatrixTransformBits> {
        let t = self.translation();
        let q = self.quaternion();
        let s = self.scale();
        if t.into_iter().chain(q).chain(s).any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile("nonfinite pose transform"));
        }
        let [x, y, z, w] = q;
        let xx = x * (x + x);
        let yy = y * (y + y);
        let zz = z * (z + z);
        let xy = x * (y + y);
        let xz = x * (z + z);
        let yz = y * (z + z);
        let wx = w * (x + x);
        let wy = w * (y + y);
        let wz = w * (z + z);
        let columns = [
            [
                (1.0 - zz - yy) * s[0],
                (xy + wz) * s[0],
                (xz - wy) * s[0],
                0.0,
            ],
            [
                (xy - wz) * s[1],
                (1.0 - zz - xx) * s[1],
                (yz + wx) * s[1],
                0.0,
            ],
            [
                (xz + wy) * s[2],
                (yz - wx) * s[2],
                (1.0 - xx - yy) * s[2],
                0.0,
            ],
            [t[0], t[1], t[2], 1.0],
        ];
        if columns.iter().flatten().any(|v| !v.is_finite()) {
            return Err(Error::InvalidProfile("unrepresentable pose matrix"));
        }
        Ok(MatrixTransformBits(
            columns.map(|column| column.map(f32::to_bits)),
        ))
    }

    /// Pack the first three matrix rows in the native affine getter's output order.
    pub fn affine(self) -> Result<AffineTransformBits> {
        Ok(self.matrix()?.affine())
    }
}

impl MatrixTransformBits {
    /// Packing only; preserves raw bits, including nonfinite values supplied by the caller.
    pub fn affine(self) -> AffineTransformBits {
        AffineTransformBits(std::array::from_fn(|row| {
            std::array::from_fn(|column| self.0[column][row])
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(t: [f32; 4], q: [f32; 4], s: [f32; 4]) -> PoseTransformBits {
        PoseTransformBits(PoseRecordBits([
            t.map(f32::to_bits),
            q.map(f32::to_bits),
            s.map(f32::to_bits),
        ]))
    }
    fn identity() -> PoseTransformBits {
        transform([0.0; 4], [0.0, 0.0, 0.0, 1.0], [1.0; 4])
    }
    fn apply(matrix: MatrixTransformBits, point: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|row| {
            (0..3)
                .map(|col| f32::from_bits(matrix.0[col][row]) * point[col])
                .sum::<f32>()
                + f32::from_bits(matrix.0[3][row])
        })
    }
    fn close(actual: [f32; 3], expected: [f64; 3], tolerance: f64) {
        for i in 0..3 {
            assert!(
                (f64::from(actual[i]) - expected[i]).abs() <= tolerance,
                "{actual:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn identity_translation_nonuniform_and_mirrored_scale_keep_column_convention() {
        let matrix = transform(
            [7.0, -3.0, 2.0, f32::NAN],
            [0.0, 0.0, 0.0, 1.0],
            [2.0, -3.0, 0.0, f32::NAN],
        )
        .matrix()
        .unwrap();
        assert_eq!(
            matrix.0.map(|c| c.map(f32::from_bits)),
            [
                [2.0, 0.0, 0.0, 0.0],
                [0.0, -3.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0],
                [7.0, -3.0, 2.0, 1.0]
            ]
        );
        close(apply(matrix, [4.0, 5.0, 6.0]), [15.0, -18.0, 2.0], 0.0);
        assert_eq!(std::mem::size_of::<AffineTransformBits>(), 48);
        assert_eq!(std::mem::size_of::<MatrixTransformBits>(), 64);
        assert_eq!(std::mem::align_of::<AffineTransformBits>(), 16);
        assert_eq!(std::mem::align_of::<MatrixTransformBits>(), 16);
    }

    #[test]
    fn quarter_turns_map_basis_directions_for_all_axes() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        for (q, point, expected) in [
            ([h, 0.0, 0.0, h], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, h, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
            ([0.0, 0.0, h, h], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -h, h], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        ] {
            close(
                apply(transform([0.0; 4], q, [1.0; 4]).matrix().unwrap(), point),
                expected,
                2e-7,
            );
        }
    }

    #[test]
    fn matrices_match_independent_quaternion_vector_rotation_with_scaled_points() {
        // Reference rotates a vector through two cross products in f64, not matrix equations.
        fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        }
        for seed in 1..128 {
            let raw = [
                seed as f32,
                (seed * 7 % 31) as f32 - 12.0,
                (seed * 3 % 19) as f32 - 7.0,
                3.0,
            ];
            let norm = raw.iter().map(|v| v * v).sum::<f32>().sqrt();
            let q = raw.map(|v| v / norm);
            let point = [2.5, -1.25, 3.0];
            let scale = [1.5, -0.5, 2.25, 0.0];
            let t = [3.0, -7.0, 4.0, 0.0];
            let v = std::array::from_fn(|i| f64::from(point[i]) * f64::from(scale[i]));
            let qxyz = [f64::from(q[0]), f64::from(q[1]), f64::from(q[2])];
            let uv = cross(qxyz, v);
            let uuv = cross(qxyz, uv);
            let reference = std::array::from_fn(|i| {
                v[i] + 2.0 * (f64::from(q[3]) * uv[i] + uuv[i]) + f64::from(t[i])
            });
            close(
                apply(transform(t, q, scale).matrix().unwrap(), point),
                reference,
                3e-6,
            );
        }
    }

    #[test]
    fn affine_pack_transposes_distinct_column_words_without_normalization() {
        let matrix = MatrixTransformBits(std::array::from_fn(|c| {
            std::array::from_fn(|r| (c * 4 + r) as u32 + 0x7fc00000)
        }));
        assert_eq!(
            matrix.affine().0,
            [
                [0x7fc00000, 0x7fc00004, 0x7fc00008, 0x7fc0000c],
                [0x7fc00001, 0x7fc00005, 0x7fc00009, 0x7fc0000d],
                [0x7fc00002, 0x7fc00006, 0x7fc0000a, 0x7fc0000e]
            ]
        );
    }

    #[test]
    fn component_scaling_uses_copied_baseline_and_preserves_unselected_raw_lanes() {
        let source = PoseTransformBits(PoseRecordBits([
            [
                (-0.0f32).to_bits(),
                2.0f32.to_bits(),
                (-3.0f32).to_bits(),
                0x7fc00123,
            ],
            [0x7fc02345; 4],
            [(-2.0f32).to_bits(), 0, 4.0f32.to_bits(), 0x7fc05678],
        ]));
        let translated = source.multiplied_translation(2.0).unwrap();
        assert_eq!(translated.translation(), [-0.0, 4.0, -6.0]);
        assert_eq!(translated.0.0[0][0], (-0.0f32).to_bits());
        assert_eq!(translated.0.0[0][3], 0x7fc00123);
        assert_eq!(&translated.0.0[1..], &source.0.0[1..]);
        assert_eq!(
            source.multiplied_translation(3.0).unwrap().translation(),
            [-0.0, 6.0, -9.0]
        );
        let scaled = source.multiplied_scale(0.5).unwrap();
        assert_eq!(scaled.scale(), [-1.0, 0.0, 2.0]);
        assert_eq!(&scaled.0.0[..2], &source.0.0[..2]);
        assert_eq!(scaled.0.0[2][3], source.0.0[2][3]);
    }

    #[test]
    fn scaling_and_conversion_reject_nonfinite_overflow_and_changed_nonzero_underflow() {
        for factor in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(identity().multiplied_scale(factor).is_err());
            assert!(identity().multiplied_translation(factor).is_err());
        }
        for (block, bits, factor) in [
            (0, f32::MAX.to_bits(), 2.0),
            (2, f32::MAX.to_bits(), 2.0),
            (0, 1, 0.25),
            (2, 1, 0.25),
            (0, f32::NAN.to_bits(), 1.0),
            (2, f32::INFINITY.to_bits(), 1.0),
        ] {
            let mut source = identity();
            source.0.0[block][0] = bits;
            assert!(source.multiply_block(block, factor).is_err());
        }
        for (block, lane) in [(0, 0), (0, 2), (1, 0), (1, 3), (2, 0), (2, 2)] {
            let mut source = identity();
            source.0.0[block][lane] = f32::NAN.to_bits();
            assert!(source.matrix().is_err());
            assert!(source.affine().is_err());
        }
        let mut source = identity();
        source.0.0[1][0] = f32::MAX.to_bits();
        assert!(source.matrix().is_err());
        source = identity();
        source.0.0[0][3] = f32::NAN.to_bits();
        source.0.0[2][3] = f32::INFINITY.to_bits();
        assert!(source.matrix().is_ok());
    }
}
