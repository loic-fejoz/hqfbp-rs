use hqfbp_rs::codec::{conv_encode, conv_decode};
use hqfbp_rs::generator::{PDUGenerator};
use hqfbp_rs::deframer::Deframer;
use hqfbp_rs::ContentEncoding;

#[test]
fn test_conv_roundtrip() {
    let data = b"Hello Convolutional World!";
    let encoded = conv_encode(data, 7, "1/2").expect("Encode failed");
    assert!(encoded.len() >= data.len() * 2);
    
    let (decoded, _) = conv_decode(&encoded, 7, "1/2").expect("Decode failed");
    assert_eq!(decoded, data);
}

#[test]
fn test_conv_error_correction() {
    let data = b"FEC test";
    let mut encoded = conv_encode(data, 7, "1/2").expect("Encode failed");
    
    // Flip one bit
    encoded[5] ^= 0x01;
    
    let (decoded, _) = conv_decode(&encoded, 7, "1/2").expect("Decode failed");
    assert_eq!(decoded, data);
}

#[test]
fn test_conv_multiple_errors() {
    let data = b"More errors to handle";
    let mut encoded = conv_encode(data, 7, "1/2").expect("Encode failed");
    
    // Flip several bits
    encoded[2] ^= 0x40;
    encoded[10] ^= 0x02;
    encoded[20] ^= 0x80;
    
    let (decoded, _) = conv_decode(&encoded, 7, "1/2").expect("Decode failed");
    assert_eq!(decoded, data);
}

#[test]
fn test_generator_deframer_conv_integration() {
    let mut deframer = Deframer::new();
    let data = b"End-to-end convolutional test";
    
    let mut generator = PDUGenerator::new(
        Some("CONV-TEST".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Conv(7, "1/2".to_string()), ContentEncoding::H]),
        Some(vec![ContentEncoding::Identity]),
        1,
    );
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    // Feed announcement
    deframer.receive_bytes(&pdus[0]);
    
    // Feed data PDU (with some noise)
    let mut pdu_with_noise = pdus[1].clone();
    let mid = pdu_with_noise.len() / 2;
    pdu_with_noise[mid] ^= 0x01;
    
    deframer.receive_bytes(&pdu_with_noise);
    
    let mut found = false;
    while let Some(ev) = deframer.next_event() {
        if let hqfbp_rs::deframer::Event::Message(me) = ev {
            assert_eq!(me.payload, data);
            found = true;
        }
    }
    assert!(found);
}
