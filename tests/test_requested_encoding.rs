use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use rand::RngCore;

#[test]
fn test_requested_complex_encoding_stack() {
    // 1. Prepare random data of 1024 bytes
    let mut data = vec![0u8; 1024];
    rand::thread_rng().fill_bytes(&mut data);

    // 2. requested encoding list: crc16,h,lt(dlen,118,20),crc16,repeat(2),conv(7,1/2)
    // We construct it manually to avoid comma-splitting issues with parameterized encodings
    let encodings = vec![
        ContentEncoding::Crc16,
        ContentEncoding::H,
        ContentEncoding::LTDynamic(118, 20),
        ContentEncoding::Crc16,
        ContentEncoding::Repeat(2),
        ContentEncoding::Conv(7, "1/2".to_string()),
    ];

    // 3. Setup Generator with the requested encodings and an announcement
    let mut generator = PDUGenerator::new(
        Some("TEST-SRC".to_string()),
        None,
        None,
        Some(encodings),
        Some(vec![ContentEncoding::H]), // Announcement with simple identity transport
        1,
    );

    // 4. Generate PDUs
    let pdus = generator.generate(&data, None).expect("Generate failed");

    // We expect:
    // - One announcement PDU (pdus[0])
    // - Multiple data PDUs (split by LT, each repeated and then conv-encoded)
    assert!(
        pdus.len() > 10,
        "Expected many PDUs due to LT and repeat(2). Got {}",
        pdus.len()
    );

    // 5. Reassemble using Deframer
    let mut deframer = Deframer::new();

    // Feed the announcement first so the deframer knows about the complex stack
    deframer.receive_bytes(&pdus[0]);

    // Feed the data PDUs
    for pdu in &pdus[1..] {
        deframer.receive_bytes(pdu);
    }

    // 6. Verify reassembly
    let mut recovered_data = None;
    let mut message_found = false;

    while let Some(event) = deframer.next_event() {
        if let Event::Message(me) = event {
            recovered_data = Some(me.payload.to_vec());
            message_found = true;
        }
    }

    assert!(message_found, "Should have recovered the message");
    assert_eq!(
        recovered_data.unwrap(),
        data,
        "Recovered data should match original 1024 bytes"
    );
}
