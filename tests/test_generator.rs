use hqfbp_rs::generator::{PDUGenerator};
use hqfbp_rs::{Header, unpack, ContentEncoding};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

#[test]
fn test_generator_single_pdu() {
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-1".to_string()),
        None,
        None,
        None,
        None,
        1,
    );
    let data = b"Hello World";
    
    let pdus = generator.generate(data, Some("text/plain;charset=utf-8".to_string())).expect("Generate failed");
    
    assert_eq!(pdus.len(), 1);
    let (header, payload) = unpack(&pdus[0]).expect("Unpack failed");
    
    assert_eq!(header.message_id, Some(1));
    assert_eq!(header.src_callsign, Some("F4JXQ-1".to_string()));
    // text/plain;charset=utf-8 is ID 0 and thus optional.
    assert!(header.content_type.is_none());
    assert!(header.content_format.is_none());
    // No encoding, but boundary marker is always present in PDUGenerator
    match header.content_encoding {
        Some(el) => assert!(el.0.iter().any(|e| matches!(e, ContentEncoding::H))),
        _ => panic!("Expected boundary encoding"),
    }
    assert_eq!(payload, data);
}

#[test]
fn test_generator_initial_msg_id() {
    let initial_id = 42;
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-1".to_string()),
        None,
        None,
        None,
        None,
        initial_id,
    );
    let data = b"Initial ID test";
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    assert_eq!(pdus.len(), 1);
    let (header, _) = unpack(&pdus[0]).expect("Unpack failed");
    assert_eq!(header.message_id, Some(initial_id));
}

#[test]
fn test_generator_single_gzip_pdu() {
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-1".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Gzip]),
        None,
        1,
    );
    let data = b"Hello World";
    
    let pdus = generator.generate(data, Some("text/plain;charset=utf-8".to_string())).expect("Generate failed");
    
    assert_eq!(pdus.len(), 1);
    let (header, payload) = unpack(&pdus[0]).expect("Unpack failed");
    
    assert_eq!(header.message_id, Some(1));
    assert_eq!(header.src_callsign, Some("F4JXQ-1".to_string()));
    
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).unwrap();
    let compressed_data = encoder.finish().unwrap();
    
    assert_eq!(payload, compressed_data);
}

#[test]
fn test_generator_single_lzma_pdu() {
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-1".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Lzma]),
        None,
        1,
    );
    let mut data = Vec::new();
    for _ in 0..10 {
        data.extend_from_slice(b"Hello World with enough data to be worth lzma compressing");
    }
    
    let pdus = generator.generate(&data, None).expect("Generate failed");
    
    assert_eq!(pdus.len(), 1);
    let (header, payload) = unpack(&pdus[0]).expect("Unpack failed");
    
    match header.content_encoding {
        Some(el) => {
            assert_eq!(el.0, vec![ContentEncoding::Lzma, ContentEncoding::H]);
        }
        _ => panic!("Expected encoding list"),
    }
    
    let compressed = hqfbp_rs::codec::lzma_compress(&data).unwrap();
    assert_eq!(payload, compressed);
}

#[test]
fn test_generator_single_brotli_pdu() {
    let mut generator = PDUGenerator::new(
        Some("F4JXQ-1".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Brotli]),
        None,
        1,
    );
    let mut data = Vec::new();
    for _ in 0..10 {
        data.extend_from_slice(b"Hello World with enough data to be worth brotli compressing");
    }
    
    let pdus = generator.generate(&data, None).expect("Generate failed");
    
    assert_eq!(pdus.len(), 1);
    let (header, payload) = unpack(&pdus[0]).expect("Unpack failed");
    
    match header.content_encoding {
        Some(el) => {
            assert_eq!(el.0, vec![ContentEncoding::Brotli, ContentEncoding::H]);
        }
        _ => panic!("Expected encoding list"),
    }
    
    let mut decompressed = Vec::new();
    let mut brotli_decoder = brotli::Decompressor::new(&payload[..], 4096);
    std::io::copy(&mut brotli_decoder, &mut decompressed).unwrap();
    
    assert_eq!(decompressed, data);
}

#[test]
fn test_generator_gzip_before_chunking() {
    let data = vec![b'A'; 100];
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&data).unwrap();
    let compressed_data = encoder.finish().unwrap();
    
    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        Some(50),
        Some(vec![ContentEncoding::Gzip]),
        None,
        1,
    );
    let pdus = generator.generate(&data, None).expect("Generate failed");
    
    assert_eq!(pdus.len(), 1);
    let (header, payload) = unpack(&pdus[0]).expect("Unpack failed");
    assert_eq!(payload, compressed_data);
    assert_eq!(header.file_size, Some(100));
}

#[test]
fn test_generator_crc_payload_only() {
    // Pre-boundary CRC (payload only)
    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Crc32]),
        None,
        1,
    );
    let data = b"payload test";
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    let (_, payload) = unpack(&pdus[0]).expect("Unpack failed");
    // The whole payload should have CRC at the end
    let crc = &payload[payload.len()-4..];
    let original = &payload[..payload.len()-4];
    assert_eq!(original, data);
    assert_eq!(crc, hqfbp_rs::codec::crc32_std(original));
}

#[test]
fn test_generator_crc_covering_header() {
    // Post-boundary CRC (covering header + payload)
    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::H, ContentEncoding::Crc32]),
        None,
        1,
    );
    let data = b"covered test";
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    // The whole PDU should have CRC at the end
    let pdu = &pdus[0];
    let crc = &pdu[pdu.len()-4..];
    let pdu_no_crc = &pdu[..pdu.len()-4];
    assert_eq!(crc, hqfbp_rs::codec::crc32_std(pdu_no_crc));
    
    // Now unpack the PDU without CRC
    let (header, payload) = unpack(pdu_no_crc).expect("Unpack failed");
    assert_eq!(payload, data);
    match header.content_encoding {
        Some(el) => assert_eq!(el.0, vec![ContentEncoding::H, ContentEncoding::Crc32]),
        _ => panic!("Expected encoding list"),
    }
}

#[test]
fn test_generator_announcement() {
    let data = b"Some data";
    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        None,
        Some(vec![ContentEncoding::Gzip, ContentEncoding::H, ContentEncoding::Crc32]),
        Some(vec![ContentEncoding::H, ContentEncoding::Crc16]),
        1,
    );
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    // We expect 2 PDUs: Announcement + One Data PDU
    assert_eq!(pdus.len(), 2);
    
    // 1. Verify Announcement PDU
    let ann_pdu = &pdus[0];
    let ann_crc = &ann_pdu[ann_pdu.len()-2..];
    let ann_pdu_no_crc = &ann_pdu[..ann_pdu.len()-2];
    assert_eq!(ann_crc, hqfbp_rs::codec::crc16_ccitt(ann_pdu_no_crc));
    
    let (ann_h, ann_p_bytes) = unpack(ann_pdu_no_crc).expect("Unpack ann failed");
    assert_eq!(ann_h.content_type, Some("application/vnd.hqfbp+cbor".to_string()));
    
    // Decode announcement payload
    let ann_header: Header = minicbor::decode(&ann_p_bytes).expect("Decode ann payload failed");
    // Announcement referred msg-id is 2
    assert_eq!(ann_header.message_id, Some(2));
    match ann_header.content_encoding {
        Some(el) => assert_eq!(el.0, vec![ContentEncoding::Gzip, ContentEncoding::H, ContentEncoding::Crc32]),
        _ => panic!("Expected encoding list in ann"),
    }
    
    // 2. Verify Data PDU
    let data_pdu = &pdus[1];
    let data_crc = &data_pdu[data_pdu.len()-4..];
    let data_pdu_no_crc = &data_pdu[..data_pdu.len()-4];
    assert_eq!(data_crc, hqfbp_rs::codec::crc32_std(data_pdu_no_crc));
    let (data_h, data_p) = unpack(data_pdu_no_crc).expect("Unpack data failed");
    
    assert_eq!(data_h.message_id, Some(2));
    
    let mut decoder = flate2::read::GzDecoder::new(&data_p[..]);
    let mut decompressed = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut decompressed).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn test_generator_chunking() {
    let mut generator = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None,
        Some(10),
        None,
        None,
        1,
    );
    let data = b"This is a longer piece of data that should be chunked.";
    assert_eq!(data.len(), 54);
    
    let pdus = generator.generate(data, None).expect("Generate failed");
    
    // 54 bytes with max 10 payload -> 6 chunks
    assert_eq!(pdus.len(), 6);
    
    let mut reassembled = Vec::new();
    for (i, pdu) in pdus.iter().enumerate() {
        let (header, payload) = unpack(pdu).expect("Unpack failed");
        assert_eq!(header.chunk_id, Some(i as u32));
        assert_eq!(header.total_chunks, Some(6));
        assert_eq!(header.original_message_id, Some(1));
        reassembled.extend_from_slice(&payload);
    }
    assert_eq!(reassembled, data);
}
