use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;
use once_cell::sync::Lazy;

pub struct Conv {
    k: usize,
    rate: String,
}

impl Conv {
    pub fn new(k: usize, rate: String) -> Self {
        Self { k, rate }
    }
}

pub fn conv_encode(data: &[u8], k: usize, rate: &str) -> Result<Vec<u8>, CodecError> {
    if k != 7 || rate != "1/2" {
        return Err(CodecError::InvalidParameters(
            "Only conv(7, 1/2) is currently supported".to_string(),
        ));
    }

    let g1 = 0o133u8;
    let g2 = 0o171u8;
    let mut state = 0u8;
    let mut bits = Vec::with_capacity(data.len() * 8 * 2 + 14);

    let mut input_bits = Vec::with_capacity(data.len() * 8 + 6);
    for &b in data {
        for i in (0..8).rev() {
            input_bits.push((b >> i) & 1);
        }
    }
    input_bits.extend(std::iter::repeat_n(0, 6));

    for bit in input_bits {
        state = ((state << 1) | bit) & 0x7F;

        let mut p1 = 0u8;
        let mut p2 = 0u8;
        for i in 0..7 {
            if (g1 >> i) & 1 != 0 {
                p1 ^= (state >> i) & 1;
            }
            if (g2 >> i) & 1 != 0 {
                p2 ^= (state >> i) & 1;
            }
        }
        bits.push(p1);
        bits.push(p2);
        state &= 0x3F;
    }

    let mut res = Vec::with_capacity(bits.len().div_ceil(8));
    for chunk in bits.chunks(8) {
        let mut byte_val = 0u8;
        for (idx, &b) in chunk.iter().enumerate() {
            byte_val |= b << (7 - idx);
        }
        res.push(byte_val);
    }
    Ok(res)
}

struct ConvTransition {
    next_state: usize,
    p1: u8,
    p2: u8,
}

static CONV_TRANSITIONS: Lazy<[[ConvTransition; 2]; 64]> = Lazy::new(|| {
    let g1 = 0o133u8;
    let g2 = 0o171u8;
    let mut transitions = std::array::from_fn(|_| {
        std::array::from_fn(|_| ConvTransition {
            next_state: 0,
            p1: 0,
            p2: 0,
        })
    });

    for (s, rules) in transitions.iter_mut().enumerate() {
        for (bit, rule) in rules.iter_mut().enumerate() {
            let new_full_state = ((s as u8) << 1) | (bit as u8);
            let mut p1 = 0u8;
            let mut p2 = 0u8;
            for i in 0..7 {
                if (g1 >> i) & 1 != 0 {
                    p1 ^= (new_full_state >> i) & 1;
                }
                if (g2 >> i) & 1 != 0 {
                    p2 ^= (new_full_state >> i) & 1;
                }
            }
            *rule = ConvTransition {
                next_state: (new_full_state & 0x3F) as usize,
                p1,
                p2,
            };
        }
    }
    transitions
});

pub fn conv_decode(data: &[u8], k: usize, rate: &str) -> Result<(Vec<u8>, usize), CodecError> {
    if k != 7 || rate != "1/2" {
        return Err(CodecError::InvalidParameters(
            "Only conv(7, 1/2) is currently supported".to_string(),
        ));
    }

    let mut input_bits = Vec::with_capacity(data.len() * 8);
    for &b in data {
        for i in (0..8).rev() {
            input_bits.push((b >> i) & 1);
        }
    }

    let num_steps = input_bits.len() / 2;
    if num_steps == 0 {
        return Ok((Vec::new(), 0));
    }

    let num_states = 64;
    let mut metrics = vec![u32::MAX; num_states];
    metrics[0] = 0;

    let mut predecessor_states = vec![[0u8; 64]; num_steps];

    for step in 0..num_steps {
        let r1 = input_bits[step * 2];
        let r2 = input_bits[step * 2 + 1];

        let mut next_metrics = vec![u32::MAX; num_states];

        for s in 0..num_states {
            let current_metric = metrics[s];
            if current_metric == u32::MAX {
                continue;
            }

            let trans_bits = &CONV_TRANSITIONS[s];

            for (bit, trans) in trans_bits.iter().enumerate() {
                let dist = ((r1 ^ trans.p1) + (r2 ^ trans.p2)) as u32;
                let new_dist = current_metric + dist;

                if new_dist < next_metrics[trans.next_state] {
                    next_metrics[trans.next_state] = new_dist;
                    predecessor_states[step][trans.next_state] = (s as u8) << 1 | (bit as u8);
                }
            }
        }
        metrics = next_metrics;
    }

    let mut best_state = 0;
    let mut min_m = metrics[0];
    for (s, &m) in metrics.iter().enumerate() {
        if m < min_m {
            min_m = m;
            best_state = s;
        }
    }

    let mut decoded_bits = Vec::with_capacity(num_steps);
    let mut curr_s = best_state;
    for step in (0..num_steps).rev() {
        let entry = predecessor_states[step][curr_s];
        let prev_s = (entry >> 1) as usize;
        let bit = entry & 1;
        decoded_bits.push(bit);
        curr_s = prev_s;
    }
    decoded_bits.reverse();

    if decoded_bits.len() > 6 {
        decoded_bits.truncate(decoded_bits.len() - 6);
    } else {
        decoded_bits.clear();
    }

    let mut res = Vec::with_capacity(decoded_bits.len() / 8);
    for chunk in decoded_bits.chunks(8) {
        if chunk.len() < 8 {
            break;
        }
        let mut byte_val = 0u8;
        for (idx, &b) in chunk.iter().enumerate() {
            byte_val |= b << (7 - idx);
        }
        res.push(byte_val);
    }

    Ok((res, min_m as usize))
}

impl Encoding for Conv {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(conv_encode(&chunk, self.k, &self.rate)?));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        let mut quality = 0.0;
        for chunk in chunks {
            let (d, metric) = conv_decode(&chunk, self.k, &self.rate)?;
            let d_bytes = Bytes::from(d);
            quality += (d_bytes.len() * 8).saturating_sub(metric) as f32;
            res.push(d_bytes);
        }
        Ok((res, quality))
    }
}
