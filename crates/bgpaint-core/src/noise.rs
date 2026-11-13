//! Deterministic hash noise used for brush and shape textures.

/// Hash of integer coordinates to `[0, 1)`.
#[inline]
pub fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Smooth value noise in `[0, 1)`; `scale` is the feature size in pixels.
#[inline]
pub fn value_noise(x: f32, y: f32, scale: f32, seed: u32) -> f32 {
    let (fx, fy) = (x / scale, y / scale);
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(tx), s(ty));
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// [`value_noise`] along row `y` from `x0`: calls `f(i, noise at x0 + i)` for `n` pixels. Same
/// values, but the lattice hashes are computed once per cell instead of per pixel.
pub fn value_noise_row(x0: i32, y: i32, n: usize, scale: f32, seed: u32, mut f: impl FnMut(usize, f32)) {
    let fy = y as f32 / scale;
    let iy = fy.floor() as i32;
    let ty = fy - fy.floor();
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let sy = s(ty);
    let mut cell = None;
    let (mut a, mut b, mut c, mut d) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let fx = (x0 + i as i32) as f32 / scale;
        let ix = fx.floor() as i32;
        if cell != Some(ix) {
            cell = Some(ix);
            (a, b) = (hash2(ix, iy, seed), hash2(ix + 1, iy, seed));
            (c, d) = (hash2(ix, iy + 1, seed), hash2(ix + 1, iy + 1, seed));
        }
        let sx = s(fx - fx.floor());
        let top = a + (b - a) * sx;
        let bottom = c + (d - c) * sx;
        f(i, top + (bottom - top) * sy);
    }
}

/// Small xorshift random generator.
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn noise_rows_match_pixels() {
        for (scale, seed) in [(3.0, 5), (2.5, 13), (14.0, 17)] {
            for y in [-7, 0, 5, 1001] {
                super::value_noise_row(-40, y, 300, scale, seed, |i, v| {
                    let x = -40 + i as i32;
                    assert_eq!(v, super::value_noise(x as f32, y as f32, scale, seed));
                });
            }
        }
    }

    use super::*;

    #[test]
    fn ranges() {
        let mut rng = Rng::new(7);
        for i in 0..1000 {
            let h = hash2(i, -i, 3);
            assert!((0.0..1.0).contains(&h));
            let v = value_noise(i as f32 * 0.7, i as f32 * 1.3, 4.0, 1);
            assert!((0.0..=1.0).contains(&v));
            assert!((0.0..1.0).contains(&rng.next_f32()));
        }
        assert_eq!(hash2(5, 6, 1), hash2(5, 6, 1));
    }
}
