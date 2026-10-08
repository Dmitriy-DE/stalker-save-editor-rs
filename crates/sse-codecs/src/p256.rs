//! ECDSA P-256 signature verification without external cryptography code.
//!
//! Update manifests produced by the released C# editor may carry either IEEE P1363
//! `r || s` signatures or canonical RFC 3279 DER `SEQUENCE(INTEGER r, INTEGER s)`
//! signatures. This verifier accepts exactly those two encodings.

use core::cmp::Ordering;
use sse_core::{Error, Result};

const P: U256 = U256([
    0xFFFF_FFFF_FFFF_FFFF,
    0x0000_0000_FFFF_FFFF,
    0x0000_0000_0000_0000,
    0xFFFF_FFFF_0000_0001,
]);
const P_MINUS_TWO: U256 = U256([
    0xFFFF_FFFF_FFFF_FFFD,
    0x0000_0000_FFFF_FFFF,
    0x0000_0000_0000_0000,
    0xFFFF_FFFF_0000_0001,
]);
const N: U256 = U256([
    0xF3B9_CAC2_FC63_2551,
    0xBCE6_FAAD_A717_9E84,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_0000_0000,
]);
const N_MINUS_TWO: U256 = U256([
    0xF3B9_CAC2_FC63_254F,
    0xBCE6_FAAD_A717_9E84,
    0xFFFF_FFFF_FFFF_FFFF,
    0xFFFF_FFFF_0000_0000,
]);
const B: U256 = U256([
    0x3BCE_3C3E_27D2_604B,
    0x651D_06B0_CC53_B0F6,
    0xB3EB_BD55_7698_86BC,
    0x5AC6_35D8_AA3A_93E7,
]);
const GX: U256 = U256([
    0xF4A1_3945_D898_C296,
    0x7703_7D81_2DEB_33A0,
    0xF8BC_E6E5_63A4_40F2,
    0x6B17_D1F2_E12C_4247,
]);
const GY: U256 = U256([
    0xCBB6_4068_37BF_51F5,
    0x2BCE_3357_6B31_5ECE,
    0x8EE7_EB4A_7C0F_9E16,
    0x4FE3_42E2_FE1A_7F9B,
]);

const OID_EC_PUBLIC_KEY: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const OID_PRIME256V1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

/// Parsed affine P-256 public key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    x: U256,
    y: U256,
}

impl PublicKey {
    /// Parses a SubjectPublicKeyInfo PEM containing an uncompressed P-256 point.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed PEM/DER, a different algorithm/curve, a
    /// compressed point, the point at infinity, or a point not on the P-256 curve.
    pub fn from_pem(text: &str) -> Result<Self> {
        let der = decode_public_key_pem(text)?;
        let mut root = DerReader::new(&der);
        let sequence = root.element(0x30)?;
        if !root.is_empty() {
            return Err(Error::damaged("trailing DER after SubjectPublicKeyInfo"));
        }

        let mut spki = DerReader::new(sequence);
        let algorithm_sequence = spki.element(0x30)?;
        let bit_string = spki.element(0x03)?;
        if !spki.is_empty() {
            return Err(Error::damaged("trailing data inside SubjectPublicKeyInfo"));
        }

        let mut algorithm = DerReader::new(algorithm_sequence);
        if algorithm.element(0x06)? != OID_EC_PUBLIC_KEY {
            return Err(Error::damaged("public key algorithm is not id-ecPublicKey"));
        }
        if algorithm.element(0x06)? != OID_PRIME256V1 {
            return Err(Error::damaged("public key curve is not prime256v1"));
        }
        if !algorithm.is_empty() {
            return Err(Error::damaged("unexpected EC algorithm parameters"));
        }

        if bit_string.len() != 66 || bit_string.first().copied() != Some(0) {
            return Err(Error::damaged("invalid EC public-key BIT STRING"));
        }
        let point = bit_string
            .get(1..)
            .ok_or_else(|| Error::damaged("missing EC public-key point"))?;
        if point.first().copied() != Some(0x04) {
            return Err(Error::damaged("P-256 public key is not an uncompressed point"));
        }
        let x_bytes = point
            .get(1..33)
            .ok_or_else(|| Error::damaged("short P-256 x coordinate"))?;
        let y_bytes = point
            .get(33..65)
            .ok_or_else(|| Error::damaged("short P-256 y coordinate"))?;
        let x = U256::from_be_slice(x_bytes)?;
        let y = U256::from_be_slice(y_bytes)?;
        if x.is_zero() && y.is_zero() {
            return Err(Error::damaged("EC point at infinity is not a public key"));
        }
        if x.cmp(&P) != Ordering::Less || y.cmp(&P) != Ordering::Less {
            return Err(Error::damaged("EC public-key coordinate is outside the field"));
        }
        let point = Affine { x, y };
        if !point_on_curve(point) {
            return Err(Error::damaged("EC public key is not on P-256"));
        }
        Ok(Self { x, y })
    }

    /// Verifies an ECDSA signature over an already-computed SHA-256 digest.
    ///
    /// The signature may be either the 64-byte IEEE P1363 `r || s` form or canonical DER
    /// `SEQUENCE { INTEGER r, INTEGER s }`, matching `UpdateSignature.cs`.
    #[must_use]
    pub fn verify(&self, message_sha256: &[u8; 32], signature: &[u8]) -> bool {
        let Some((r, s)) = parse_signature(signature) else {
            return false;
        };
        if r.is_zero() || s.is_zero() || r.cmp(&N) != Ordering::Less || s.cmp(&N) != Ordering::Less {
            return false;
        }

        let Ok(mut z) = U256::from_be_slice(message_sha256) else {
            return false;
        };
        if z.cmp(&N) != Ordering::Less {
            let (reduced, borrow) = z.sub_raw(N);
            if borrow {
                return false;
            }
            z = reduced;
        }

        let w = pow_mod(s, N_MINUS_TWO, N);
        if w.is_zero() {
            return false;
        }
        let u1 = mul_mod(z, w, N);
        let u2 = mul_mod(r, w, N);

        let g = Affine { x: GX, y: GY };
        let q = Affine { x: self.x, y: self.y };
        let p1 = scalar_mul(g, u1);
        let p2 = scalar_mul(q, u2);
        let sum = jacobian_add(p1, p2);
        let Some(x) = jacobian_x(sum) else {
            return false;
        };

        let xr = if x.cmp(&N) == Ordering::Less {
            x
        } else {
            let (value, borrow) = x.sub_raw(N);
            if borrow {
                return false;
            }
            value
        };
        xr == r
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct U256([u64; 4]);

impl U256 {
    const ZERO: Self = Self([0, 0, 0, 0]);
    const ONE: Self = Self([1, 0, 0, 0]);

    fn from_be_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 32 {
            return Err(Error::damaged("integer does not fit 256 bits"));
        }
        let mut padded = [0_u8; 32];
        let start = 32_usize
            .checked_sub(bytes.len())
            .ok_or_else(|| Error::damaged("integer width overflow"))?;
        let target = padded
            .get_mut(start..)
            .ok_or_else(|| Error::damaged("integer padding range"))?;
        target.copy_from_slice(bytes);

        let l0 = read_be_u64(padded.get(24..32).ok_or_else(|| Error::damaged("integer limb 0"))?)?;
        let l1 = read_be_u64(padded.get(16..24).ok_or_else(|| Error::damaged("integer limb 1"))?)?;
        let l2 = read_be_u64(padded.get(8..16).ok_or_else(|| Error::damaged("integer limb 2"))?)?;
        let l3 = read_be_u64(padded.get(0..8).ok_or_else(|| Error::damaged("integer limb 3"))?)?;
        Ok(Self([l0, l1, l2, l3]))
    }

    fn limb(self, index: usize) -> u64 {
        self.0.get(index).copied().unwrap_or_default()
    }

    fn set_limb(&mut self, index: usize, value: u64) {
        if let Some(slot) = self.0.get_mut(index) {
            *slot = value;
        }
    }

    fn is_zero(self) -> bool {
        self == Self::ZERO
    }

    fn bit(self, index: usize) -> bool {
        let limb_index = index.checked_div(64).unwrap_or(usize::MAX);
        let shift = u32::try_from(index.checked_rem(64).unwrap_or_default()).unwrap_or_default();
        self.0
            .get(limb_index)
            .copied()
            .is_some_and(|limb| limb & 1_u64.checked_shl(shift).unwrap_or_default() != 0)
    }

    fn cmp(&self, other: &Self) -> Ordering {
        let mut index = 4_usize;
        while index != 0 {
            index = index.saturating_sub(1);
            match self.limb(index).cmp(&other.limb(index)) {
                Ordering::Equal => {}
                value => return value,
            }
        }
        Ordering::Equal
    }

    fn add_raw(self, other: Self) -> (Self, bool) {
        let mut out = Self::ZERO;
        let mut carry = false;
        let mut index = 0_usize;
        while index < 4 {
            let (a, c1) = self.limb(index).overflowing_add(other.limb(index));
            let (b, c2) = a.overflowing_add(u64::from(carry));
            out.set_limb(index, b);
            carry = c1 || c2;
            index = index.checked_add(1).unwrap_or(4);
        }
        (out, carry)
    }

    fn sub_raw(self, other: Self) -> (Self, bool) {
        let mut out = Self::ZERO;
        let mut borrow = false;
        let mut index = 0_usize;
        while index < 4 {
            let (a, b1) = self.limb(index).overflowing_sub(other.limb(index));
            let (b, b2) = a.overflowing_sub(u64::from(borrow));
            out.set_limb(index, b);
            borrow = b1 || b2;
            index = index.checked_add(1).unwrap_or(4);
        }
        (out, borrow)
    }
}

fn read_be_u64(bytes: &[u8]) -> Result<u64> {
    let array = <[u8; 8]>::try_from(bytes).map_err(|_| Error::damaged("short u64"))?;
    Ok(u64::from_be_bytes(array))
}

fn modulus_complement(modulus: U256) -> U256 {
    let (inverted_plus_one, _) =
        U256([!modulus.limb(0), !modulus.limb(1), !modulus.limb(2), !modulus.limb(3)]).add_raw(U256::ONE);
    inverted_plus_one
}

fn add_mod(a: U256, b: U256, modulus: U256) -> U256 {
    let (sum, carry) = a.add_raw(b);
    if carry {
        let (reduced, carry_again) = sum.add_raw(modulus_complement(modulus));
        if carry_again {
            return reduced;
        }
        return reduced;
    }
    if sum.cmp(&modulus) != Ordering::Less {
        let (reduced, borrow) = sum.sub_raw(modulus);
        if !borrow {
            return reduced;
        }
    }
    sum
}

fn sub_mod(a: U256, b: U256, modulus: U256) -> U256 {
    let (difference, borrow) = a.sub_raw(b);
    if !borrow {
        return difference;
    }
    let (distance, borrow_again) = b.sub_raw(a);
    if borrow_again {
        return U256::ZERO;
    }
    let (value, borrow_mod) = modulus.sub_raw(distance);
    if borrow_mod {
        U256::ZERO
    } else {
        value
    }
}

fn mul_mod(a: U256, b: U256, modulus: U256) -> U256 {
    let mut result = U256::ZERO;
    let mut addend = a;
    let mut bit = 0_usize;
    while bit < 256 {
        if b.bit(bit) {
            result = add_mod(result, addend, modulus);
        }
        addend = add_mod(addend, addend, modulus);
        bit = bit.checked_add(1).unwrap_or(256);
    }
    result
}

fn pow_mod(base: U256, exponent: U256, modulus: U256) -> U256 {
    let mut result = U256::ONE;
    let mut bit = 256_usize;
    while bit != 0 {
        bit = bit.saturating_sub(1);
        result = mul_mod(result, result, modulus);
        if exponent.bit(bit) {
            result = mul_mod(result, base, modulus);
        }
    }
    result
}

fn field_square(value: U256) -> U256 {
    mul_mod(value, value, P)
}

fn field_mul(a: U256, b: U256) -> U256 {
    mul_mod(a, b, P)
}

fn field_double(value: U256) -> U256 {
    add_mod(value, value, P)
}

fn field_triple(value: U256) -> U256 {
    add_mod(field_double(value), value, P)
}

fn field_quad(value: U256) -> U256 {
    field_double(field_double(value))
}

fn field_oct(value: U256) -> U256 {
    field_double(field_quad(value))
}

#[derive(Clone, Copy, Debug)]
struct Affine {
    x: U256,
    y: U256,
}

#[derive(Clone, Copy, Debug)]
struct Jacobian {
    x: U256,
    y: U256,
    z: U256,
}

impl Jacobian {
    fn infinity() -> Self {
        Self {
            x: U256::ZERO,
            y: U256::ONE,
            z: U256::ZERO,
        }
    }

    fn from_affine(point: Affine) -> Self {
        Self {
            x: point.x,
            y: point.y,
            z: U256::ONE,
        }
    }

    fn is_infinity(self) -> bool {
        self.z.is_zero()
    }
}

fn point_on_curve(point: Affine) -> bool {
    if point.x.cmp(&P) != Ordering::Less || point.y.cmp(&P) != Ordering::Less {
        return false;
    }
    let y2 = field_square(point.y);
    let x2 = field_square(point.x);
    let x3 = field_mul(x2, point.x);
    let three_x = field_triple(point.x);
    let rhs = add_mod(sub_mod(x3, three_x, P), B, P);
    y2 == rhs
}

fn jacobian_double(point: Jacobian) -> Jacobian {
    if point.is_infinity() || point.y.is_zero() {
        return Jacobian::infinity();
    }

    let delta = field_square(point.z);
    let gamma = field_square(point.y);
    let beta = field_mul(point.x, gamma);
    let x_minus_delta = sub_mod(point.x, delta, P);
    let x_plus_delta = add_mod(point.x, delta, P);
    let alpha = field_triple(field_mul(x_minus_delta, x_plus_delta));
    let x3 = sub_mod(field_square(alpha), field_oct(beta), P);
    let z3 = sub_mod(sub_mod(field_square(add_mod(point.y, point.z, P)), gamma, P), delta, P);
    let four_beta_minus_x3 = sub_mod(field_quad(beta), x3, P);
    let y3 = sub_mod(field_mul(alpha, four_beta_minus_x3), field_oct(field_square(gamma)), P);
    Jacobian { x: x3, y: y3, z: z3 }
}

fn jacobian_add(left: Jacobian, right: Jacobian) -> Jacobian {
    if left.is_infinity() {
        return right;
    }
    if right.is_infinity() {
        return left;
    }

    let z1z1 = field_square(left.z);
    let z2z2 = field_square(right.z);
    let u1 = field_mul(left.x, z2z2);
    let u2 = field_mul(right.x, z1z1);
    let s1 = field_mul(left.y, field_mul(right.z, z2z2));
    let s2 = field_mul(right.y, field_mul(left.z, z1z1));

    if u1 == u2 {
        if s1 == s2 {
            return jacobian_double(left);
        }
        return Jacobian::infinity();
    }

    let h = sub_mod(u2, u1, P);
    let i = field_square(field_double(h));
    let j = field_mul(h, i);
    let r = field_double(sub_mod(s2, s1, P));
    let v = field_mul(u1, i);
    let x3 = sub_mod(sub_mod(field_square(r), j, P), field_double(v), P);
    let y3 = sub_mod(field_mul(r, sub_mod(v, x3, P)), field_double(field_mul(s1, j)), P);
    let z3 = field_mul(
        sub_mod(sub_mod(field_square(add_mod(left.z, right.z, P)), z1z1, P), z2z2, P),
        h,
    );
    Jacobian { x: x3, y: y3, z: z3 }
}

fn scalar_mul(base: Affine, scalar: U256) -> Jacobian {
    let mut result = Jacobian::infinity();
    let addend = Jacobian::from_affine(base);
    let mut bit = 256_usize;
    while bit != 0 {
        bit = bit.saturating_sub(1);
        result = jacobian_double(result);
        if scalar.bit(bit) {
            result = jacobian_add(result, addend);
        }
    }
    result
}

fn jacobian_x(point: Jacobian) -> Option<U256> {
    if point.is_infinity() {
        return None;
    }
    let z_inverse = pow_mod(point.z, P_MINUS_TWO, P);
    if z_inverse.is_zero() {
        return None;
    }
    Some(field_mul(point.x, field_square(z_inverse)))
}

fn parse_signature(signature: &[u8]) -> Option<(U256, U256)> {
    parse_der_signature(signature).or_else(|| parse_p1363_signature(signature))
}

fn parse_p1363_signature(signature: &[u8]) -> Option<(U256, U256)> {
    if signature.len() == 64 {
        let r = U256::from_be_slice(signature.get(0..32)?).ok()?;
        let s = U256::from_be_slice(signature.get(32..64)?).ok()?;
        return Some((r, s));
    }
    None
}

fn parse_der_signature(signature: &[u8]) -> Option<(U256, U256)> {
    let mut root = DerReader::new(signature);
    let sequence = root.element(0x30).ok()?;
    if !root.is_empty() {
        return None;
    }
    let mut content = DerReader::new(sequence);
    let r_bytes = content.integer().ok()?;
    let s_bytes = content.integer().ok()?;
    if !content.is_empty() {
        return None;
    }
    let r = U256::from_be_slice(r_bytes).ok()?;
    let s = U256::from_be_slice(s_bytes).ok()?;
    Some((r, s))
}

struct DerReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> DerReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    fn is_empty(&self) -> bool {
        self.position == self.data.len()
    }

    fn element(&mut self, tag: u8) -> Result<&'a [u8]> {
        if self.byte()? != tag {
            return Err(Error::damaged("unexpected DER tag"));
        }
        let length = self.length()?;
        self.take(length)
    }

    fn integer(&mut self) -> Result<&'a [u8]> {
        let bytes = self.element(0x02)?;
        if bytes.is_empty() {
            return Err(Error::damaged("empty DER INTEGER"));
        }
        let first = bytes
            .first()
            .copied()
            .ok_or_else(|| Error::damaged("empty DER INTEGER"))?;
        if first & 0x80 != 0 {
            return Err(Error::damaged("negative DER INTEGER"));
        }
        if first == 0 {
            if bytes.len() == 1 {
                return Ok(bytes);
            }
            let second = bytes
                .get(1)
                .copied()
                .ok_or_else(|| Error::damaged("short DER INTEGER"))?;
            if second & 0x80 == 0 {
                return Err(Error::damaged("non-canonical DER INTEGER leading zero"));
            }
            let stripped = bytes.get(1..).ok_or_else(|| Error::damaged("DER INTEGER range"))?;
            if stripped.len() > 32 {
                return Err(Error::damaged("DER INTEGER exceeds 256 bits"));
            }
            return Ok(stripped);
        }
        if bytes.len() > 32 {
            return Err(Error::damaged("DER INTEGER exceeds 256 bits"));
        }
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8> {
        let byte = self
            .data
            .get(self.position)
            .copied()
            .ok_or_else(|| Error::damaged("truncated DER"))?;
        self.position = self
            .position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("DER position overflow"))?;
        Ok(byte)
    }

    fn length(&mut self) -> Result<usize> {
        let first = self.byte()?;
        if first & 0x80 == 0 {
            return Ok(usize::from(first));
        }
        let count = usize::from(first & 0x7F);
        if count == 0 || count > core::mem::size_of::<usize>() {
            return Err(Error::damaged("invalid DER length-of-length"));
        }
        let mut length = 0_usize;
        let mut index = 0_usize;
        while index < count {
            let byte = self.byte()?;
            if index == 0 && byte == 0 {
                return Err(Error::damaged("non-canonical DER length"));
            }
            length = length
                .checked_shl(8)
                .and_then(|value| value.checked_add(usize::from(byte)))
                .ok_or_else(|| Error::damaged("DER length overflow"))?;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("DER length index overflow"))?;
        }
        if length < 128 {
            return Err(Error::damaged("non-canonical long-form DER length"));
        }
        Ok(length)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| Error::damaged("DER range overflow"))?;
        let value = self
            .data
            .get(self.position..end)
            .ok_or_else(|| Error::damaged("truncated DER element"))?;
        self.position = end;
        Ok(value)
    }
}

fn decode_public_key_pem(text: &str) -> Result<Vec<u8>> {
    const BEGIN: &str = "-----BEGIN PUBLIC KEY-----";
    const END: &str = "-----END PUBLIC KEY-----";

    let mut inside = false;
    let mut ended = false;
    let mut base64 = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !inside {
            if trimmed == BEGIN {
                inside = true;
            } else if !trimmed.is_empty() {
                return Err(Error::damaged("text before PUBLIC KEY PEM header"));
            }
        } else if trimmed == END {
            ended = true;
            inside = false;
        } else if ended {
            if !trimmed.is_empty() {
                return Err(Error::damaged("text after PUBLIC KEY PEM footer"));
            }
        } else {
            base64.push_str(trimmed);
        }
    }
    if !ended || inside {
        return Err(Error::damaged("PUBLIC KEY PEM footer is missing"));
    }
    crate::base64::decode_standard(&base64)
}

#[cfg(test)]
mod tests {
    use super::{
        field_mul, field_square, jacobian_add, jacobian_double, jacobian_x, parse_signature, pow_mod, scalar_mul,
        Affine, PublicKey, GX, GY, N, P, P_MINUS_TWO, U256,
    };
    use core::cmp::Ordering;

    const PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----\n\
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEugfpzNXArEpRIoUsxrmG6KWIWMFc\n\
DuEkmd6oGnQq6qsZmILc2fYC0wfqEMk/NB88BSFAC1N6fmziJf11RVtlLQ==\n\
-----END PUBLIC KEY-----\n";

    const RELEASE_DIGEST: [u8; 32] = [
        0xFC, 0x25, 0x8D, 0xD2, 0x76, 0x1A, 0x9E, 0x9F, 0x9A, 0xD6, 0x70, 0xF5, 0x4E, 0xCC, 0x24, 0x56, 0x0E, 0xDB,
        0x12, 0x1D, 0x12, 0x27, 0x18, 0x05, 0x1A, 0xA7, 0x03, 0x8E, 0xFC, 0x07, 0x28, 0xEA,
    ];

    const RELEASE_SIGNATURE_DER: [u8; 70] = [
        0x30, 0x44, 0x02, 0x20, 0x26, 0x1E, 0xB1, 0x50, 0xEA, 0x3B, 0x0C, 0xA4, 0xD3, 0x4A, 0x7D, 0x6C, 0x5F, 0x7E,
        0x6A, 0x35, 0x26, 0xAE, 0xF8, 0xD5, 0xC4, 0xD5, 0xB5, 0xD9, 0xE9, 0xB2, 0x6D, 0xAC, 0x4B, 0x5B, 0xF7, 0x67,
        0x02, 0x20, 0x56, 0xEB, 0x1E, 0x2D, 0xC0, 0x7F, 0xF8, 0xA8, 0x4D, 0x4A, 0xFB, 0x83, 0xE9, 0x35, 0xDF, 0x96,
        0xE6, 0x8E, 0x4D, 0x68, 0x86, 0x79, 0x51, 0xF2, 0xBF, 0x2B, 0x62, 0x8D, 0x8B, 0x73, 0xA1, 0x6A,
    ];

    #[test]
    fn release_manifest_signature_verifies() {
        let key = PublicKey::from_pem(PUBLIC_KEY).unwrap_or_else(|error| panic!("{error}"));
        assert!(key.verify(&RELEASE_DIGEST, &RELEASE_SIGNATURE_DER));
    }

    #[test]
    fn release_signature_accepts_p1363_form_too() {
        let key = PublicKey::from_pem(PUBLIC_KEY).unwrap_or_else(|error| panic!("{error}"));
        let parsed = parse_signature(&RELEASE_SIGNATURE_DER);
        let Some((r, s)) = parsed else {
            panic!("release DER did not parse");
        };
        let mut raw = [0_u8; 64];
        write_be_32(r, raw.get_mut(0..32).unwrap_or_else(|| panic!("r slice")));
        write_be_32(s, raw.get_mut(32..64).unwrap_or_else(|| panic!("s slice")));
        assert!(key.verify(&RELEASE_DIGEST, &raw));
    }

    #[test]
    fn flipped_digest_bit_fails() {
        let key = PublicKey::from_pem(PUBLIC_KEY).unwrap_or_else(|error| panic!("{error}"));
        let mut digest = RELEASE_DIGEST;
        if let Some(byte) = digest.get_mut(17) {
            *byte ^= 0x20;
        }
        assert!(!key.verify(&digest, &RELEASE_SIGNATURE_DER));
    }

    #[test]
    fn flipped_signature_bit_fails() {
        let key = PublicKey::from_pem(PUBLIC_KEY).unwrap_or_else(|error| panic!("{error}"));
        let mut signature = RELEASE_SIGNATURE_DER;
        if let Some(byte) = signature.get_mut(41) {
            *byte ^= 0x04;
        }
        assert!(!key.verify(&RELEASE_DIGEST, &signature));
    }

    #[test]
    fn rejects_zero_and_out_of_range_scalars() {
        let zero = [0_u8; 64];
        let (r, s) = parse_signature(&zero).unwrap_or_else(|| panic!("raw signature"));
        assert!(r.is_zero());
        assert!(s.is_zero());

        let mut n_bytes = [0_u8; 32];
        write_be_32(N, &mut n_bytes);
        let mut raw = [0_u8; 64];
        if let Some(r_part) = raw.get_mut(0..32) {
            r_part.copy_from_slice(&n_bytes);
        }
        if let Some(s_part) = raw.get_mut(32..64) {
            s_part.fill(1);
        }
        let (r, _) = parse_signature(&raw).unwrap_or_else(|| panic!("raw signature"));
        assert!(r.cmp(&N) != Ordering::Less);
    }

    #[test]
    fn jacobian_add_uses_doubling_when_u1_g_equals_u2_q() {
        let g = Affine { x: GX, y: GY };
        let two_g = scalar_mul(g, U256([2, 0, 0, 0]));
        let z_inv = pow_mod(two_g.z, P_MINUS_TWO, P);
        let q = Affine {
            x: field_mul(two_g.x, field_square(z_inv)),
            y: field_mul(two_g.y, field_mul(field_square(z_inv), z_inv)),
        };
        let p1 = scalar_mul(g, U256([6, 0, 0, 0]));
        let p2 = scalar_mul(q, U256([3, 0, 0, 0]));
        assert_eq!(jacobian_x(p1), jacobian_x(p2));
        assert_eq!(jacobian_x(jacobian_add(p1, p2)), jacobian_x(jacobian_double(p1)));
    }

    #[test]
    fn signature_parser_rejects_bad_der_and_p1363_lengths() {
        assert!(parse_signature(&[0_u8; 63]).is_none());
        assert!(parse_signature(&[0_u8; 65]).is_none());
        assert!(parse_signature(&[0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x02, 0x01]).is_none());
        assert!(parse_signature(&[0x30, 0x81, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01]).is_none());
        let mut oversized_integer = vec![0x30, 0x26, 0x02, 0x21];
        oversized_integer.extend_from_slice(&[1_u8; 33]);
        oversized_integer.extend_from_slice(&[0x02, 0x01, 0x01]);
        assert!(parse_signature(&oversized_integer).is_none());
    }

    #[test]
    fn parses_a_64_byte_der_signature_before_trying_p1363() {
        let mut signature = vec![0x30, 0x3e, 0x02, 0x1d];
        signature.extend_from_slice(&[1_u8; 29]);
        signature.extend_from_slice(&[0x02, 0x1d]);
        signature.extend_from_slice(&[2_u8; 29]);
        assert_eq!(signature.len(), 64);

        let (r, s) = parse_signature(&signature).unwrap_or_else(|| panic!("64-byte DER signature did not parse"));
        let expected_r = U256::from_be_slice(&[1_u8; 29]).unwrap_or_else(|error| panic!("r: {error}"));
        let expected_s = U256::from_be_slice(&[2_u8; 29]).unwrap_or_else(|error| panic!("s: {error}"));
        assert_eq!((r, s), (expected_r, expected_s));
    }

    #[test]
    fn rejects_noncanonical_der_integers() {
        let redundant_zero = [0x30, 0x07, 0x02, 0x02, 0x00, 0x01, 0x02, 0x01, 0x01];
        let negative = [0x30, 0x06, 0x02, 0x01, 0x80, 0x02, 0x01, 0x01];
        let trailing = [0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x01, 0x00];
        assert!(parse_signature(&redundant_zero).is_none());
        assert!(parse_signature(&negative).is_none());
        assert!(parse_signature(&trailing).is_none());
    }

    fn write_be_32(value: U256, out: &mut [u8]) {
        if out.len() != 32 {
            return;
        }
        let mut limb_index = 0_usize;
        while limb_index < 4 {
            let source = value.limb(limb_index).to_be_bytes();
            let from_end = limb_index.checked_add(1).and_then(|v| v.checked_mul(8)).unwrap_or(32);
            let start = 32_usize.saturating_sub(from_end);
            let end = start.checked_add(8).unwrap_or(32);
            if let Some(target) = out.get_mut(start..end) {
                target.copy_from_slice(&source);
            }
            limb_index = limb_index.checked_add(1).unwrap_or(4);
        }
    }
}
