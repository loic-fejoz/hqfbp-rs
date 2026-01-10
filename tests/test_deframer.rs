use hqfbp_rs::generator::{PDUGenerator};
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::{Header, pack, unpack};

#[test]
fn test_deframer_single_pdu() {
    let mut deframer = Deframer::new();
    let payload = b"hello world";
    let header = Header {
        message_id: Some(1),
        src_callsign: Some("N0CALL".to_string()),
        ..Default::default()
    };
    let pdu = pack(&header, payload).expect("Pack failed");
    
    deframer.receive_bytes(&pdu);
    
    // Expect PDUEvent then MessageEvent
    let ev1 = deframer.next_event().expect("Expected PDUEvent");
    if let Event::PDU(pe) = ev1 {
        assert_eq!(pe.payload.as_ref(), payload);
    } else {
        panic!("Expected Pdu event, got {:?}", ev1);
    }
    
    let ev2 = deframer.next_event().expect("Expected MessageEvent");
    if let Event::Message(me) = ev2 {
        assert_eq!(me.payload.as_ref(), payload);
        assert_eq!(me.header.src_callsign, Some("N0CALL".to_string()));
    } else {
        panic!("Expected Message event, got {:?}", ev2);
    }
}

#[test]
fn test_deframer_chunked() {
    let mut deframer = Deframer::new();
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-1".to_string()),
        None,
        Some(10),
        None,
        None,
        1,
    );
    let data = b"This is a longer message that will be chunked.";
    let pdus = generator.generate(data, None).expect("Generate failed");
    assert!(pdus.len() > 1);
    
    let mut prev_msg_id = None;
    let mut first_orig_msg_id = None;
    
    for (i, pdu) in pdus.iter().enumerate() {
        deframer.receive_bytes(pdu);
        
        // Check that we get exactly one PDUEvent per receive_bytes
        let ev = deframer.next_event().expect("Expected Pdu event");
        if let Event::PDU(pe) = ev {
            let (_, p) = unpack(pdu.clone()).unwrap();
            assert_eq!(pe.payload, p);
            
            // Check Message-Id monotonicity in PDUs
            let curr_msg_id = pe.header.message_id.unwrap();
            if let Some(prev) = prev_msg_id {
                assert_eq!(curr_msg_id, prev + 1);
            }
            prev_msg_id = Some(curr_msg_id);
            
            // Check Original-Message-Id consistency
            let orig_id = pe.header.original_message_id;
            if first_orig_msg_id.is_none() {
                first_orig_msg_id = orig_id;
            }
            assert_eq!(orig_id, first_orig_msg_id);
        } else {
            panic!("Expected Pdu event");
        }
        
        // Check that MessageEvent is NOT emitted until the last chunk
        let msg_ev = deframer.next_event();
        if i < pdus.len() - 1 {
            assert!(msg_ev.is_none());
        } else {
            let ev = msg_ev.expect("Expected MessageEvent at last chunk");
            if let Event::Message(me) = ev {
                assert_eq!(me.payload.as_ref(), data);
                assert_eq!(me.header.src_callsign, Some("F4JXQ-1".to_string()));
                // Message-Id, Chunk-Id, Original-Message-Id, Total-Chunks are excluded from merged header
                assert!(me.header.message_id.is_none());
                assert!(me.header.chunk_id.is_none());
                assert!(me.header.original_message_id.is_none());
                assert!(me.header.total_chunks.is_none());
            } else {
                panic!("Expected Message event");
            }
        }
    }
}

#[test]
fn test_deframer_multi_sender() {
    let mut deframer = Deframer::new();
    
    // Sender 1
    let mut generator1 = PDUGenerator::new(Some("S1".to_string()), None, Some(5), None, None, 100);
    let pdus1 = generator1.generate(b"S1DATA", None).unwrap(); // 2 chunks
    
    // Sender 2
    let mut generator2 = PDUGenerator::new(Some("S2".to_string()), None, Some(5), None, None, 200);
    let pdus2 = generator2.generate(b"S2DATA", None).unwrap(); // 2 chunks
    
    // Interleave PDUs
    deframer.receive_bytes(&pdus1[0]);
    deframer.next_event(); // Skip PDU event
    assert!(deframer.next_event().is_none()); // No message yet
    
    deframer.receive_bytes(&pdus2[0]);
    deframer.next_event(); // Skip PDU event
    assert!(deframer.next_event().is_none()); // No message yet
    
    deframer.receive_bytes(&pdus1[1]);
    deframer.next_event(); // Skip PDU event
    let ev1 = deframer.next_event().expect("Expected S1 message");
    if let Event::Message(me) = ev1 {
        assert_eq!(me.payload.as_ref(), b"S1DATA");
        assert_eq!(me.header.src_callsign, Some("S1".to_string()));
    }
    
    deframer.receive_bytes(&pdus2[1]);
    deframer.next_event(); // Skip PDU event
    let ev2 = deframer.next_event().expect("Expected S2 message");
    if let Event::Message(me) = ev2 {
        assert_eq!(me.payload.as_ref(), b"S2DATA");
        assert_eq!(me.header.src_callsign, Some("S2".to_string()));
    }
}
