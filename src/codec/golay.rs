use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;

type Result<T, E = String> = std::result::Result<T, E>;

/// Golay(24,12) Systematic generator matrix part B (12x12)
/// Standard B matrix for the (24,12) code.
const GOLAY_B: [u16; 12] = [
    0x8ED, 0x1DB, 0x3B6, 0x76C, 0xED8, 0xDB5, 0xB6B, 0x6D7, 0xDAE, 0xB5D, 0x6BA, 0xD74,
];

/// Encodes 12 bits of data into a 24-bit codeword.
pub fn encode_codeword(data: u16) -> u32 {
    let mut parity = 0u16;
    for i in 0..12 {
        if (data >> (11 - i)) & 1 != 0 {
            parity ^= GOLAY_B[i];
        }
    }
    ((data as u32) << 12) | (parity as u32)
}

/// Computes the syndrome of a 24-bit received word.
fn compute_syndrome(received: u32) -> u16 {
    let data = (received >> 12) as u16;
    let parity = (received & 0xFFF) as u16;
    let mut expected_parity = 0u16;
    for i in 0..12 {
        if (data >> (11 - i)) & 1 != 0 {
            expected_parity ^= GOLAY_B[i];
        }
    }
    parity ^ expected_parity
}

/// Weight (number of set bits) of a 16-bit word.
fn weight12(n: u16) -> u32 {
    (n & 0xFFF).count_ones()
}

/// Decodes a 24-bit word and corrects up to 3 errors.
/// Returns the corrected 12-bit data and the number of corrected bits.
pub fn decode_codeword(received: u32) -> (u16, u32) {
    let s = compute_syndrome(received);
    if s == 0 {
        return ((received >> 12) as u16, 0);
    }

    // 1. Check if weight(s) <= 3
    if weight12(s) <= 3 {
        let corrected = received ^ (s as u32);
        return ((corrected >> 12) as u16, weight12(s));
    }

    // 2. Check if weight(s + Bi) <= 2
    for i in 0..12 {
        let si = s ^ GOLAY_B[i];
        if weight12(si) <= 2 {
            let error_pattern = (si as u32) | (1 << (23 - i));
            let corrected = received ^ error_pattern;
            return ((corrected >> 12) as u16, weight12(si) + 1);
        }
    }

    // Let's re-verify the decoding algorithm logic for Golay (24,12).
    // A more robust way is to check all 1-bit, 2-bit, 3-bit error patterns.
    // But there are 24C1 + 24C2 + 24C3 = 24 + 276 + 2024 = 2324 patterns.
    // That's actually small enough to precompute or check!

    // Let's use the "Algebraic decoding of the binary (24, 12, 8) Golay code" approach:
    // s' = s * B
    // if weight(s') <= 3, error is in the data part.
    // ...

    // For now, let's implement the simpler part and see if it covers basic cases.
    // Actually, I'll use a slightly more complete syndrome decoding.

    // If weight(s) > 3, we try s * B.
    let mut s_prime = 0u16;
    for i in 0..12 {
        let mut row_sum = 0u8;
        for j in 0..12 {
            if (s & (1 << (11 - j))) != 0 && (GOLAY_B[j] & (1 << (11 - i))) != 0 {
                row_sum ^= 1;
            }
        }
        if row_sum != 0 {
            s_prime |= 1 << (11 - i);
        }
    }

    if weight12(s_prime) <= 3 {
        let error_pattern = (s_prime as u32) << 12;
        let corrected = received ^ error_pattern;
        return ((corrected >> 12) as u16, weight12(s_prime));
    }

    for i in 0..12 {
        let s_prime_i = s_prime ^ GOLAY_B[i];
        if weight12(s_prime_i) <= 2 {
            let error_pattern = ((s_prime_i as u32) << 12) | (1 << (11 - i));
            let corrected = received ^ error_pattern;
            return ((corrected >> 12) as u16, weight12(s_prime_i) + 1);
        }
    }

    // If still not found, we can't correct more than 3 errors.
    // Return original (uncorrected) or just bail.
    ((received >> 12) as u16, 0)
}

pub fn golay_encode(data: &[u8]) -> Vec<u8> {
    // Process every 12 bits. 3 bytes of input = 24 bits = 2 codewords.
    // Wait, 12 bits is 1.5 bytes.
    // To keep it simple and bit-accurate with what Python might do:
    // We'll take 3 bytes of input (24 bits) and produce 6 bytes of output (48 bits = 2 codewords of 24 bits).
    // No, 12 bits -> 24 bits. So 2 bytes of input -> 1 codeword (if we ignore 4 bits) or 3 bytes -> 2 codewords.
    // Let's go with 3 bytes -> 2 codewords (3 bytes each) = 6 bytes.

    let mut encoded = Vec::with_capacity((data.len() + 2) / 3 * 6);
    let mut i = 0;
    while i < data.len() {
        let b1 = data[i];
        let b2 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b3 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        // Codeword 1: bits 0-11
        let w1 = ((b1 as u16) << 4) | ((b2 as u16) >> 4);
        // Codeword 2: bits 12-23
        let w2 = (((b2 & 0x0F) as u16) << 8) | (b3 as u16);

        let c1 = encode_codeword(w1);
        let c2 = encode_codeword(w2);

        encoded.extend_from_slice(&c1.to_be_bytes()[1..]); // 24 bits = 3 bytes
        encoded.extend_from_slice(&c2.to_be_bytes()[1..]); // 24 bits = 3 bytes

        i += 3;
    }
    encoded
}

pub fn golay_decode(data: &[u8]) -> Result<(Vec<u8>, usize), CodecError> {
    if data.len() % 6 != 0 {
        return Err(CodecError::InsufficientData(Some(
            "Invalid Golay data length: must be multiple of 6 bytes (2 codewords)".to_string(),
        )));
    }

    let mut decoded = Vec::with_capacity(data.len() / 6 * 3);
    let mut total_corrected = 0u32;

    for chunk in data.chunks(6) {
        if chunk.len() < 6 {
            break;
        }

        let mut c1_bytes = [0u8; 4];
        c1_bytes[1..4].copy_from_slice(&chunk[0..3]);
        let c1 = u32::from_be_bytes(c1_bytes);

        let mut c2_bytes = [0u8; 4];
        c2_bytes[1..4].copy_from_slice(&chunk[3..6]);
        let c2 = u32::from_be_bytes(c2_bytes);

        let (w1, n1) = decode_codeword(c1);
        let (w2, n2) = decode_codeword(c2);

        total_corrected += n1 + n2;

        // Reconstruct 3 bytes from two 12-bit words
        let b1 = (w1 >> 4) as u8;
        let b2 = (((w1 & 0xF) as u8) << 4) | ((w2 >> 8) as u8);
        let b3 = (w2 & 0xFF) as u8;

        decoded.push(b1);
        decoded.push(b2);
        decoded.push(b3);
    }

    Ok((decoded, total_corrected as usize))
}

pub struct Golay {
    _n: usize,
    _k: usize,
}

impl Golay {
    pub fn new(n: usize, k: usize) -> Self {
        Self { _n: n, _k: k }
    }
}

impl Encoding for Golay {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(golay_encode(&chunk)));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        let mut quality = 0.0;
        for chunk in chunks {
            let (d, corrected) = golay_decode(&chunk)?;
            res.push(Bytes::from(d));
            quality += corrected as f32;
        }
        Ok((res, quality))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golay_codeword_roundtrip() {
        let input = 0xABC;
        let encoded = encode_codeword(input);
        let (decoded, corrected) = decode_codeword(encoded);
        assert_eq!(decoded, input);
        assert_eq!(corrected, 0);
    }

    #[test]
    fn test_golay_correction_1bit() {
        let input = 0x123;
        let encoded = encode_codeword(input);

        for i in 0..24 {
            let noisy = encoded ^ (1 << i);
            let (decoded, corrected) = decode_codeword(noisy);
            assert_eq!(decoded, input, "Failed to correct bit {}", i);
            assert_eq!(corrected, 1);
        }
    }

    #[test]
    fn test_golay_correction_3bits() {
        let input = 0x555;
        let encoded = encode_codeword(input);

        // Error in bits 0, 5, 20
        let noisy = encoded ^ (1 << 0) ^ (1 << 5) ^ (1 << 20);
        let (decoded, corrected) = decode_codeword(noisy);
        assert_eq!(decoded, input);
        assert_eq!(corrected, 3);
    }

    #[test]
    fn test_golay_full_roundtrip() {
        let data = b"Hello Golays!";
        let encoded = golay_encode(data);
        assert_eq!(encoded.len(), (data.len() + 2) / 3 * 6);

        let (decoded, corrected) = golay_decode(&encoded).unwrap();
        // Since we pad with 0s to 3-byte boundaries, we might have extra zeros
        assert!(decoded.starts_with(data));
        assert_eq!(corrected, 0);
    }
}
