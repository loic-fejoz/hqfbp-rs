use anyhow::{Result, anyhow, bail};
use crc::{Crc, CRC_16_XMODEM, CRC_32_ISO_HDLC};
use reed_solomon::Encoder as RSEncoder;
use reed_solomon::Decoder as RSDecoder;
use raptorq::{Encoder as RQEncoder};
use bytes::Bytes;
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
    const CRC_16_PYTHON: crc::Algorithm<u16> = crc::Algorithm { 
        width: 16, 
        poly: 0x1021, 
        init: 0xffff, 
        refin: false, 
        refout: false, 
        xorout: 0x0000, 
        check: 0x29b1,
        residue: 0x0000
    };
    let crc = Crc::<u16>::new(&CRC_16_PYTHON);
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
    lzma_rs::xz_compress(&mut Cursor::new(data), &mut res)
        .map_err(|e| anyhow!("XZ compress failed: {}", e))?;
    Ok(res)
}

pub fn lzma_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut res = Vec::new();
    lzma_rs::xz_decompress(&mut Cursor::new(data), &mut res)
        .map_err(|e| anyhow!("XZ decompress failed: {}", e))?;
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
    if n > 255 || k == 0 || k > n {
        bail!("Invalid RS parameters: n={}, k={}", n, k);
    }
    let ecc_len = n - k;
    let encoder = RSEncoder::new(ecc_len);
    let mut encoded = Vec::with_capacity((data.len() + k - 1) / k * n);
    
    for chunk in data.chunks(k) {
        // Systematic RS: Input data preserved at the beginning.
        // Python reedsolo appends zeros for padding: data.ljust(k, b'\0')
        // We MUST do the same for bit-accuracy.
        let mut block_for_lib = chunk.to_vec();
        block_for_lib.resize(k, 0); // Appends zeros
        
        let lib_pad = 255 - n;
        // Match the "pre-pad to 255" strategy but keep chunk at the start.
        let mut full_codeword_for_lib = vec![0u8; lib_pad];
        full_codeword_for_lib.extend_from_slice(&block_for_lib);
        
        // Systematic RS means it returns [data_part, parity]
        let encoded_full_codeword = encoder.encode(&full_codeword_for_lib);
        
        let parity = &encoded_full_codeword[255 - ecc_len..];
        
        // Python transmits the full 'n' bytes (including the k-len padded block)
        encoded.extend_from_slice(&block_for_lib);
        encoded.extend_from_slice(parity);
    }
    Ok(encoded)
}

pub fn rs_decode(data: &[u8], n: usize, k: usize) -> Result<(Vec<u8>, usize)> {
    if n > 255 || k == 0 || k >= n {
        bail!("Invalid RS parameters: n={}, k={}", n, k);
    }
    let ecc_len = n - k;
    let decoder = RSDecoder::new(ecc_len);
    let mut decoded = Vec::with_capacity(data.len().checked_div(n).unwrap_or(0) * k);
    let mut total_corrected = 0;

    let mut i = 0;
    while i < data.len() {
        let remaining = data.len() - i;
        let block_len = if remaining >= n { n } else { remaining };
        let chunk = &data[i..i + block_len];
        i += block_len;

        if block_len < n {
             // In Python-accurate mode, blocks are ALWAYS n bytes because we pad during encode.
             // If we got a shorter block, it's either an error or we need to pad it.
             // But if we want bit-accuracy, we should probably expect n.
             // However, for robustness, let's treat it as a truncated block.
        }
        
        if block_len <= ecc_len {
            bail!("RS block too short to contain parity");
        }
        
        let lib_pad = 255 - n;
        // In encode, we did: [lib_pad, chunk_padded_to_k]
        // chunk here is [data_part, parity]
        let mut full_codeword = vec![0u8; lib_pad];
        full_codeword.extend_from_slice(chunk);
        
        // If chunk was shorter than n, we need to pad it to n (which means padding the data part)
        // Wait, if chunk is [data, parity] and data is shorter than k.
        // We should have [data, pad_to_k, parity].
        // If chunk is just [data, parity], it's ambiguous.
        // But since we aligned encode to always produce n bytes, we expect block_len == n.
        if full_codeword.len() < 255 {
            let needed = 255 - full_codeword.len();
            // We must insert the padding BEFORE the parity.
            let parity_part = &full_codeword[full_codeword.len() - ecc_len..];
            let mut new_fw = full_codeword[..full_codeword.len() - ecc_len].to_vec();
            new_fw.resize(new_fw.len() + needed, 0);
            new_fw.extend_from_slice(parity_part);
            full_codeword = new_fw;
        }

        match decoder.correct_err_count(&full_codeword, None) {
            Ok((corrected, err_count)) => {
                // Decoded data is from lib_pad to lib_pad + k
                let dpart = &corrected[lib_pad..lib_pad + k];
                decoded.extend_from_slice(dpart);
                total_corrected += err_count;
            }
            Err(e) => {
                bail!("RS decode failed: {:?}", e);
            }
        }
    }
    Ok((decoded, total_corrected))
}

pub fn rq_encode(data: &[u8], original_count: usize, mtu: u16, repair_count: u32) -> Result<Vec<Bytes>> {
    let mut padded_data = data.to_vec();
    padded_data.resize(original_count, 0);
    
    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let encoder = RQEncoder::new(&padded_data, oti);
    let packets = encoder.get_encoded_packets(repair_count);
    
    Ok(packets.into_iter().map(|p| Bytes::from(p.serialize())).collect())
}

pub fn rq_decode(packets: Vec<Bytes>, original_count: usize, mtu: u16) -> Result<Vec<u8>> {
    // Standard HQFBP alignment is 1-4. Python uses 1 by default often.
    // Let's try 1 for alignment.
    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let mut decoder = raptorq::Decoder::new(oti);
    
    for packet_bytes in packets {
        if packet_bytes.len() < 4 {
            continue;
        }
        
        let packet = raptorq::EncodingPacket::deserialize(packet_bytes.as_ref());
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
    let num_states = 1 << (k - 1);
    
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rs_basic() {
        let n = 255;
        let k = 223;
        let mut data = vec![1, 2, 3, 4, 5];
        data.resize(k, 223);
        let encoded = rs_encode(&data, n, k).unwrap();
        assert_eq!(encoded.len(), 255);
        
        let (decoded, corrected) = rs_decode(&encoded, n, k).unwrap();
        assert_eq!(decoded, data);
        assert_eq!(corrected, 0);
    }

    #[test]
    fn test_rs_noisy() {
        let n = 255;
        let k = 223;
        let mut data = vec![0x42u8; 223];
        let mut encoded = rs_encode(&data, n, k).unwrap();
        
        // Flip one byte (symbol)
        encoded[10] ^= 0xFF;
        
        let (decoded, corrected) = rs_decode(&encoded, n, k).unwrap();
        assert_eq!(decoded, data, "Should have corrected one byte error");
        assert_eq!(corrected, 1);
    }
}
