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

#[test]
fn test_deframer_announcement_and_crc() {
    let mut deframer = Deframer::new();
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-2".to_string()),
        None,
        None,
        Some(vec![hqfbp_rs::ContentEncoding::H, hqfbp_rs::ContentEncoding::Crc32]),
        Some(vec![hqfbp_rs::ContentEncoding::Identity]),
        1,
    );
    let data = b"Sensitive Data";
    let pdus = generator.generate(data, None).unwrap();
    
    deframer.receive_bytes(&pdus[0]); // Announcement
    deframer.receive_bytes(&pdus[1]); // Data
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert!(me.payload.starts_with(data));
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn test_deframer_compression() {
    let mut deframer = Deframer::new();
    let data = b"Compress me please!".repeat(10);
    let mut generator = PDUGenerator::new(
        Some("GZIPPER".to_string()),
        None,
        None,
        Some(vec![hqfbp_rs::ContentEncoding::Gzip]),
        None,
        1,
    );
    let pdus = generator.generate(&data, None).unwrap();
    for pdu in pdus {
        deframer.receive_bytes(&pdu);
    }
    
    let mut msg_ev = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            msg_ev = Some(me);
        }
    }
    let msg = msg_ev.expect("Message expected");
    assert_eq!(msg.payload.as_ref(), data);
}

#[test]
fn test_deframer_heuristic_gzip_header() {
    let mut deframer = Deframer::new();
    let data = b"Heuristic data with gzipped header";
    let mut generator = PDUGenerator::new(
        Some("HEURISTIC-1".to_string()),
        None,
        None,
        Some(vec![hqfbp_rs::ContentEncoding::H, hqfbp_rs::ContentEncoding::Gzip]),
        Some(vec![hqfbp_rs::ContentEncoding::Identity]),
        1,
    );
    let pdus = generator.generate(data, None).unwrap();
    
    deframer.receive_bytes(&pdus[0]); // Announcement
    deframer.receive_bytes(&pdus[1]); // Data (heuristic)
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert!(me.payload.starts_with(data));
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn test_deframer_heuristic_multi_encodings() {
    let mut deframer = Deframer::new();
    let data = b"Multi-layer heuristic test";
    let mut generator = PDUGenerator::new(
        Some("HEURISTIC-2".to_string()),
        None,
        None,
        Some(vec![hqfbp_rs::ContentEncoding::H, hqfbp_rs::ContentEncoding::Gzip, hqfbp_rs::ContentEncoding::Crc32]),
        Some(vec![hqfbp_rs::ContentEncoding::Identity]),
        1,
    );
    let pdus = generator.generate(data, None).unwrap();
    
    deframer.receive_bytes(&pdus[0]);
    deframer.receive_bytes(&pdus[1]);
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert!(me.payload.starts_with(data));
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn test_deframer_multi_sender_interleaved_announcements() {
    let mut deframer = Deframer::new();
    
    // S1: Standard
    let mut gen1 = PDUGenerator::new(Some("S1".to_string()), None, Some(10), None, None, 1);
    let data1 = b"S1: Basic data";
    let pdus1 = gen1.generate(data1, None).unwrap();
    
    // S2: Gzip post-boundary
    let mut gen2 = PDUGenerator::new(
        Some("S2".to_string()), 
        None, 
        Some(10), 
        Some(vec![hqfbp_rs::ContentEncoding::H, hqfbp_rs::ContentEncoding::Gzip]),
        Some(vec![hqfbp_rs::ContentEncoding::Identity]),
        1
    );
    let data2 = b"S2: Gzipped header data";
    let pdus2 = gen2.generate(data2, None).unwrap();
    
    // S3: Complex
    let mut gen3 = PDUGenerator::new(
        Some("S3".to_string()), 
        None, 
        Some(10), 
        Some(vec![hqfbp_rs::ContentEncoding::H, hqfbp_rs::ContentEncoding::Gzip, hqfbp_rs::ContentEncoding::Crc32]),
        Some(vec![hqfbp_rs::ContentEncoding::Identity]),
        1
    );
    let data3 = b"S3: Double trouble";
    let pdus3 = gen3.generate(data3, None).unwrap();
    
    let mut all_pdus = Vec::new();
    let max_len = pdus1.len().max(pdus2.len()).max(pdus3.len());
    for i in range(0..max_len) {
        if i < pdus1.len() { all_pdus.push(&pdus1[i]); }
        if i < pdus2.len() { all_pdus.push(&pdus2[i]); }
        if i < pdus3.len() { all_pdus.push(&pdus3[i]); }
    }
    
    for pdu in all_pdus {
        deframer.receive_bytes(pdu);
    }
    
    let mut results = std::collections::HashMap::new();
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            results.insert(me.header.src_callsign.clone().unwrap(), me.payload.clone());
        }
    }
    
    assert_eq!(results.get("S1").unwrap().as_ref(), data1);
    assert_eq!(results.get("S2").unwrap().as_ref(), data2);
    assert_eq!(results.get("S3").unwrap().as_ref(), data3);
    assert_eq!(results.len(), 3);
}

fn range(r: std::ops::Range<usize>) -> std::ops::Range<usize> { r }
