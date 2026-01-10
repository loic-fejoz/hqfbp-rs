use hqfbp_rs::codec::{rs_encode, rs_decode};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::ContentEncoding;

#[test]
fn test_regression_rs_shortened_block() {
    let n = 120;
    let k = 100;
    let data = vec![0xCCu8; 50]; // 50 bytes < k=100
    
    let encoded = rs_encode(&data, n, k).expect("RS encode failed");
    // Expected length: data.len() + (n - k) = 50 + 20 = 70
    assert_eq!(encoded.len(), 70);
    
    let (decoded, _) = rs_decode(&encoded, n, k).expect("RS decode failed");
    assert_eq!(decoded, data);
}

#[test]
fn test_regression_deframer_announcement_priority() {
    // Test that deframer correctly uses announcement to strip post-boundary encodings (like CRC32)
    let data = b"Sensitive message";
    let mut generator = PDUGenerator::new(
        Some("SRC".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::H, ContentEncoding::Crc32]),
        Some(vec![ContentEncoding::H, ContentEncoding::Crc16]), // Announcement has H,CRC16
        1,
    );
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    // We expect 2 PDUs:
    // 1. Announcement PDU (H,CRC16)
    // 2. Data PDU (H,CRC32)
    assert_eq!(pdus.len(), 2);
    
    let mut deframer = Deframer::new();
    // Process announcement first
    deframer.receive_bytes(&pdus[0]);
    // Process data
    deframer.receive_bytes(&pdus[1]);
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert_eq!(me.payload, data);
            found = true;
        }
    }
    assert!(found, "Message not deframed via announcement");
}

#[test]
fn test_regression_rs_multi_layer_deframing() {
    // Test a more complex stack: Gzip, H, RS(120, 100)
    let data = b"This is a relatively long message that will be gzipped and then RS encoded.";
    let mut generator = PDUGenerator::new(
        Some("SRC".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Gzip, ContentEncoding::H, ContentEncoding::ReedSolomon(120, 100)]),
        Some(vec![ContentEncoding::H, ContentEncoding::Crc16]),
        1,
    );
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    let mut deframer = Deframer::new();
    for pdu in pdus {
        deframer.receive_bytes(&pdu);
    }
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        match ev {
            Event::PDU(pe) => {
                println!("[TEST] Received PDU: msg_id={:?}, payload_len={}", pe.header.message_id, pe.payload.len());
            }
            Event::Message(me) => {
                println!("[TEST] Received Message: payload_len={}", me.payload.len());
                assert_eq!(me.payload, data);
                found = true;
            }
        }
    }
    assert!(found, "Complex multi-layer message not deframed");
}
