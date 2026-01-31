use crate::ContentEncoding;
use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;
use std::collections::{HashMap, HashSet};

/// SplitMix64 PRNG - Deterministic Cross-Language
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z = z ^ (z >> 31);
        z
    }
}

/// Robust Soliton Distribution
pub struct RobustSoliton {
    k: usize,
    cdf: Vec<f64>,
}

impl RobustSoliton {
    pub fn new(k: usize) -> Self {
        if k == 0 {
            return Self {
                k: 0,
                cdf: vec![1.0],
            };
        }
        let c = 0.1;
        let delta = 0.5;

        // Ideal Soliton
        let mut rho = vec![0.0; k + 1];
        rho[1] = 1.0 / (k as f64);
        for (d, item) in rho.iter_mut().enumerate().take(k + 1).skip(2) {
            *item = 1.0 / ((d * (d - 1)) as f64);
        }

        // Robust Component
        let mut tau = vec![0.0; k + 1];
        let s = c * (k as f64 / delta).ln() * (k as f64).sqrt();
        let limit = if s > 0.0 {
            (k as f64 / s).round() as usize
        } else {
            1
        };

        for (d, item) in tau.iter_mut().enumerate().take(k + 1).skip(1) {
            if d < limit.saturating_sub(1) {
                // d < K/S - 1? Wait, checking Py logic: d < round(k/s) - 1
                *item = s / (k as f64) * (1.0 / d as f64);
            } else if d == limit {
                // d == round(k/s) CHECKME py: d == round(k/s)
                *item = s * (s / delta).ln() / (k as f64);
            } else {
                *item = 0.0;
            }
        }

        let z: f64 = rho.iter().sum::<f64>() + tau.iter().sum::<f64>();

        let mut mu = vec![0.0; k + 1];
        for d in 1..=k {
            mu[d] = (rho[d] + tau[d]) / z;
        }

        let mut cdf = vec![0.0; k + 2];
        let mut current = 0.0;
        for d in 1..=k {
            current += mu[d];
            cdf[d] = current;
        }
        cdf[k + 1] = 1.0;

        Self { k, cdf }
    }

    pub fn sample(&self, prng: &mut SplitMix64) -> usize {
        let val = (prng.next_u64() as f64) / 18446744073709551616.0; // 2^64
        for d in 1..=self.k {
            if val < self.cdf[d] {
                return d;
            }
        }
        1
    }
}

fn xor_block(dest: &mut [u8], src: &[u8]) {
    for (d, s) in dest.iter_mut().zip(src.iter()) {
        *d ^= s;
    }
}

pub struct LTEncoder {
    blocks: Vec<Vec<u8>>,
    k: usize,
    symbol_size: usize,
    dist: RobustSoliton,
}

impl LTEncoder {
    pub fn new(mut data: Vec<u8>, symbol_size: usize) -> Self {
        if symbol_size == 0 || data.is_empty() {
            return Self {
                blocks: Vec::new(),
                k: 0,
                symbol_size: 0,
                dist: RobustSoliton::new(0),
            };
        }
        // Pad
        if !data.len().is_multiple_of(symbol_size) {
            let pad_len = symbol_size - (data.len() % symbol_size);
            data.extend(std::iter::repeat_n(0, pad_len));
        }

        let k = data.len() / symbol_size;
        let mut blocks = Vec::with_capacity(k);
        for i in 0..k {
            blocks.push(data[i * symbol_size..(i + 1) * symbol_size].to_vec());
        }

        Self {
            blocks,
            k,
            symbol_size,
            dist: RobustSoliton::new(k),
        }
    }

    pub fn encode(&self, repair_count: usize) -> Vec<Vec<u8>> {
        if self.k == 0 {
            return Vec::new();
        }
        let mut pkts = Vec::new();
        let total_count = self.k + repair_count;

        for esi in 0..total_count {
            let mut payload = vec![0u8; 4 + self.symbol_size];
            // Write ESI
            payload[0..4].copy_from_slice(&(esi as u32).to_be_bytes());

            if esi < self.k {
                // Systematic
                payload[4..].copy_from_slice(&self.blocks[esi]);
            } else {
                let mut prng = SplitMix64::new(esi as u64);
                let degree = self.dist.sample(&mut prng);

                let mut neighbors = HashSet::new();
                while neighbors.len() < degree {
                    let idx = (prng.next_u64() as usize) % self.k;
                    neighbors.insert(idx);
                }

                let mut first = true;
                let dest = &mut payload[4..];

                for idx in neighbors {
                    if first {
                        dest.copy_from_slice(&self.blocks[idx]);
                        first = false;
                    } else {
                        xor_block(dest, &self.blocks[idx]);
                    }
                }
            }
            pkts.push(payload);
        }
        pkts
    }
}

pub struct LTDecoder {
    total_len: usize,
    symbol_size: usize,
    k: usize,
    dist: RobustSoliton,

    blocks: HashMap<usize, Vec<u8>>,
    graph: HashMap<usize, (HashSet<usize>, Vec<u8>)>, // ESI -> (Neighbors, Payload)
    block_deps: HashMap<usize, HashSet<usize>>,       // Block ID -> Set of waiting Drop ESIs
}

impl LTDecoder {
    pub fn new(total_len: usize, symbol_size: usize) -> Self {
        if symbol_size == 0 || total_len == 0 {
            return Self {
                total_len: 0,
                symbol_size: 0,
                k: 0,
                dist: RobustSoliton::new(0),
                blocks: HashMap::new(),
                graph: HashMap::new(),
                block_deps: HashMap::new(),
            };
        }
        let k = total_len.div_ceil(symbol_size);
        let mut block_deps = HashMap::with_capacity(k);
        for i in 0..k {
            block_deps.insert(i, HashSet::new());
        }

        Self {
            total_len,
            symbol_size,
            k,
            dist: RobustSoliton::new(k),
            blocks: HashMap::new(),
            graph: HashMap::new(),
            block_deps,
        }
    }

    pub fn decode(&mut self, packet: &[u8]) -> bool {
        if packet.len() < 4 + self.symbol_size {
            return false;
        }

        let esi = u32::from_be_bytes(packet[0..4].try_into().unwrap()) as usize;
        let mut payload = packet[4..].to_vec();

        if payload.len() != self.symbol_size {
            return false;
        }

        if esi < self.k {
            if let std::collections::hash_map::Entry::Vacant(e) = self.blocks.entry(esi) {
                e.insert(payload);
                self.propagate(esi);
            }
        } else if self.k > 0 {
            let mut prng = SplitMix64::new(esi as u64);
            let degree = self.dist.sample(&mut prng);
            let mut neighbors = HashSet::new();
            while neighbors.len() < degree {
                let idx = (prng.next_u64() as usize) % self.k;
                neighbors.insert(idx);
            }

            let mut unknown_neighbors = HashSet::new();
            for idx in neighbors {
                if let Some(blk) = self.blocks.get(&idx) {
                    xor_block(&mut payload, blk);
                } else {
                    unknown_neighbors.insert(idx);
                }
            }

            if unknown_neighbors.is_empty() {
                // Redundant
            } else if unknown_neighbors.len() == 1 {
                let new_idx = *unknown_neighbors.iter().next().unwrap();
                if let std::collections::hash_map::Entry::Vacant(e) = self.blocks.entry(new_idx) {
                    e.insert(payload);
                    self.propagate(new_idx);
                }
            } else {
                // Store
                for &idx in &unknown_neighbors {
                    self.block_deps.entry(idx).or_default().insert(esi);
                }
                self.graph.insert(esi, (unknown_neighbors, payload));
            }
        }

        self.blocks.len() == self.k
    }

    fn propagate(&mut self, start_idx: usize) {
        let mut stack = vec![start_idx];

        while let Some(resolved_idx) = stack.pop() {
            if let Some(waiting_drops) = self.block_deps.remove(&resolved_idx) {
                let block_val = self.blocks.get(&resolved_idx).unwrap().clone();

                for esi in waiting_drops {
                    let mut solved_new_idx = None;

                    if let Some((neighbors, payload)) = self.graph.get_mut(&esi)
                        && neighbors.contains(&resolved_idx)
                    {
                        xor_block(payload, &block_val);
                        neighbors.remove(&resolved_idx);

                        if neighbors.len() == 1 {
                            solved_new_idx = Some(*neighbors.iter().next().unwrap());
                        }
                    }

                    if let Some(new_idx) = solved_new_idx {
                        // Move payload from graph to blocks
                        if let Some((_, payload)) = self.graph.remove(&esi)
                            && let std::collections::hash_map::Entry::Vacant(e) =
                                self.blocks.entry(new_idx)
                        {
                            e.insert(payload);
                            stack.push(new_idx);
                        }
                    }
                }
            }
        }
    }

    pub fn get_result(&self) -> Option<Vec<u8>> {
        if self.k == 0 {
            return Some(Vec::new());
        }
        if self.blocks.len() < self.k {
            return None;
        }
        let mut res = Vec::with_capacity(self.blocks.len() * self.symbol_size);
        for i in 0..self.k {
            res.extend_from_slice(self.blocks.get(&i).unwrap());
        }
        res.truncate(self.total_len);
        Some(res)
    }
}

pub struct LT {
    len: usize,
    mtu: u16,
    repair_count: u32,
}

impl LT {
    pub fn new(len: usize, mtu: u16, repair_count: u32) -> Self {
        Self {
            len,
            mtu,
            repair_count,
        }
    }
}

pub struct LTDynamic {
    mtu: u16,
    repair_count: u32,
}

impl LTDynamic {
    pub fn new(mtu: u16, repair_count: u32) -> Self {
        Self { mtu, repair_count }
    }
}

pub fn lt_encode(
    data: &[u8],
    original_count: usize,
    mtu: u16,
    repair_count: u32,
) -> Result<Vec<Bytes>, CodecError> {
    if mtu == 0 || original_count == 0 {
        return Ok(Vec::new());
    }
    let encoder = LTEncoder::new(data.to_vec(), mtu as usize);
    let packets = encoder.encode(repair_count as usize);

    Ok(packets.into_iter().map(Bytes::from).collect())
}

pub fn lt_decode(
    packets: Vec<Bytes>,
    original_count: usize,
    mtu: u16,
) -> Result<Vec<u8>, CodecError> {
    if mtu == 0 || original_count == 0 {
        return Ok(Vec::new());
    }
    let mut decoder = LTDecoder::new(original_count, mtu as usize);

    for packet_bytes in packets {
        if packet_bytes.len() < 4 + (mtu as usize) {
            continue;
        }
        decoder.decode(&packet_bytes);
    }

    if let Some(res) = decoder.get_result() {
        Ok(res)
    } else {
        Err(CodecError::InsufficientData(None))
    }
}

impl Codec for LT {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.extend(lt_encode(&chunk, self.len, self.mtu, self.repair_count)?);
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        if chunks.is_empty() {
            return Ok((Vec::new(), 0.0));
        }
        let ctx = chunks[0].0.clone();
        let inputs: Vec<Bytes> = chunks.into_iter().map(|(_, b)| b).collect();
        let res = lt_decode(inputs, self.len, self.mtu)?;
        Ok((vec![(ctx, Bytes::from(res))], 10.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}

impl Codec for LTDynamic {
    fn encode(&self, data: Vec<Bytes>, ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let len = chunk.len();
            let resolved = ContentEncoding::LT(len, self.mtu, self.repair_count);
            if ctx.current_index < ctx.encodings.len() {
                ctx.encodings[ctx.current_index] = resolved;
            }
            res.extend(lt_encode(&chunk, len, self.mtu, self.repair_count)?);
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        if chunks.is_empty() {
            return Ok((Vec::new(), 0.0));
        }
        let ctx = chunks[0].0.clone();
        let total_len: usize = chunks.iter().map(|(_, b)| b.len()).sum();
        let inputs: Vec<Bytes> = chunks.into_iter().map(|(_, b)| b).collect();
        let res = lt_decode(inputs, total_len, self.mtu)?;
        Ok((vec![(ctx, Bytes::from(res))], 10.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_robust_soliton_zero_k() {
        let rs = RobustSoliton::new(0);
        assert_eq!(rs.k, 0);
        // Should not panic on sample
        let mut prng = SplitMix64::new(42);
        assert_eq!(rs.sample(&mut prng), 1);
    }

    #[test]
    fn test_lt_encoder_decoder_zero_params() {
        let data = Vec::new();
        let encoder = LTEncoder::new(data, 100);
        assert_eq!(encoder.k, 0);
        let pkts = encoder.encode(10);
        assert!(pkts.is_empty());

        let decoder = LTDecoder::new(0, 100);
        assert_eq!(decoder.k, 0);
        assert!(decoder.get_result().is_some());
        assert!(decoder.get_result().unwrap().is_empty());
    }

    #[test]
    fn test_lt_decoder_zero_mtu() {
        let decoder = LTDecoder::new(100, 0);
        assert_eq!(decoder.k, 0);
        assert!(decoder.get_result().is_some());
    }
}
