#![no_main]

use hqfbp_rs::deframer::Deframer;
use hqfbp_rs::kiss::{KissCommand, KissDecoder, parse_kiss_frame};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut decoder = KissDecoder::new();
    let mut deframer = Deframer::new();

    for &byte in data {
        if let Some(frame) = decoder.process_byte(byte) {
            if let Some(parsed) = parse_kiss_frame(&frame) {
                if parsed.command == KissCommand::DataFrame {
                    deframer.receive_bytes(&parsed.data);
                    while let Some(_event) = deframer.next_event() {}
                }
            }
        }
    }
});
