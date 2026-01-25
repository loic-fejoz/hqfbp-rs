use anyhow::{Result, anyhow, bail};
use bytes::Bytes;
use crc::{CRC_32_ISO_HDLC, Crc};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use raptorq::Encoder as RQEncoder;
use reed_solomon::Decoder as RSDecoder;
use reed_solomon::Encoder as RSEncoder;
use std::io::{Cursor, Read, Write};
pub mod golay;
pub mod lt;
use golay::{golay_decode as golay_dec, golay_encode as golay_enc};
use lt::{LTDecoder, LTEncoder};

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
        residue: 0x0000,
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
        .map_err(|e| anyhow!("XZ compress failed: {e}"))?;
    Ok(res)
}

pub fn lzma_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut res = Vec::new();
    lzma_rs::xz_decompress(&mut Cursor::new(data), &mut res)
        .map_err(|e| anyhow!("XZ decompress failed: {e}"))?;
    Ok(res)
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

pub fn rs_encode(data: &[u8], n: usize, k: usize) -> Result<Vec<u8>> {
    if n > 255 || k == 0 || k > n {
        bail!("Invalid RS parameters: n={n}, k={k}");
    }
    let ecc_len = n - k;
    let encoder = RSEncoder::new(ecc_len);
    let mut encoded = Vec::with_capacity(data.len().div_ceil(k) * n);

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
        bail!("Invalid RS parameters: n={n}, k={k}");
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
                bail!("RS decode failed: {e:?}");
            }
        }
    }
    Ok((decoded, total_corrected))
}

pub fn rq_encode(
    data: &[u8],
    original_count: usize,
    mtu: u16,
    repair_count: u32,
) -> Result<Vec<Bytes>> {
    let mut padded_data = data.to_vec();
    padded_data.resize(original_count, 0);

    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let encoder = RQEncoder::new(&padded_data, oti);
    let packets = encoder.get_encoded_packets(repair_count);

    Ok(packets
        .into_iter()
        .map(|p| Bytes::from(p.serialize()))
        .collect())
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

        // Truncate to expected size (symbol size + 4 bytes ESI) to remove padding
        // introduced by lower layers (e.g. RS).
        let expected_len = (mtu as usize) + 4;

        if packet_bytes.len() < expected_len {
            continue;
        }

        let p_ref = if packet_bytes.len() > expected_len {
            &packet_bytes[..expected_len]
        } else {
            packet_bytes.as_ref()
        };

        let packet = raptorq::EncodingPacket::deserialize(p_ref);
        if let Some(res) = decoder.decode(packet) {
            return Ok(res);
        }
    }
    bail!("RaptorQ decoding failed: insufficient symbols")
}

pub fn lt_encode(
    data: &[u8],
    original_count: usize,
    mtu: u16,
    repair_count: u32,
) -> Result<Vec<Bytes>> {
    if mtu == 0 || original_count == 0 {
        return Ok(Vec::new());
    }
    let mut padded_data = data.to_vec();
    if padded_data.len() < original_count {
        padded_data.resize(original_count, 0);
    }

    let encoder = LTEncoder::new(padded_data, mtu as usize);
    let packets = encoder.encode(repair_count as usize);

    Ok(packets.into_iter().map(Bytes::from).collect())
}

pub fn lt_decode(packets: Vec<Bytes>, original_count: usize, mtu: u16) -> Result<Vec<u8>> {
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
        bail!("LT decoding failed: insufficient symbols")
    }
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

use once_cell::sync::Lazy;

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

pub fn conv_decode(data: &[u8], k: usize, rate: &str) -> Result<(Vec<u8>, usize)> {
    if k != 7 || rate != "1/2" {
        bail!("Only conv(7, 1/2) is currently supported");
    }

    let num_steps = data.len() * 4;
    if num_steps == 0 {
        return Ok((Vec::new(), 0));
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

    // predecessor_states[step][state] = (prev_state << 1) | bit
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

    // Backtrack
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

pub fn golay_encode(data: &[u8]) -> Result<Vec<u8>> {
    Ok(golay_enc(data))
}

pub fn golay_decode(data: &[u8]) -> Result<(Vec<u8>, usize)> {
    golay_dec(data)
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
        let data = vec![0x42u8; 223];
        let mut encoded = rs_encode(&data, n, k).unwrap();

        // Flip one byte (symbol)
        encoded[10] ^= 0xFF;

        let (decoded, corrected) = rs_decode(&encoded, n, k).unwrap();
        assert_eq!(decoded, data, "Should have corrected one byte error");
        assert_eq!(corrected, 1);
    }
}
