#![no_main]

use hqfbp_rs::kiss::KissDecoder;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut decoder = KissDecoder::new();
    for &byte in data {
        if let Some(frame) = decoder.process_byte(byte) {
            let _ = frame.len();
        }
    }
});
