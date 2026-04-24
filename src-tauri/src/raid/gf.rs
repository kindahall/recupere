#![allow(dead_code)]
// ============================================================================
// Galois Field GF(2^8) arithmetic for RAID 6 Reed-Solomon reconstruction.
// Uses the same primitive polynomial (0x11d) as the Linux kernel libraid6.
// ============================================================================

use std::sync::OnceLock;

const POLY: u16 = 0x11d;

struct GfTables {
    log: [u8; 256],
    exp: [u8; 256],
}

fn tables() -> &'static GfTables {
    static T: OnceLock<GfTables> = OnceLock::new();
    T.get_or_init(|| {
        let mut log = [0u8; 256];
        let mut exp = [0u8; 256];
        let mut x: u16 = 1;
        for (i, slot) in exp.iter_mut().enumerate().take(255) {
            *slot = x as u8;
            log[x as usize] = i as u8;
            x <<= 1;
            if x & 0x100 != 0 {
                x ^= POLY;
            }
        }
        exp[255] = exp[0];
        GfTables { log, exp }
    })
}

#[inline]
pub fn mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let t = tables();
    let s = t.log[a as usize] as usize + t.log[b as usize] as usize;
    t.exp[s % 255]
}

#[inline]
pub fn div(a: u8, b: u8) -> u8 {
    assert!(b != 0, "GF division by zero");
    if a == 0 {
        return 0;
    }
    let t = tables();
    let s = (t.log[a as usize] as i32 - t.log[b as usize] as i32 + 255) % 255;
    t.exp[s as usize]
}

/// g^n in GF(2^8) (used for Q parity coefficients).
#[inline]
pub fn pow(n: usize) -> u8 {
    tables().exp[n % 255]
}

/// XOR a slice into another (P parity helper).
pub fn xor_into(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d ^= *s;
    }
}

/// Compute RAID 6 P parity (XOR of all data blocks).
pub fn compute_p(data: &[&[u8]]) -> Vec<u8> {
    let len = data[0].len();
    let mut p = vec![0u8; len];
    for block in data {
        xor_into(&mut p, block);
    }
    p
}

/// Compute RAID 6 Q parity using g^i coefficients.
pub fn compute_q(data: &[&[u8]]) -> Vec<u8> {
    let len = data[0].len();
    let mut q = vec![0u8; len];
    for (i, block) in data.iter().enumerate() {
        let coef = pow(i);
        for (qb, &db) in q.iter_mut().zip(block.iter()) {
            *qb ^= mul(coef, db);
        }
    }
    q
}

/// Reconstruct one missing data block from P parity (RAID 5 / RAID 6 single failure).
/// `known` are the surviving data blocks; `parity` is P.
pub fn reconstruct_from_p(known: &[&[u8]], parity: &[u8]) -> Vec<u8> {
    let len = parity.len();
    let mut out = parity.to_vec();
    for k in known {
        debug_assert_eq!(k.len(), len);
        xor_into(&mut out, k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf_mul_identity() {
        for x in 0u8..=255 {
            assert_eq!(mul(x, 1), x);
            assert_eq!(mul(1, x), x);
            assert_eq!(mul(x, 0), 0);
        }
    }

    #[test]
    fn gf_mul_div_inverse() {
        for x in 1u8..=255 {
            for y in 1u8..=255 {
                let p = mul(x, y);
                assert_eq!(div(p, y), x);
            }
        }
    }

    #[test]
    fn p_parity_roundtrip_single_failure() {
        let d0: Vec<u8> = (0..32).collect();
        let d1: Vec<u8> = (32..64).collect();
        let d2: Vec<u8> = (64..96).collect();
        let blocks: Vec<&[u8]> = vec![&d0, &d1, &d2];
        let p = compute_p(&blocks);
        // Drop d1, reconstruct from d0, d2 + p
        let recovered = reconstruct_from_p(&[&d0, &d2], &p);
        assert_eq!(recovered, d1);
    }

    #[test]
    fn q_parity_nontrivial() {
        let d0 = vec![0xFFu8; 16];
        let d1 = vec![0x01u8; 16];
        let blocks: Vec<&[u8]> = vec![&d0, &d1];
        let q = compute_q(&blocks);
        // Q must not equal P for distinct coefficients
        let p = compute_p(&blocks);
        assert_ne!(q, p);
    }
}
