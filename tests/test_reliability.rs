use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use rand::RngCore;

fn verify_stack_decoding(encodings: Vec<ContentEncoding>, name: &str) {
    let mut generator = PDUGenerator::new(
        Some("SRC".to_string()),
        Some("DST".to_string()),
        None,
        Some(encodings.clone()),
        None,
        1,
    );

    let mut rng = rand::thread_rng();
    let mut data = vec![0u8; 1024];
    rng.fill_bytes(&mut data);

    let pdus = generator.generate(&data, None).expect("Generator failed");
    assert!(!pdus.is_empty(), "{} generated no PDUs", name);

    // 1. Verify Knowledgeable Deframer succeeds
    let mut knowledgeable_deframer = Deframer::new();
    knowledgeable_deframer.register_announcement(Some("SRC".to_string()), 1, encodings);

    let mut found_knowledgeable = false;
    let mut recovered_payload = Vec::new();

    for pdu in &pdus {
        knowledgeable_deframer.receive_bytes(pdu);
    }

    while let Some(ev) = knowledgeable_deframer.next_event() {
        match ev {
            Event::PDU(_) => found_knowledgeable = true,
            Event::Message(m) => recovered_payload = m.payload.to_vec(),
        }
    }

    assert!(
        found_knowledgeable,
        "{}: Knowledgeable Deframer failed to find PDU/Message",
        name
    );
    if !recovered_payload.is_empty() {
        assert_eq!(
            recovered_payload, data,
            "{}: Recovered payload mismatch",
            name
        );
    }
}

#[test]
fn test_scr_rs_conv_opaque_reliability() {
    let encodings = vec![
        ContentEncoding::H,
        ContentEncoding::Crc32,
        ContentEncoding::Scrambler(0x1a9, Some(0xff)),
        ContentEncoding::ReedSolomon(120, 92),
        ContentEncoding::Conv(7, "1/2".to_string()),
    ];
    verify_stack_decoding(encodings, "Opaque (Scr+RS+Conv)");
}

#[test]
fn test_complex_stack_reliability() {
    // "rq(dlen, 72, 10%),crc32,h,rs(120,100),conv(7,1/2)"
    let encodings = vec![
        ContentEncoding::RaptorQDynamicPercent(72, 10),
        ContentEncoding::Crc32,
        ContentEncoding::H,
        ContentEncoding::ReedSolomon(120, 100),
        ContentEncoding::Conv(7, "1/2".to_string()),
    ];
    verify_stack_decoding(encodings, "Complex Stack");
}
