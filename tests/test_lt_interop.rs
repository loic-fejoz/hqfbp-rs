use anyhow::Result;

use hqfbp_rs::deframer::{Deframer, Event};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

fn get_samples_dir() -> PathBuf {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.pop();
    d.push("py-hqfbp");
    d.push("samples");
    d
}

#[test]
fn test_lt_interop() -> Result<()> {
    let sample_path = get_samples_dir().join("fec_lt.kiss");
    if !sample_path.exists() {
        eprintln!("Skipping test_lt_interop: sample not found at {sample_path:?}");
        return Ok(());
    }

    let file = File::open(sample_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;

    let mut deframer = Deframer::new();
    let mut decoded_messages = 0;

    let mut current_frame = Vec::new();
    let mut in_frame = false;
    let mut escape_next = false;

    const FEND: u8 = 0xC0;
    const FESC: u8 = 0xDB;
    const TFEND: u8 = 0xDC;
    const TFESC: u8 = 0xDD;

    for &byte in &buffer {
        if byte == FEND {
            if in_frame && !current_frame.is_empty() {
                // End of frame. Process it.
                // First byte is port/command (usually 0x00 for data). strip it.
                if current_frame.len() > 1 {
                    let payload = &current_frame[1..];
                    deframer.receive_bytes(payload);

                    while let Some(event) = deframer.next_event() {
                        if let Event::Message(msg) = event {
                            println!(
                                "Decoded message from {:?}, size: {}",
                                msg.header.src_callsign,
                                msg.payload.len()
                            );
                            decoded_messages += 1;
                        }
                    }
                }
                current_frame.clear();
            }
            in_frame = true;
            continue;
        }

        if !in_frame {
            continue;
        }

        if escape_next {
            match byte {
                TFEND => current_frame.push(FEND),
                TFESC => current_frame.push(FESC),
                _ => current_frame.push(byte), // content was invalid escape, just push literal?
            }
            escape_next = false;
        } else if byte == FESC {
            escape_next = true;
        } else {
            current_frame.push(byte);
        }
    }

    assert!(
        decoded_messages > 0,
        "Failed to decode any LT messages from sample"
    );
    Ok(())
}
