use hqfbp_rs::codec::{rq_encode, rq_decode};
use hqfbp_rs::generator::{PDUGenerator};
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::ContentEncoding;


#[test]
fn test_rq_basic_encode_decode() {
    let mut data = Vec::new();
    for _ in 0..10 {
        data.extend_from_slice(b"Hello RaptorQ! ");
    }
    let mtu = 10;
    let repair = 2;
    
    let encoded = rq_encode(&data, data.len(), mtu, repair).expect("Encode failed");
    let decoded = rq_decode(encoded, data.len(), mtu).expect("Decode failed");
    
    assert_eq!(decoded, data);
}

#[test]
fn test_rq_with_loss() {
    let mut data = Vec::new();
    for _ in 0..5 {
        data.extend_from_slice(b"Resilient Data Transmission");
    }
    let mtu = 10;
    let repair = 5;
    
    let mut packets = rq_encode(&data, data.len(), mtu, repair).expect("Encode failed");
 
    // We should have some number of packets. 
    // k = ceil(135 / 10) = 14 source packets + 5 repair = 19 packets
    assert_eq!(packets.len(), 19);
    
    // Simulate losing 3 packets (we still have 16, which is >= 14)
    packets.remove(2);
    packets.remove(5);
    packets.remove(10);
    
    // Decoding should still work
    let decoded = rq_decode(packets, data.len(), mtu).expect("Decode failed");
    assert_eq!(decoded, data);
}

#[test]
fn test_generator_deframer_rq_post_boundary() {
    let data = b"End-to-end RaptorQ test data";
    let rq_len = data.len() + 45; // Must be greater than len(data + CBOR header)
    let mtu = (rq_len + 60) as u16;
    let repair_count = 5;
    
    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::H, ContentEncoding::RaptorQ(rq_len, mtu, repair_count)]),
        Some(vec![ContentEncoding::Identity]),
        1,
    );
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    assert!(pdus.len() >= 1 + repair_count as usize); // 7 PDUs actually
    let mut deframer = Deframer::new();
    for pdu in pdus {
        deframer.receive_bytes(&pdu);
    }
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert!(me.payload.as_ref().starts_with(data));
            found = true;
        }
    }
    assert!(found, "Message not deframed");
}

#[test]
fn test_rq_decode_insufficient_symbols() {
    let data = b"Limited redundancy";
    let mtu = 4;
    let repair = 1;
    
    let mut packets = rq_encode(data, data.len(), mtu, repair).expect("Encode failed");
    
    // Lose many symbols. 
    if packets.len() > 3 {
        packets.remove(0);
        packets.remove(0);
        packets.remove(0);
    }
    
    let res = rq_decode(packets, data.len(), mtu);
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("insufficient symbols"));
}
