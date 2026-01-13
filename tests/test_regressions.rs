use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::ContentEncoding;

#[test]
fn test_quality_prioritization_regression() {
    let data = b"Hello Quality World!".to_vec();
    let encs = vec![ContentEncoding::H, ContentEncoding::ReedSolomon(255, 223)];
    let mut generator = PDUGenerator::new(Some("SRC".to_string()), None, None, Some(encs), None, 1);
    let pdus = generator.generate(&data, None).unwrap();
    
    let mut deframer = Deframer::new();

    // 1. Receive a "noisy" version first
    let mut noisy_bytes = pdus[0].to_vec();
    let last_idx = noisy_bytes.len() - 1;
    noisy_bytes[last_idx] ^= 0x01; // Force one RS correction
    
    deframer.receive_bytes(&noisy_bytes);
    
    // 2. Receive a "clean" version next
    let clean_bytes = pdus[0].to_vec();
    deframer.receive_bytes(&clean_bytes);
    
    // Recovery
    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    
    // If prioritization is correct, it should have used the clean PDU (0 errors)
    // instead of keeping the noisy one (1 error) if noisy was received first.
    assert_eq!(recovered.unwrap()[..data.len()], data);
}
