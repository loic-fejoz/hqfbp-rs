use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;

#[test]
fn test_chunk_rs_after_h() {
    // 1. Setup Generator with fragmentation-inducing encodings
    // Python: ["gzip", "h", "chunk(223)", "rs(255,223)", "repeat(2)"]
    let mut generator = PDUGenerator::new(
        Some("TEST".to_string()),
        None,
        None,
        Some(vec![
            ContentEncoding::Gzip,
            ContentEncoding::H,
            ContentEncoding::Chunk(223),
            ContentEncoding::ReedSolomon(255, 223),
            ContentEncoding::Repeat(2),
        ]),
        Some(vec![ContentEncoding::Crc16]),
        1,
    );

    // 500 bytes of data
    let data: Vec<u8> = (0..500).map(|i| (i % 256) as u8).collect();

    // 2. Generate PDUs
    let pdus = generator.generate(&data, None).expect("Generate failed");

    // 3. Process PDUs with Deframer
    let mut deframer = Deframer::new();
    let mut recovered_messages = Vec::new();
    for pdu in pdus {
        deframer.receive_bytes(&pdu);
        while let Some(ev) = deframer.next_event() {
            if let Event::Message(me) = ev {
                recovered_messages.push(me);
            }
        }
    }

    // 4. Assertions
    assert_eq!(recovered_messages.len(), 1);
    let recovered = &recovered_messages[0];
    assert_eq!(recovered.payload.len(), 500);
    assert_eq!(recovered.payload.as_ref(), data.as_slice());
    assert_eq!(recovered.header.src_callsign.as_deref(), Some("TEST"));

    // Header should be clean (no chunk, rs, repeat, or h)
    if let Some(ce) = &recovered.header.content_encoding {
        for e in &ce.0 {
            assert!(!matches!(e, ContentEncoding::H));
            assert!(!matches!(e, ContentEncoding::Chunk(_)));
            assert!(!matches!(e, ContentEncoding::ReedSolomon(_, _)));
            assert!(!matches!(e, ContentEncoding::Repeat(_)));
        }
    }
}
