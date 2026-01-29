use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;

#[test]
fn test_post_asm_roundtrip() {
    let sync_word = vec![0x1A, 0xCF, 0xFC, 0x1D];

    let encs = vec![
        ContentEncoding::H,
        ContentEncoding::PostAsm(sync_word.clone()),
    ];

    let mut generator =
        PDUGenerator::new(Some("F4JXQ".to_string()), None, None, Some(encs), None, 1);

    let data = b"Hello with Post-ASM in Rust";
    let pdus = generator.generate(data, None).expect("Generation failed");
    assert_eq!(pdus.len(), 1);

    let pdu = &pdus[0];
    assert!(pdu.ends_with(&sync_word));

    let mut deframer = Deframer::new();
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
fn test_post_asm_parse() {
    let s = "post_asm(0x1acffc1d)";
    let enc = ContentEncoding::try_from(s).expect("Parse failed");
    if let ContentEncoding::PostAsm(w) = enc {
        assert_eq!(w, vec![0x1A, 0xCF, 0xFC, 0x1D]);
    } else {
        panic!("Wrong variant");
    }

    let s2 = "post_asm(43605)"; // 0xAA55
    let enc2 = ContentEncoding::try_from(s2).expect("Parse failed");
    if let ContentEncoding::PostAsm(w) = enc2 {
        assert_eq!(w, vec![0xAA, 0x55]);
    } else {
        panic!("Wrong variant");
    }
}
