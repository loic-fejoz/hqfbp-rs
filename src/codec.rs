use anyhow::{Result, anyhow, bail};
use crc::{Crc, CRC_16_XMODEM, CRC_32_ISO_HDLC};
use reed_solomon::Encoder as RSEncoder;
use reed_solomon::Decoder as RSDecoder;
use raptorq::{Encoder as RQEncoder, Decoder as RQDecoder};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Cursor, Read, Write};

pub const ENCODING_REGISTRY: &[(i8, &str)] = &[
    (-1, "h"),
    (0, "identity"),
    (1, "gzip"),
    (2, "deflate"),
    (3, "br"),
    (4, "lzma"),
    (5, "crc16"),
    (6, "crc32"),
];

pub fn crc16_ccitt(data: &[u8]) -> [u8; 2] {
    let crc = Crc::<u16>::new(&CRC_16_XMODEM);
    crc.checksum(data).to_be_bytes()
}

pub fn crc32_std(data: &[u8]) -> [u8; 4] {
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    crc.checksum(data).to_be_bytes()
}

pub fn gzip_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut res = Vec::new();
    decoder.read_to_end(&mut res)?;
    Ok(res)
}

pub fn brotli_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut res = Vec::new();
    let mut writer = brotli::CompressorWriter::new(&mut res, 4096, 6, 22);
    writer.write_all(data)?;
    writer.flush()?;
    drop(writer);
    Ok(res)
}

pub fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut res = Vec::new();
    let mut reader = brotli::Decompressor::new(data, 4096);
    reader.read_to_end(&mut res)?;
    Ok(res)
}

pub fn lzma_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut res = Vec::new();
    lzma_rs::lzma_compress(&mut Cursor::new(data), &mut res)
        .map_err(|e| anyhow!("LZMA compress failed: {}", e))?;
    Ok(res)
}

pub fn lzma_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut res = Vec::new();
    lzma_rs::lzma_decompress(&mut Cursor::new(data), &mut res)
        .map_err(|e| anyhow!("LZMA decompress failed: {}", e))?;
    Ok(res)
}

pub fn scr_xor(data: &[u8], poly_mask: u64) -> Vec<u8> {
    if poly_mask == 0 {
        return data.to_vec();
    }

    let width = 64 - poly_mask.leading_zeros();
    let mask = (1u64 << width) - 1;
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

pub fn rs_encode(data: &[u8], n: usize, k: usize) -> Result<Vec<u8>> {
    let encoder = RSEncoder::new(n - k);
    let mut encoded = Vec::with_capacity((data.len() + k - 1) / k * n);
    
    for chunk in data.chunks(k) {
        let mut block = chunk.to_vec();
        if block.len() < k {
            block.resize(k, 0);
        }
        let ecc = encoder.encode(&block);
        encoded.extend_from_slice(&block);
        encoded.extend_from_slice(&ecc);
    }
    Ok(encoded)
}

pub fn rs_decode(data: &[u8], n: usize, k: usize) -> Result<(Vec<u8>, usize)> {
    if data.len() % n != 0 {
        bail!("RS data length must be a multiple of n");
    }
    let decoder = RSDecoder::new(n - k);
    let mut decoded = Vec::with_capacity(data.len() / n * k);
    let mut total_corrected = 0;

    for chunk in data.chunks(n) {
        let mut block = chunk[..k].to_vec();
        let ecc = &chunk[k..];
        
        match decoder.correct(&mut block, Some(ecc)) {
            Ok(corrected) => {
                decoded.extend_from_slice(&block);
                total_corrected += corrected.len();
            }
            Err(_) => bail!("Reed-Solomon decoding failed"),
        }
    }
    Ok((decoded, total_corrected))
}

pub fn rq_encode(data: &[u8], original_count: usize, mtu: u16, repair_count: u32) -> Result<Vec<Vec<u8>>> {
    let mut padded_data = data.to_vec();
    padded_data.resize(original_count, 0);
    
    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let encoder = RQEncoder::new(&padded_data, oti);
    let packets = encoder.get_encoded_packets(repair_count);
    
    Ok(packets.into_iter().map(|p| p.serialize()).collect())
}

pub fn rq_decode(packets: Vec<Vec<u8>>, original_count: usize, mtu: u16) -> Result<Vec<u8>> {
    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let mut decoder = RQDecoder::new(oti);
    for packet_bytes in packets {
        let packet = raptorq::EncodingPacket::deserialize(&packet_bytes);
        if let Some(res) = decoder.decode(packet) {
            return Ok(res);
        }
    }
    bail!("RaptorQ decoding failed: insufficient symbols")
}

pub fn conv_encode(data: &[u8], k: usize, rate: &str) -> Result<Vec<u8>> {
    if k != 7 || rate != "1/2" {
        bail!("Only conv(7, 1/2) is currently supported");
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
    for _ in 0..6 {
        input_bits.push(0);
    }
    
    for bit in input_bits {
        state = ((state << 1) | bit) & 0x7F; // keep 7 bits for G1, G2
        // NASA polynomials G1=133, G2=171 octal
        // Constraint length 7 means state has 7 bits.
        // Python code uses `state &= 0x3F` at the END, so it keeps 6 bits of memory.
        // It shifts in the new bit at the START.
        
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
        state &= 0x3F; // Keep 6 bits for next iteration
    }
    
    let mut res = Vec::with_capacity((bits.len() + 7) / 8);
    for chunk in bits.chunks(8) {
        let mut byte_val = 0u8;
        for (idx, &b) in chunk.iter().enumerate() {
            byte_val |= b << (7 - idx);
        }
        res.push(byte_val);
    }
    Ok(res)
}

pub fn conv_decode(data: &[u8], k: usize, rate: &str) -> Result<(Vec<u8>, usize)> {
    if k != 7 || rate != "1/2" {
        bail!("Only conv(7, 1/2) is currently supported");
    }
    
    let g1 = 0o133u8;
    let g2 = 0o171u8;
    let num_states = 1 << (k - 1); // 64
    
    let mut transitions = vec![[(0usize, 0u8, 0u8); 2]; num_states];
    for s in 0..num_states {
        for bit in 0..2 {
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
            transitions[s][bit] = ((new_full_state & 0x3F) as usize, p1, p2);
        }
    }
    
    let mut metrics = vec![usize::MAX; num_states];
    metrics[0] = 0;
    let mut paths = vec![Vec::new(); num_states];
    
    let mut input_bits = Vec::with_capacity(data.len() * 8);
    for &b in data {
        for i in (0..8).rev() {
            input_bits.push((b >> i) & 1);
        }
    }
    
    for i in (0..input_bits.len().saturating_sub(1)).step_by(2) {
        let r1 = input_bits[i];
        let r2 = input_bits[i+1];
        
        let mut new_metrics = vec![usize::MAX; num_states];
        let mut new_paths = vec![Vec::new(); num_states];
        
        for s in 0..num_states {
            if metrics[s] == usize::MAX {
                continue;
            }
            
            for bit in 0..2 {
                let (next_s, p1, p2) = transitions[s][bit];
                let dist = ((r1 ^ p1) + (r2 ^ p2)) as usize;
                let new_dist = metrics[s] + dist;
                
                if new_dist < new_metrics[next_s] {
                    new_metrics[next_s] = new_dist;
                    let mut path = paths[s].clone();
                    path.push(bit as u8);
                    new_paths[next_s] = path;
                }
            }
        }
        metrics = new_metrics;
        paths = new_paths;
    }
    
    let mut best_state = 0;
    let mut min_m = metrics[0];
    for (s, &m) in metrics.iter().enumerate() {
        if m < min_m {
            min_m = m;
            best_state = s;
        }
    }
    
    let mut decoded_bits = paths[best_state].clone();
    if decoded_bits.len() > 6 {
        decoded_bits.truncate(decoded_bits.len() - 6);
    } else {
        decoded_bits.clear();
    }
    
    let mut res = Vec::with_capacity(decoded_bits.len() / 8);
    for chunk in decoded_bits.chunks(8) {
        if chunk.len() < 8 { break; }
        let mut byte_val = 0u8;
        for (idx, &b) in chunk.iter().enumerate() {
            byte_val |= b << (7 - idx);
        }
        res.push(byte_val);
    }
    
    Ok((res, min_m))
}
