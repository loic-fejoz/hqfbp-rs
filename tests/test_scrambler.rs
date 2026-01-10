use hqfbp_rs::codec::scr_xor;
use hqfbp_rs::generator::{PDUGenerator};
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::ContentEncoding;

#[test]
fn test_scrambler_roundtrip() {
    let data = b"Scrambler test data with some zeros: \x00\x00\x00\x00";
    let poly = 0x1FF; // NASA-like 9-bit polynomial
    
    let encoded = scr_xor(data, poly);
    assert_ne!(encoded, data);
    
    let decoded = scr_xor(&encoded, poly);
    assert_eq!(decoded, data);
}

#[test]
fn test_scrambler_whitening() {
    // Long string of zeros should become high-entropy with a good polynomial
    let data = vec![0u8; 100];
    // PRBS-9: x^9 + x^5 + 1 -> binary 100010000 (bits 8 and 4 set) -> 0x110
    let poly = 0x110;
    
    let encoded = scr_xor(&data, poly);
    
    // Check that we don't have too many zeros
    let zero_count = encoded.iter().filter(|&&b| b == 0).count();
    // Statically, should be around 50/100.
    assert!(zero_count < 70);
    
    // And it should have many different values
    let unique_values: std::collections::HashSet<u8> = encoded.iter().cloned().collect();
    assert!(unique_values.len() > 10);
}

#[test]
fn test_generator_deframer_scrambler_integration() {
    let mut deframer = Deframer::new();
    let data = b"End-to-end scrambling test";
    // G3RUH-like: x^17 + x^12 + 1 -> 0x10800
    let poly = 0x10800;
    
    let mut generator = PDUGenerator::new(
        Some("SCR-TEST".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Scrambler(poly as u64), ContentEncoding::H]),
        Some(vec![ContentEncoding::Identity]),
        1,
    );
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    // Feed announcement
    deframer.receive_bytes(&pdus[0]);
    
    // Feed data PDU
    deframer.receive_bytes(&pdus[1]);
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            assert_eq!(me.payload.as_ref(), data);
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn test_scrambler_different_polynomials() {
    let data = b"Testing different polynomials";
    // Use two different primitive polynomials
    let p1 = 0x110; // x^9 + x^5 + 1
    let p2 = 0x10800; // x^17 + x^12 + 1
    
    let e1 = scr_xor(data, p1);
    let e2 = scr_xor(data, p2);
    
    assert_ne!(e1, e2);
    assert_eq!(scr_xor(&e1, p1), data);
    assert_eq!(scr_xor(&e2, p2), data);
}
