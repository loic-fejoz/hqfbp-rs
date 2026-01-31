use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;

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
        Some(vec![
            ContentEncoding::RaptorQ(dlen, mtu, repair),
            ContentEncoding::H,
        ]),
        Some(vec![ContentEncoding::Identity]),
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

    let mut msg_ev = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            msg_ev = Some(me);
        }
    }
    let me = msg_ev.expect("Message should be decoded after 10 packets");
    assert_eq!(me.payload, data);
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
        Some(vec![
            ContentEncoding::RaptorQ(dlen, mtu, repair),
            ContentEncoding::H,
        ]),
        Some(vec![ContentEncoding::Identity]),
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

    let mut msg_ev = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            msg_ev = Some(me);
        }
    }
    let me = msg_ev.expect("Should decode with 8 symbols even if some source packets are missing");
    assert_eq!(me.payload, data);
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
        Some(vec![
            ContentEncoding::RaptorQ(dlen, mtu, repair),
            ContentEncoding::H,
        ]),
        Some(vec![ContentEncoding::Identity]),
        1,
    );

    let pdus = generator.generate(&data, None).expect("Generate failed");
    deframer.receive_bytes(&pdus[0]);

    // Feed 3 packets (not enough)
    for pdu in &pdus[1..4] {
        deframer.receive_bytes(pdu);
    }

    // No MessageEvent (other than announcement) yet
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            if me.payload.len() == data.len() {
                panic!("Premature message event");
            }
        }
    }

    // Feed 4th packet
    deframer.receive_bytes(&pdus[4]);

    let mut msg_ev = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            msg_ev = Some(me);
        }
    }
    let me = msg_ev.expect("Expected message event");
    assert_eq!(me.payload, data);
}
