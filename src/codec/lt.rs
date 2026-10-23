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
    pub fn try_new(k: usize) -> anyhow::Result<Self> {
        if k == 0 || k > 65_536 {
            anyhow::bail!("k must be between 1 and 65536, got {k}");
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
        let limit = (k as f64 / s).round() as usize;

        for (d, item) in tau.iter_mut().enumerate().take(k + 1).skip(1) {
            // Guard against underflow with saturating_sub(1)
            if d < limit.saturating_sub(1) {
                *item = s / (k as f64) * (1.0 / d as f64);
            } else if d == limit {
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

        Ok(Self { k, cdf })
    }

    pub fn new(k: usize) -> Self {
        Self::try_new(k).expect("Invalid k for RobustSoliton")
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

#[inline]
fn xor_block(dest: &mut [u8], src: &[u8]) {
    let len = dest.len().min(src.len());
    let (d_chunks, d_rem) = dest[..len].as_chunks_mut::<32>();
    let (s_chunks, s_rem) = src[..len].as_chunks::<32>();
    for (d, s) in d_chunks.iter_mut().zip(s_chunks.iter()) {
        for i in 0..32 {
            d[i] ^= s[i];
        }
    }
    for (d, s) in d_rem.iter_mut().zip(s_rem.iter()) {
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
    pub fn try_new(mut data: Vec<u8>, symbol_size: usize) -> anyhow::Result<Self> {
        if symbol_size == 0 {
            anyhow::bail!("symbol_size must be greater than zero");
        }
        if data.is_empty() {
            anyhow::bail!("data must not be empty");
        }
        // Pad
        if !data.len().is_multiple_of(symbol_size) {
            let pad_len = symbol_size - (data.len() % symbol_size);
            data.extend(std::iter::repeat_n(0, pad_len));
        }

        let k = data.len() / symbol_size;
        if k == 0 || k > 65_536 {
            anyhow::bail!("k must be between 1 and 65536, got {k}");
        }
        let mut blocks = Vec::with_capacity(k);
        for i in 0..k {
            blocks.push(data[i * symbol_size..(i + 1) * symbol_size].to_vec());
        }

        Ok(Self {
            blocks,
            k,
            symbol_size,
            dist: RobustSoliton::try_new(k)?,
        })
    }

    pub fn new(data: Vec<u8>, symbol_size: usize) -> Self {
        Self::try_new(data, symbol_size).expect("Invalid parameters for LTEncoder")
    }

    pub fn encode(&self, repair_count: usize) -> Vec<Vec<u8>> {
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
    pub fn try_new(total_len: usize, symbol_size: usize) -> anyhow::Result<Self> {
        if symbol_size == 0 {
            anyhow::bail!("symbol_size must be greater than zero");
        }
        if total_len == 0 {
            anyhow::bail!("total_len must be greater than zero");
        }
        let k = total_len.div_ceil(symbol_size);
        if k > 65_536 {
            anyhow::bail!("k must be at most 65536, got {k}");
        }
        let mut block_deps = HashMap::new();
        for i in 0..k {
            block_deps.insert(i, HashSet::new());
        }

        Ok(Self {
            total_len,
            symbol_size,
            k,
            dist: RobustSoliton::try_new(k)?,
            blocks: HashMap::new(),
            graph: HashMap::new(),
            block_deps,
        })
    }

    pub fn new(total_len: usize, symbol_size: usize) -> Self {
        Self::try_new(total_len, symbol_size).expect("Invalid parameters for LTDecoder")
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
        } else {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_block_correctness() {
        // Test varying lengths to cover:
        // 1. Exact 32 bytes
        // 2. > 32 bytes with remainder
        // 3. < 32 bytes
        for len in [5, 32, 64, 75] {
            let mut dest = vec![0xAA; len];
            let src = vec![0x55; len];
            xor_block(&mut dest, &src);
            assert_eq!(dest, vec![0xFF; len]);

            // Test non-trivial XOR
            let mut dest2 = vec![0xF0; len];
            let src2 = vec![0x0F; len];
            xor_block(&mut dest2, &src2);
            assert_eq!(dest2, vec![0xFF; len]);

            // Test identical XOR giving 0
            xor_block(&mut dest2, &vec![0xFF; len]);
            assert_eq!(dest2, vec![0x00; len]);
        }
    }

    #[test]
    fn test_lt_encoder_decoder_peeling() {
        let original_data =
            b"This is a longer message used to test LT peeling decoder with k > 1 blocks!";
        let symbol_size = 16;
        let encoder = LTEncoder::new(original_data.to_vec(), symbol_size);
        let mut decoder = LTDecoder::new(original_data.len(), symbol_size);

        let packets = encoder.encode(30);

        // Drop some original systematic packets (e.g. esi 0 and 2) to force repair packets and XOR peeling
        for (esi, pkt) in packets.into_iter().enumerate() {
            if esi == 0 || esi == 2 {
                continue; // drop
            }
            decoder.decode(&pkt);
            if decoder.get_result().is_some() {
                break;
            }
        }

        assert_eq!(decoder.get_result().as_deref(), Some(&original_data[..]));
    }
}
