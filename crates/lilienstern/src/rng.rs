//! Seeded randomness (SplitMix64): the same seed gives the same game on every machine.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x5DEE_CE66_D1CE_4E5B)
    }

    /// Where the sequence is (for fingerprints of a game's state).
    pub fn state(&self) -> u64 {
        self.0
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in 0..n (0 when n is 0).
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// A value in [0, 1) for a lattice point, the same for the same seed.
pub fn hash2(seed: u64, x: i64, y: i64) -> f64 {
    let mut r = Rng::new(
        seed ^ (x as u64).wrapping_mul(0x9E37_79B1_85EB_CA87)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    );
    r.f64()
}

/// Smooth value noise in [0, 1), varying over about one unit.
pub fn noise(seed: u64, x: f64, y: f64) -> f64 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i64, y0 as i64);
    let a = hash2(seed, ix, iy);
    let b = hash2(seed, ix + 1, iy);
    let c = hash2(seed, ix, iy + 1);
    let d = hash2(seed, ix + 1, iy + 1);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Two octaves of [`noise`].
pub fn fbm(seed: u64, x: f64, y: f64) -> f64 {
    0.65 * noise(seed, x, y) + 0.35 * noise(seed.wrapping_add(1), 2.0 * x, 2.0 * y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_numbers() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let xs: Vec<u64> = (0..5).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..5).map(|_| b.next_u64()).collect();
        assert_eq!(xs, ys);
        assert_ne!(Rng::new(8).next_u64(), xs[0]);
        let f = Rng::new(1).f64();
        assert!((0.0..1.0).contains(&f));
    }

    #[test]
    fn noise_is_continuous() {
        let a = noise(3, 1.5, 2.5);
        let b = noise(3, 1.5001, 2.5);
        assert!((a - b).abs() < 1e-3);
        assert!((0.0..1.0).contains(&fbm(3, 10.2, -4.7)));
    }
}
