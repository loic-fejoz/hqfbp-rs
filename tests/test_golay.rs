use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::Deframer;
use hqfbp_rs::generator::PDUGenerator;

#[test]
fn test_golay_stack_roundtrip() {
    let _ = env_logger::builder().is_test(true).try_init();

    let encodings = vec![ContentEncoding::H, ContentEncoding::Golay(24, 12)];

    let mut generator = PDUGenerator::new(
        Some("TEST".to_string()),
        None,
        None,
        Some(encodings.clone()),
        None,
        1,
    );

    let data = b"Hello from Golay stack!";
    let pdus = generator.generate(data, None).unwrap();

    let mut deframer = Deframer::new();
    deframer.register_announcement(Some("TEST".to_string()), 1, encodings.clone());

    for pdu in pdus {
        deframer.receive_bytes(&pdu);
    }

    let mut events = Vec::new();
    while let Some(e) = deframer.next_event() {
        events.push(e);
    }

    // Check if we got a message event
    let msg = events
        .iter()
        .find_map(|e| {
            if let hqfbp_rs::deframer::Event::Message(m) = e {
                Some(m)
            } else {
                None
            }
        })
        .expect("Should have received a message");

    assert!(msg.payload.starts_with(data));
}

#[test]
fn test_complex_golay_stack() {
    // rq(dlen, 256, 64), crc32, h, scr(G3RUH), golay(24,12)
    // Wait, SCR(G3RUH) needs the polynomial. G3RUH is 0x21001.

    let encodings = vec![
        ContentEncoding::RaptorQDynamic(256, 64),
        ContentEncoding::Crc32,
        ContentEncoding::H,
        ContentEncoding::Scrambler(0x21001, None),
        ContentEncoding::Golay(24, 12),
    ];

    let mut generator = PDUGenerator::new(
        Some("TEST".to_string()),
        None,
        None,
        Some(encodings.clone()),
        None,
        1,
    );

    let data = vec![0xAAu8; 4096];
    let pdus = generator.generate(&data, None).unwrap();

    // Total PDUs should be more than 1 due to RaptorQ/H boundary
    assert!(pdus.len() > 1);

    let mut deframer = Deframer::new();
    // Register announcement so it knows the stack
    deframer.register_announcement(Some("TEST".to_string()), 1, encodings.clone());

    for (i, pdu) in pdus.iter().enumerate() {
        let mut noisy_pdu = pdu.to_vec();
        // Add some noise to the first PDU: flip a bit in a Golay block
        if i == 0 && noisy_pdu.len() > 10 {
            noisy_pdu[10] ^= 1;
        }
        deframer.receive_bytes(&noisy_pdu);
    }

    let mut found = false;
    while let Some(event) = deframer.next_event() {
        if let hqfbp_rs::deframer::Event::Message(msg) = event {
            assert_eq!(msg.payload.len(), 4096);
            assert_eq!(msg.payload[..], data[..]);
            found = true;
        }
    }
    assert!(found, "Message was not recovered");
}
