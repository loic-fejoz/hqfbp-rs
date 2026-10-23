#![no_main]

use hqfbp_rs::deframer::{Deframer, Event};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let mut deframer = Deframer::new();

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
});
