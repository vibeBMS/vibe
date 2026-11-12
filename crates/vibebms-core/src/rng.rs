//! Deterministic PRNG compatible with `java.util.Random`.
//!
//! beatoraja (Java) seeds lane shuffles with `java.util.Random`, so using the
//! same generator lets replays store only a seed.

#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
}

const MULT: u64 = 0x5DEECE66D;
const MASK: u64 = (1 << 48) - 1;

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        Self { seed: (seed as u64 ^ MULT) & MASK }
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULT).wrapping_add(0xB)) & MASK;
        (self.seed >> (48 - bits)) as i64 as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// Uniform integer in `[0, bound)`. Mirrors `Random.nextInt(int)`.
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        assert!(bound > 0);
        if bound & (-bound) == bound {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % bound;
            if bits.wrapping_sub(val).wrapping_add(bound - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_f64(&mut self) -> f64 {
        let hi = (self.next(26) as i64) << 27;
        let lo = self.next(27) as i64;
        (hi + lo) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Fisher-Yates shuffle (same order as `Collections.shuffle(list, rnd)`).
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.next_int_bounded(i as i32 + 1) as usize;
            v.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_java_reference_values() {
        // new java.util.Random(42).nextInt() == -1170105035
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(), -1170105035);
        // new java.util.Random(42).nextInt(10) == 0
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int_bounded(10), 0);
    }
}
