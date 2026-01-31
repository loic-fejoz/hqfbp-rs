use bytes::Bytes;
use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;

#[test]
fn test_ax25_encoding_decoding() {
    let mut pdu_gen = PDUGenerator::new(
        Some("N0CALL".to_string()),
        Some("QST".to_string()),
        None,
        Some(vec![
            ContentEncoding::Chunk(256),
            ContentEncoding::Ax25,
            ContentEncoding::Crc16,
            ContentEncoding::PostAsm(vec![0x7E]),
            ContentEncoding::Asm(vec![0x7E]),
        ]),
        Some(vec![ContentEncoding::H]),
        1,
    );

    let data = b"Hello AX.25!";
    let pdus = pdu_gen
        .generate(data, None)
        .expect("Failed to generate PDU");

    // One announcement, one data PDU
    assert_eq!(pdus.len(), 2);

    let mut deframer = Deframer::new();
    // 1. Receive announcement
    deframer.receive_bytes(&pdus[0]);
    // 2. Receive data
    deframer.receive_bytes(&pdus[1]);

    let mut last_msg = None;
    while let Some(event) = deframer.next_event() {
        if let Event::Message(msg) = event {
            last_msg = Some(msg);
        }
    }
    let msg = last_msg.expect("Expected Message event");
    assert_eq!(msg.payload, Bytes::from_static(data));
    assert_eq!(msg.header.src_callsign.as_deref(), Some("N0CALL"));
    assert_eq!(msg.header.dst_callsign.as_deref(), Some("QST"));
}

#[test]
fn test_ax25_complex_stack() {
    let mut pdu_gen = PDUGenerator::new(
        Some("W1AW-7".to_string()),
        Some("BEACON".to_string()),
        None,
        Some(vec![
            ContentEncoding::Chunk(256),
            ContentEncoding::Ax25,
            ContentEncoding::Crc16,
            ContentEncoding::PostAsm(vec![0x7E]),
            ContentEncoding::Asm(vec![0x7E]),
        ]),
        Some(vec![ContentEncoding::H]),
        100,
    );

    let data = b"This is a longer message that should be compressed and wrapped in AX.25.";
    let pdus = pdu_gen.generate(data, None).expect("Failed to generate");

    // One announcement, one data PDU
    assert_eq!(pdus.len(), 2);

    let mut deframer = Deframer::new();
    // 1. Receive announcement
    deframer.receive_bytes(&pdus[0]);
    // 2. Receive data
    deframer.receive_bytes(&pdus[1]);

    let mut last_msg = None;
    while let Some(event) = deframer.next_event() {
        if let Event::Message(msg) = event {
            last_msg = Some(msg);
        }
    }
    let msg = last_msg.expect("Expected Message event");
    assert_eq!(msg.payload, Bytes::from_static(data));
    assert_eq!(msg.header.src_callsign.as_deref(), Some("W1AW-7"));
    assert_eq!(msg.header.dst_callsign.as_deref(), Some("BEACON"));
}
