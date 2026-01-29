use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct Scrambler {
    poly_mask: u64,
    seed: Option<u64>,
}

impl Scrambler {
    pub fn new(poly_mask: u64, seed: Option<u64>) -> Self {
        Self { poly_mask, seed }
    }
}

pub fn scr_xor(data: &[u8], poly_mask: u64, seed: Option<u64>) -> Vec<u8> {
    if poly_mask == 0 {
        return data.to_vec();
    }

    let width = 64 - poly_mask.leading_zeros();
    let mask = if let Some(seed) = seed {
        seed
    } else if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let mut state = mask;

    let mut res = Vec::with_capacity(data.len());
    for &b in data {
        let mut out_byte = 0u8;
        for i in (0..8).rev() {
            let mut feedback = 0u8;
            let mut temp = state & poly_mask;
            while temp != 0 {
                feedback ^= (temp & 1) as u8;
                temp >>= 1;
            }

            let bit = (b >> i) & 1;
            let scr_bit = bit ^ feedback;
            out_byte = (out_byte << 1) | scr_bit;

            state = ((state << 1) | (feedback as u64)) & mask;
            if state == 0 {
                state = mask;
            }
        }
        res.push(out_byte);
    }
    res
}

impl Encoding for Scrambler {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(scr_xor(&chunk, self.poly_mask, self.seed)));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        for chunk in chunks {
            res.push(Bytes::from(scr_xor(&chunk, self.poly_mask, self.seed)));
        }
        Ok((res, 1.0))
    }
}
