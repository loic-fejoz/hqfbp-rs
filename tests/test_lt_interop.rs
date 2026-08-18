use anyhow::Result;
use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::kiss::KissDecoder;
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
fn test_lt_rust_roundtrip() -> Result<()> {
    // Tests LT pipeline purely within Rust to ensure non-vacuous coverage
    let data = b"Hello LT Code Interop World from Rust!";
    let lt_len = data.len() + 45;
    let mtu = (lt_len + 60) as u16;
    let repair_count = 5;

    let encs = vec![
        ContentEncoding::H,
        ContentEncoding::LT(lt_len, mtu, repair_count),
    ];
    let mut generator = PDUGenerator::new(
        Some("RUST".to_string()),
        None,
        None,
        Some(encs),
        Some(vec![ContentEncoding::Identity]),
        1,
    );
    let pdus = generator.generate(data, None)?;

    let mut deframer = Deframer::new();
    for p in pdus {
        deframer.receive_bytes(&p);
    }

    let mut recovered = None;
    while let Some(event) = deframer.next_event() {
        if let Event::Message(msg) = event {
            recovered = Some(msg.payload.to_vec());
        }
    }

    assert_eq!(
        recovered.as_deref(),
        Some(data.as_slice()),
        "Failed to decode LT message in Rust roundtrip"
    );
    Ok(())
}

#[test]
fn test_lt_py_sample_interop() -> Result<()> {
    let sample_path = get_samples_dir().join("fec_lt.kiss");
    if !sample_path.exists() {
        eprintln!("Note: Python sample not found at {sample_path:?}, skipping external file check");
        return Ok(());
    }

    let file = File::open(sample_path)?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;

    let mut deframer = Deframer::new();
    let mut kiss_decoder = KissDecoder::new();
    let mut decoded_messages = 0;

    for &byte in &buffer {
        if let Some(frame) = kiss_decoder.process_byte(byte)
            && frame.len() > 1
            && frame[0] == 0x00
        {
            let payload = &frame[1..];
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
    }

    assert!(
        decoded_messages > 0,
        "Failed to decode any LT messages from sample"
    );
    Ok(())
}
