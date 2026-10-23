#![no_main]

use hqfbp_rs::deframer::{Deframer, Event};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let mut deframer = Deframer::new();

    // Optionally register a peer encoding to exercise peer fallback paths
    if data.len() > 4 && data[0] % 2 == 0 {
        deframer.register_peer_encodings(
            "PEER",
            vec![
                hqfbp_rs::ContentEncoding::Gzip,
                hqfbp_rs::ContentEncoding::H,
                hqfbp_rs::ContentEncoding::Crc16,
            ],
        );
    }

    // Feed in chunks to simulate multiple sequential/interleaved PDUs
    let chunk_size = (data[0] as usize % 64).max(1);
    for chunk in data[1..].chunks(chunk_size) {
        deframer.receive_bytes(chunk);
        while let Some(event) = deframer.next_event() {
            match event {
                Event::PDU(p) => {
                    let _ = p.payload.len();
                }
                Event::Message(m) => {
                    let _ = m.payload.len();
                }
            }
        }
    }

    // Exercise session expiration cleanup
    let _ = deframer.cleanup_expired(std::time::Duration::from_millis(data.len() as u64));
});
