#![no_main]

use bytes::Bytes;
use hqfbp_rs::codec::*;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let mode = data[0] % 10;
    let payload = &data[1..];

    match mode {
        0 => {
            let _ = gzip_decompress(payload);
        }
        1 => {
            let _ = brotli_decompress(payload);
        }
        2 => {
            let _ = lzma_decompress(payload);
        }
        3 => {
            let _ = crc16_ccitt(payload);
            let _ = crc32_std(payload);
        }
        4 => {
            if payload.len() >= 8 {
                let mask = u64::from_le_bytes(payload[..8].try_into().unwrap());
                let _ = scr_xor(&payload[8..], mask);
            }
        }
        5 => {
            // RS decode with valid (n, k)
            let _ = rs_decode(payload, 255, 223);
        }
        6 => {
            let _ = conv_decode(payload, 7, "1/2");
        }
        7 => {
            let b = vec![Bytes::copy_from_slice(payload)];
            let _ = rq_decode(b, payload.len().max(1), 64);
        }
        8 => {
            let b = vec![Bytes::copy_from_slice(payload)];
            let _ = lt_decode(b, payload.len().max(1), 64);
        }
        9 => {
            let _ = deflate_decompress(payload);
        }
        _ => {}
    }
});
