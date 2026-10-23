#![no_main]

use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok((header, payload)) = hqfbp_rs::unpack(Bytes::copy_from_slice(data)) {
        let _ = header.media_type();
        let _ = header.into_human_readable();
        let _ = payload.len();
    }
});
