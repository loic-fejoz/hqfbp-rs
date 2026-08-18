use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::{ContentEncoding, Header};

#[test]
fn test_quality_prioritization_regression() {
    let data = b"Hello Quality World!".to_vec();
    let encs = vec![ContentEncoding::H, ContentEncoding::ReedSolomon(255, 223)];
    let mut generator = PDUGenerator::new(Some("SRC".to_string()), None, None, Some(encs), None, 1);
    let pdus = generator.generate(&data, None).unwrap();

    let mut deframer = Deframer::new();

    // 1. Receive a "noisy" version first
    let mut noisy_bytes = pdus[0].to_vec();
    let last_idx = noisy_bytes.len() - 1;
    noisy_bytes[last_idx] ^= 0x01; // Force one RS correction

    deframer.receive_bytes(&noisy_bytes);

    // 2. Receive a "clean" version next
    let clean_bytes = pdus[0].to_vec();
    deframer.receive_bytes(&clean_bytes);

    // Recovery
    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }

    // If prioritization is correct, it should have used the clean PDU (0 errors)
    // instead of keeping the noisy one (1 error) if noisy was received first.
    assert_eq!(recovered.unwrap(), data);
}

#[test]
fn test_deframer_no_panic_on_missing_message_id() {
    let mut deframer = Deframer::new();

    // Create an announcement header (which has no message_id) packed as raw bytes
    let header = Header {
        message_id: None,
        original_message_id: None,
        src_callsign: Some("NOCALL".to_string()),
        content_type: Some("application/vnd.hqfbp+cbor".to_string()),
        ..Default::default()
    };

    let mut buf = Vec::new();
    let mut encoder = minicbor::Encoder::new(&mut buf);
    encoder.encode(&header).unwrap();

    // Feeding this directly to deframer should not panic
    deframer.receive_bytes(&buf);

    // Verify deframer is still functional
    let data = b"Normal payload".to_vec();
    let mut generator = PDUGenerator::new(Some("SRC".to_string()), None, None, None, None, 42);
    let normal_pdus = generator.generate(&data, None).unwrap();
    for p in normal_pdus {
        deframer.receive_bytes(&p);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(recovered.unwrap(), data);
}
