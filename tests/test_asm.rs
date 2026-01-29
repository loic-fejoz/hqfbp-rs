use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;

#[test]
fn test_asm_roundtrip() {
    let sync_word = vec![0x1A, 0xCF, 0xFC, 0x1D];

    // Order matters: ASM is applied LAST on encode (outermost), so it appears FIRST in list if we think transmission order.
    // Wait, PDUGenerator applies encodings in order.
    // Generally framing: [SYNC][HEADER+PAYLOAD]
    // So if we have H (Header) encoding, ASM should wrap it.
    // If encodings = [H, ASM], then ASM(H(payload)) -> SYNC + header + payload.
    // Let's verify that.

    let encs = vec![ContentEncoding::H, ContentEncoding::Asm(sync_word.clone())];

    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        None,
        Some(encs),
        Some(vec![ContentEncoding::H]),
        1,
    );

    let data = b"Hello with ASM in Rust";
    let pdus = generator.generate(data, None).expect("Generation failed");
    assert_eq!(pdus.len(), 2);

    // First PDU is announcement (H encoded).
    let ann_pdu = &pdus[0];

    // Second PDU is Data (ASM encoded).
    let pdu = &pdus[1];
    assert!(pdu.starts_with(&sync_word));

    let mut deframer = Deframer::new();
    // feed announcement
    deframer.receive_bytes(ann_pdu);
    // feed data
    deframer.receive_bytes(pdu);

    let mut events = Vec::new();
    while let Some(ev) = deframer.next_event() {
        events.push(ev);
    }

    let msg_ev = events
        .iter()
        .find_map(|ev| {
            if let Event::Message(m) = ev {
                Some(m)
            } else {
                None
            }
        })
        .expect("No MessageEvent found");

    assert_eq!(msg_ev.payload.as_ref(), data);
}

#[test]
fn test_asm_parse() {
    let s = "asm(0x1acffc1d)";
    let enc = ContentEncoding::try_from(s).expect("Parse failed");
    if let ContentEncoding::Asm(w) = enc {
        assert_eq!(w, vec![0x1A, 0xCF, 0xFC, 0x1D]);
    } else {
        panic!("Wrong variant");
    }

    let s2 = "asm(43605)"; // 0xAA55
    let enc2 = ContentEncoding::try_from(s2).expect("Parse failed");
    if let ContentEncoding::Asm(w) = enc2 {
        assert_eq!(w, vec![0xAA, 0x55]);
    } else {
        panic!("Wrong variant");
    }
}
