use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;

#[test]
fn test_regression_repeat_rs_pdu_level() {
    // This test reproduces the bug where Repeat(2) would destroy a single RS PDU
    // by slicing it, making RS decoding fail.

    let mut generator = PDUGenerator::new(
        Some("TEST".to_string()),
        None,
        None,
        Some(vec![
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 223),
            ContentEncoding::Repeat(2),
        ]),
        None,
        1,
    );

    let data = b"Hello World! This is a test of the RS + Repeat system.".to_vec();
    let pdus = generator.generate(&data, None).expect("Generate failed");

    // With Repeat(2), we expect 2 PDUs for this small data
    assert_eq!(pdus.len(), 2);

    let mut deframer = Deframer::new();

    // Process only ONE of the duplicated PDUs (simulating loss of the other)
    // This should still work because Phase 1 or Phase 2 should handle the single copy.
    deframer.receive_bytes(&pdus[0]);

    let mut messages = Vec::new();
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            messages.push(me);
        }
    }

    assert_eq!(
        messages.len(),
        1,
        "Should have recovered the message from a single repeated PDU"
    );
    assert_eq!(messages[0].payload.as_ref(), &data[..]);
}

#[test]
fn test_regression_systematic_rs_header_corrupt() {
    // This test ensures that if the header is corrupt but RS can fix it,
    // the deframer still recovers the PDU.

    let mut generator = PDUGenerator::new(
        Some("TEST".to_string()),
        None,
        None,
        Some(vec![
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 223),
        ]),
        None,
        1,
    );

    let data = b"Some data that needs protection".to_vec();
    let pdus = generator.generate(&data, None).expect("Generate failed");
    let mut noisy_pdu = pdus[0].to_vec();

    // Corrupt the header (first few bytes)
    // Systematic RS puts the header at the beginning.
    noisy_pdu[0] ^= 0xFF;
    noisy_pdu[1] ^= 0xFF;

    let mut deframer = Deframer::new();

    // We NEED an announcement to recover a corrupt header via RS
    // because Phase 1 (peek) will fail, and Phase 2 needs ann_encs.
    let mut ann_gen = PDUGenerator::new(
        Some("TEST".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::H]),
        None,
        2,
    );
    // Announcements usually send the encodings used for message_id 1
    // (Actual simulation sends announcements with the full encoding list)
    // Here we manually handle announcement logic simplified.

    // To make it simpler, we use the fact that Deframer stores announcements
    // when it receives an announcement PDU.

    let mut ann_header = hqfbp_rs::Header {
        message_id: Some(1),
        content_encoding: Some(hqfbp_rs::EncodingList(vec![
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 223),
        ])),
        ..Default::default()
    };
    ann_header.set_media_type(Some(hqfbp_rs::MediaType::Type(
        "application/vnd.hqfbp+cbor".to_string(),
    )));

    let ann_body = minicbor::to_vec(&ann_header).unwrap();
    let ann_pdus = ann_gen
        .generate(
            &ann_body,
            Some(hqfbp_rs::MediaType::Type(
                "application/vnd.hqfbp+cbor".to_string(),
            )),
        )
        .unwrap();

    deframer.receive_bytes(&ann_pdus[0]);

    // Now receive the corrupt PDU
    deframer.receive_bytes(&noisy_pdu);

    let mut messages = Vec::new();
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            messages.push(me);
        }
    }

    assert_eq!(
        messages.len(),
        1,
        "Should have recovered the message from a corrupt PDU using RS via announcement"
    );
    assert_eq!(messages[0].payload.as_ref(), &data[..]);
}
