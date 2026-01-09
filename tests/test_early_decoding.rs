use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::{PDUGenerator, EncValue};

#[test]
fn test_deframer_early_rq_decoding() {
    let mut deframer = Deframer::new();
    let mut data = Vec::new();
    for _ in 0..10 {
        data.extend_from_slice(b"Early RaptorQ decoding test data");
    }
    let dlen = data.len();
    let mtu = 30;
    let repair = 20;
    
    let mut generator = PDUGenerator::new(
        Some("EARLY-RQ".to_string()),
        None,
        None,
        Some(vec![EncValue::String(format!("rq({}, {}, {})", dlen, mtu, repair)), EncValue::String("h".to_string())]),
        Some(vec![EncValue::String("identity".to_string())]),
        1,
    );
    
    let pdus = generator.generate(&data, None).expect("Generate failed");
    
    // pdus[0] is announcement
    // pdus[1..11] are source packets (K symbols)
    
    // 1. Feed announcement
    deframer.receive_bytes(&pdus[0]);
    
    // 2. Feed only K source packets (11)
    for pdu in &pdus[1..12] {
        deframer.receive_bytes(pdu);
    }
    
    // Check if decoded early
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert_eq!(me.payload, data);
            found = true;
        }
    }
    assert!(found, "Message should be decoded after 10 packets");
}

#[test]
fn test_deframer_early_rq_with_loss() {
    let mut deframer = Deframer::new();
    let mut data = Vec::new();
    for _ in 0..5 {
        data.extend_from_slice(b"RaptorQ early decoding with loss");
    }
    let dlen = data.len();
    let mtu = 20;
    let repair = 10;
    
    let mut generator = PDUGenerator::new(
        Some("RQ-LOSS".to_string()),
        None,
        None,
        Some(vec![EncValue::String(format!("rq({}, {}, {})", dlen, mtu, repair)), EncValue::String("h".to_string())]),
        Some(vec![EncValue::String("identity".to_string())]),
        1,
    );
    
    let pdus = generator.generate(&data, None).expect("Generate failed");
    // K = ceil(160 / 20) = 8
    
    deframer.receive_bytes(&pdus[0]);
    
    // Lose some source packets but keep enough total
    // Receive packets 1, 3, 5, 7, 9, 11, 13, 15 (8 packets total)
    for &i in &[1, 3, 5, 7, 9, 11, 13, 15] {
        deframer.receive_bytes(&pdus[i]);
    }
        
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert_eq!(me.payload, data);
            found = true;
        }
    }
    assert!(found, "Should decode with 8 symbols even if some source packets are missing");
}

#[test]
fn test_deframer_rq_wait_for_enough_symbols() {
    let mut deframer = Deframer::new();
    let mut data = Vec::new();
    for _ in 0..10 {
        data.extend_from_slice(b"Wait for symbols");
    }
    let dlen = data.len();
    let mtu = 50;
    let repair = 5;
    // K = ceil(160 / 50) = 4
    
    let mut generator = PDUGenerator::new(
        Some("RQ-WAIT".to_string()),
        None,
        None,
        Some(vec![EncValue::String(format!("rq({}, {}, {})", dlen, mtu, repair)), EncValue::String("h".to_string())]),
        Some(vec![EncValue::String("identity".to_string())]),
        1,
    );
    
    let pdus = generator.generate(&data, None).expect("Generate failed");
    deframer.receive_bytes(&pdus[0]);
    
    // Feed 3 packets (not enough)
    for pdu in &pdus[1..4] {
        deframer.receive_bytes(pdu);
    }
        
    // No MessageEvent yet
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(_) = ev {
            panic!("Premature message event");
        }
    }
    
    // Feed 4th packet
    deframer.receive_bytes(&pdus[4]);
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert_eq!(me.payload, data);
            found = true;
        }
    }
    assert!(found);
}
