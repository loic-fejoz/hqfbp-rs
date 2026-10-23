use anyhow::{Result, anyhow, bail};
use bytes::Bytes;
use crc::{CRC_32_ISO_HDLC, Crc};
use flate2::Compression;
use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use raptorq::Encoder as RQEncoder;
use reed_solomon::Decoder as RSDecoder;
use reed_solomon::Encoder as RSEncoder;
use std::io::{Cursor, Read, Write};
pub mod lt;
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

pub const MAX_DECOMPRESSED_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB

struct BoundedWriter<W> {
    inner: W,
    written: u64,
    max: u64,
}

impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written + buf.len() as u64 > self.max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Decompressed payload exceeded maximum allowed size",
            ));
        }
        let n = self.inner.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

pub fn gzip_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data).take(MAX_DECOMPRESSED_SIZE + 1);
    let mut res = Vec::new();
    decoder.read_to_end(&mut res)?;
    if res.len() as u64 > MAX_DECOMPRESSED_SIZE {
        bail!("Gzip decompression exceeded maximum allowed size ({MAX_DECOMPRESSED_SIZE} bytes)");
    }
    Ok(res)
}

pub fn deflate_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

pub fn deflate_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data).take(MAX_DECOMPRESSED_SIZE + 1);
    let mut res = Vec::new();
    if decoder.read_to_end(&mut res).is_err() {
        res.clear();
        let mut raw_decoder = DeflateDecoder::new(data).take(MAX_DECOMPRESSED_SIZE + 1);
        raw_decoder.read_to_end(&mut res)?;
    }
    if res.len() as u64 > MAX_DECOMPRESSED_SIZE {
        bail!(
            "Deflate decompression exceeded maximum allowed size ({MAX_DECOMPRESSED_SIZE} bytes)"
        );
    }
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
    let mut reader = brotli::Decompressor::new(data, 4096).take(MAX_DECOMPRESSED_SIZE + 1);
    reader.read_to_end(&mut res)?;
    if res.len() as u64 > MAX_DECOMPRESSED_SIZE {
        bail!("Brotli decompression exceeded maximum allowed size ({MAX_DECOMPRESSED_SIZE} bytes)");
    }
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
    let mut bounded = BoundedWriter {
        inner: &mut res,
        written: 0,
        max: MAX_DECOMPRESSED_SIZE,
    };
    lzma_rs::xz_decompress(&mut Cursor::new(data), &mut bounded)
        .map_err(|e| anyhow!("XZ decompress failed or exceeded limit: {e}"))?;
    Ok(res)
}

pub fn scr_xor(data: &[u8], poly_mask: u64) -> Vec<u8> {
    if poly_mask == 0 {
        return data.to_vec();
    }

    let width = 64 - poly_mask.leading_zeros();
    let mask = u64::MAX >> (64 - width);
    let mut state = mask;

    #[inline(always)]
    fn next_byte(state: &mut u64, poly_mask: u64, mask: u64) -> u8 {
        let mut ks = 0u8;
        for _ in 0..8 {
            let feedback = ((*state & poly_mask).count_ones() & 1) as u8;
            ks = (ks << 1) | feedback;
            *state = ((*state << 1) | (feedback as u64)) & mask;
            if *state == 0 {
                *state = mask;
            }
        }
        ks
    }

    let mut res = Vec::with_capacity(data.len());
    let (chunks, remainder) = data.as_chunks::<8>();
    for chunk in chunks {
        let in_word = u64::from_le_bytes(*chunk);
        let mut ks_word = 0u64;
        for i in 0..8 {
            let b = next_byte(&mut state, poly_mask, mask);
            ks_word |= (b as u64) << (i * 8);
        }
        res.extend_from_slice(&(in_word ^ ks_word).to_le_bytes());
    }

    for &b in remainder {
        let ks = next_byte(&mut state, poly_mask, mask);
        res.push(b ^ ks);
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
    let lib_pad = 255 - n;
    let mut codeword_buf = [0u8; 255];

    for chunk in data.chunks(k) {
        // Clear the data section of codeword_buf
        codeword_buf[lib_pad..255 - ecc_len].fill(0);
        codeword_buf[lib_pad..lib_pad + chunk.len()].copy_from_slice(chunk);

        let encoded_full_codeword = encoder.encode(&codeword_buf[..255 - ecc_len]);
        let parity = &encoded_full_codeword[255 - ecc_len..];

        encoded.extend_from_slice(&codeword_buf[lib_pad..255 - ecc_len]);
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
    let lib_pad = 255 - n;

    let mut i = 0;
    while i < data.len() {
        let remaining = data.len() - i;
        let block_len = if remaining >= n { n } else { remaining };
        let chunk = &data[i..i + block_len];
        i += block_len;

        if block_len <= ecc_len {
            bail!("RS block too short to contain parity");
        }

        let mut full_codeword = [0u8; 255];
        if block_len == n {
            full_codeword[lib_pad..255].copy_from_slice(chunk);
        } else {
            let data_part_len = block_len - ecc_len;
            full_codeword[lib_pad..lib_pad + data_part_len]
                .copy_from_slice(&chunk[..data_part_len]);
            full_codeword[255 - ecc_len..255].copy_from_slice(&chunk[data_part_len..]);
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
    if original_count == 0 || mtu == 0 || original_count as u64 > MAX_DECOMPRESSED_SIZE {
        bail!("Invalid RaptorQ encoding parameters: original_count={original_count}, mtu={mtu}");
    }
    let k = original_count.div_ceil(mtu as usize);
    if k > 65_536 {
        bail!("RaptorQ symbol count k={k} exceeds maximum allowed (65536)");
    }

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
    if original_count == 0 || mtu == 0 || original_count as u64 > MAX_DECOMPRESSED_SIZE {
        bail!("Invalid RaptorQ decoding parameters: original_count={original_count}, mtu={mtu}");
    }
    let k = original_count.div_ceil(mtu as usize);
    if k > 65_536 {
        bail!("RaptorQ symbol count k={k} exceeds maximum allowed (65536)");
    }

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
    if original_count == 0 || mtu == 0 || original_count as u64 > MAX_DECOMPRESSED_SIZE {
        bail!("Invalid LT encoding parameters: original_count={original_count}, mtu={mtu}");
    }
    let k = original_count.div_ceil(mtu as usize);
    if k > 65_536 {
        bail!("LT symbol count k={k} exceeds maximum allowed (65536)");
    }

    let mut padded_data = data.to_vec();
    if padded_data.len() < original_count {
        padded_data.resize(original_count, 0);
    }

    let encoder = LTEncoder::try_new(padded_data, mtu as usize)?;
    let packets = encoder.encode(repair_count as usize);

    Ok(packets.into_iter().map(Bytes::from).collect())
}

pub fn lt_decode(packets: Vec<Bytes>, original_count: usize, mtu: u16) -> Result<Vec<u8>> {
    if original_count == 0 || mtu == 0 || original_count as u64 > MAX_DECOMPRESSED_SIZE {
        bail!("Invalid LT decoding parameters: original_count={original_count}, mtu={mtu}");
    }
    let k = original_count.div_ceil(mtu as usize);
    if k > 65_536 {
        bail!("LT symbol count k={k} exceeds maximum allowed (65536)");
    }

    let mut decoder = LTDecoder::try_new(original_count, mtu as usize)?;

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

pub fn conv_decode(data: &[u8], k: usize, rate: &str) -> Result<(Vec<u8>, usize)> {
    if k != 7 || rate != "1/2" {
        bail!("Only conv(7, 1/2) is currently supported");
    }

    const NUM_STATES: usize = 64;
    const INF: u16 = 30000;

    // Precompute generator outputs for each of the 128 full states (7 bits)
    // full_state = (s << 1) | bit
    const G1: u8 = 0o133;
    const G2: u8 = 0o171;

    let mut out_bits = [(0u8, 0u8); 128];
    for (state, out) in out_bits.iter_mut().enumerate() {
        let mut p1 = 0u8;
        let mut p2 = 0u8;
        for i in 0..7 {
            if (G1 >> i) & 1 != 0 {
                p1 ^= ((state >> i) & 1) as u8;
            }
            if (G2 >> i) & 1 != 0 {
                p2 ^= ((state >> i) & 1) as u8;
            }
        }
        *out = (p1, p2);
    }

    let num_pairs = (data.len() * 8) / 2;
    if num_pairs == 0 {
        return Ok((Vec::new(), 0));
    }

    let mut metrics = [INF; NUM_STATES];
    metrics[0] = 0;

    // Flat table of predecessor states for traceback: num_pairs * 64
    let mut backpointers = vec![[0u8; NUM_STATES]; num_pairs];

    let mut step = 0;
    for &b in data {
        for bit_idx in (0..8).step_by(2).rev() {
            if step >= num_pairs {
                break;
            }
            let r1 = (b >> (bit_idx + 1)) & 1;
            let r2 = (b >> bit_idx) & 1;

            let mut new_metrics = [INF; NUM_STATES];
            let bp = &mut backpointers[step];

            // Butterfly update over 64 states
            for next_s in 0..NUM_STATES {
                let s0 = next_s >> 1;
                let s1 = s0 | 32;

                let (p1_0, p2_0) = out_bits[next_s]; // full state = (s0 << 1) | bit = next_s
                let (p1_1, p2_1) = out_bits[next_s | 64]; // full state = (s1 << 1) | bit = next_s | 64

                let dist0 = ((r1 ^ p1_0) + (r2 ^ p2_0)) as u16;
                let dist1 = ((r1 ^ p1_1) + (r2 ^ p2_1)) as u16;

                let cost0 = metrics[s0].saturating_add(dist0);
                let cost1 = metrics[s1].saturating_add(dist1);

                if cost0 <= cost1 {
                    new_metrics[next_s] = cost0;
                    bp[next_s] = s0 as u8;
                } else {
                    new_metrics[next_s] = cost1;
                    bp[next_s] = s1 as u8;
                }
            }

            metrics = new_metrics;
            step += 1;
        }
    }

    // Find best ending state
    let mut best_state = 0;
    let mut min_m = metrics[0];
    for (s, &m) in metrics.iter().enumerate() {
        if m < min_m {
            min_m = m;
            best_state = s;
        }
    }

    // Traceback
    let mut decoded_bits = Vec::with_capacity(num_pairs);
    let mut curr_state = best_state;
    for t in (0..num_pairs).rev() {
        decoded_bits.push((curr_state & 1) as u8);
        curr_state = backpointers[t][curr_state] as usize;
    }
    decoded_bits.reverse();

    // Strip tail bits (flush bits = 6)
    if decoded_bits.len() > 6 {
        decoded_bits.truncate(decoded_bits.len() - 6);
    } else {
        decoded_bits.clear();
    }

    let mut res = Vec::with_capacity(decoded_bits.len() / 8);
    for chunk in decoded_bits.as_chunks::<8>().0 {
        let mut byte_val = 0u8;
        for (idx, &b) in chunk.iter().enumerate() {
            byte_val |= b << (7 - idx);
        }
        res.push(byte_val);
    }

    Ok((res, min_m as usize))
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
