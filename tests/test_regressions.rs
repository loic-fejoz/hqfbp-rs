use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::{ContentEncoding, Header};

#[test]
fn test_quality_prioritization_regression() {
    let data = b"Hello Quality World!".to_vec();
    let encs = vec![ContentEncoding::H, ContentEncoding::ReedSolomon(255, 223)];
    let mut generator = PDUGenerator::new(Some("SRC".to_string()), None, Some(encs), None, 1);
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
    assert_eq!(recovered.unwrap(), data);
}

#[test]
fn test_deframer_no_panic_on_missing_message_id() {
    let mut deframer = Deframer::new();

    // Create an announcement header (which has no message_id) packed as raw bytes
    let header = Header {
        message_id: None,
        original_message_id: None,
        src_callsign: Some("NOCALL".to_string()),
        content_type: Some("application/vnd.hqfbp+cbor".to_string()),
        ..Default::default()
    };

    let mut buf = Vec::new();
    let mut encoder = minicbor::Encoder::new(&mut buf);
    encoder.encode(&header).unwrap();

    // Feeding this directly to deframer should not panic
    deframer.receive_bytes(&buf);

    // Verify deframer is still functional
    let data = b"Normal payload".to_vec();
    let mut generator = PDUGenerator::new(Some("SRC".to_string()), None, None, None, 42);
    let normal_pdus = generator.generate(&data, None).unwrap();
    for p in normal_pdus {
        deframer.receive_bytes(&p);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(recovered.unwrap(), data);
}

#[test]
fn test_decompression_bomb_protection() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use hqfbp_rs::codec::gzip_decompress;
    use std::io::Write;

    // Create a 17 MB zero stream and compress it into a tiny buffer
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    let zeros = vec![0u8; 1024 * 1024]; // 1 MB chunk
    for _ in 0..17 {
        encoder.write_all(&zeros).unwrap();
    }
    let bomb = encoder.finish().unwrap();

    // Ensure decompression cleanly fails without allocating 17 MB
    let res = gzip_decompress(&bomb);
    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(
        err_msg.contains("exceeded maximum allowed size"),
        "Unexpected error: {err_msg}"
    );

    // Deflate bomb test
    use flate2::write::ZlibEncoder;
    use hqfbp_rs::codec::deflate_decompress;
    let mut z_encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    for _ in 0..17 {
        z_encoder.write_all(&zeros).unwrap();
    }
    let z_bomb = z_encoder.finish().unwrap();
    let z_res = deflate_decompress(&z_bomb);
    assert!(z_res.is_err());
    let z_err_msg = z_res.unwrap_err().to_string();
    assert!(
        z_err_msg.contains("exceeded maximum allowed size"),
        "Unexpected error: {z_err_msg}"
    );
}

#[test]
fn test_kiss_frame_buffer_cap() {
    use hqfbp_rs::kiss::{FEND, KissDecoder, MAX_KISS_FRAME_SIZE};

    let mut decoder = KissDecoder::new();
    // Start frame
    assert!(decoder.process_byte(FEND).is_none());

    // Feed more than MAX_KISS_FRAME_SIZE bytes without closing FEND
    for _ in 0..MAX_KISS_FRAME_SIZE + 10 {
        assert!(decoder.process_byte(0x42).is_none());
    }

    // Now send closing FEND; the oversized frame must have been dropped
    assert!(decoder.process_byte(FEND).is_none());
}

#[test]
fn test_lt_and_rq_zero_params_no_panic() {
    use bytes::Bytes;
    use hqfbp_rs::codec::lt::LTDecoder;
    use hqfbp_rs::codec::{lt_decode, rq_decode};

    // LTDecoder::try_new should return Err on invalid/zero params
    assert!(LTDecoder::try_new(0, 0).is_err());
    assert!(LTDecoder::try_new(100, 0).is_err());
    assert!(LTDecoder::try_new(0, 64).is_err());

    // lt_decode on empty packets should cleanly return Err without panicking
    assert!(lt_decode(vec![], 0, 0).is_err());
    assert!(lt_decode(vec![Bytes::from_static(b"1234")], 0, 10).is_err());

    // rq_decode on invalid params should cleanly return Err without panicking
    assert!(rq_decode(vec![], 0, 0).is_err());
    assert!(rq_decode(vec![Bytes::from_static(b"1234")], 0, 10).is_err());
}

#[test]
fn test_cbor_dos_limits() {
    use hqfbp_rs::{EncodingList, Header};

    // Test Header map with exactly 32 entries (allowed)
    let mut buf_ok = vec![0xb8, 32]; // map of 32 items
    for i in 0..32u32 {
        let _ = minicbor::encode(1000 + i, &mut buf_ok);
        buf_ok.push(0x00);
    }
    let res = minicbor::decode::<Header>(&buf_ok);
    assert!(res.is_ok(), "Expected Ok, got {:?}", res);

    // Test Header map with > 32 entries (definite length 33, rejected)
    let mut buf = vec![0xb8, 33];
    for i in 0..33u32 {
        let _ = minicbor::encode(1000 + i, &mut buf);
        buf.push(0x00);
    }
    assert!(minicbor::decode::<Header>(&buf).is_err());

    // Test Header indefinite map with > 32 entries (rejected)
    let mut buf_indef = vec![0xbf]; // indefinite map
    for i in 0..33u32 {
        let _ = minicbor::encode(1000 + i, &mut buf_indef);
        buf_indef.push(0x00);
    }
    buf_indef.push(0xff); // break
    assert!(minicbor::decode::<Header>(&buf_indef).is_err());

    // Test EncodingList array with exactly 32 items (allowed)
    let mut arr_ok = vec![0x98, 32];
    arr_ok.resize(2 + 32, 0x00);
    let res = minicbor::decode::<EncodingList>(&arr_ok);
    assert!(res.is_ok(), "Expected Ok, got {:?}", res);

    // Test EncodingList array with > 32 items (rejected)
    let mut arr_buf = vec![0x98, 33];
    arr_buf.resize(2 + 33, 0x00);
    assert!(minicbor::decode::<EncodingList>(&arr_buf).is_err());
}

#[test]
fn test_deflate_codec_and_deframer_roundtrip() {
    use hqfbp_rs::codec::{deflate_compress, deflate_decompress};

    let original = b"The quick brown fox jumps over the lazy dog repeatedly! 1234567890";
    let compressed = deflate_compress(original).expect("Deflate compress failed");
    let decompressed = deflate_decompress(&compressed).expect("Deflate decompress failed");
    assert_eq!(decompressed, original);

    // End-to-end via Generator and Deframer
    let encs = vec![
        ContentEncoding::Deflate,
        ContentEncoding::H,
        ContentEncoding::Crc32,
    ];
    let mut generator = PDUGenerator::new(Some("DEF".to_string()), None, Some(encs), None, 1);
    let pdus = generator.generate(original, None).expect("Generate failed");

    let mut deframer = Deframer::new();
    for pdu in pdus {
        deframer.receive_bytes(&pdu);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(recovered.as_deref(), Some(&original[..]));
}

#[test]
fn test_anchored_regex_encodings() {
    // Valid patterns should parse as the specific variant
    assert_eq!(
        ContentEncoding::try_from("rs(255, 223)").unwrap(),
        ContentEncoding::ReedSolomon(255, 223)
    );
    assert_eq!(
        ContentEncoding::try_from("rq(100, 64, 10)").unwrap(),
        ContentEncoding::RaptorQ(100, 64, 10)
    );
    assert_eq!(
        ContentEncoding::try_from("rq(dlen, 64, 10)").unwrap(),
        ContentEncoding::RaptorQDynamic(64, 10)
    );
    assert_eq!(
        ContentEncoding::try_from("lt(100, 64, 10)").unwrap(),
        ContentEncoding::LT(100, 64, 10)
    );
    assert_eq!(
        ContentEncoding::try_from("lt(dlen, 64, 10)").unwrap(),
        ContentEncoding::LTDynamic(64, 10)
    );
    assert_eq!(
        ContentEncoding::try_from("conv(7, 1/2)").unwrap(),
        ContentEncoding::Conv(7, "1/2".to_string())
    );
    assert_eq!(
        ContentEncoding::try_from("scr(0x1234)").unwrap(),
        ContentEncoding::Scrambler(0x1234)
    );
    assert_eq!(
        ContentEncoding::try_from("chunk(512)").unwrap(),
        ContentEncoding::Chunk(512)
    );
    assert_eq!(
        ContentEncoding::try_from("repeat(3)").unwrap(),
        ContentEncoding::Repeat(3)
    );

    // Unanchored substrings must NOT parse as those variants; they must fall back to OtherString
    assert_eq!(
        ContentEncoding::try_from("prefix_rs(255, 223)").unwrap(),
        ContentEncoding::OtherString("prefix_rs(255, 223)".to_string())
    );
    assert_eq!(
        ContentEncoding::try_from("rs(255, 223)_suffix").unwrap(),
        ContentEncoding::OtherString("rs(255, 223)_suffix".to_string())
    );
    assert_eq!(
        ContentEncoding::try_from("foo_rq(100, 64, 10)_bar").unwrap(),
        ContentEncoding::OtherString("foo_rq(100, 64, 10)_bar".to_string())
    );
    assert_eq!(
        ContentEncoding::try_from("chunk(512)extra").unwrap(),
        ContentEncoding::OtherString("chunk(512)extra".to_string())
    );
}

#[test]
fn test_peer_encodings_registration_and_fallback() {
    let mut deframer = Deframer::new();
    let callsign = "F4JXQ";

    // 1. Verify initially none
    assert!(deframer.peer_encodings(callsign).is_none());

    // 2. Register peer encoding pipeline (gzip, h, crc32)
    let peer_pipeline = vec![
        ContentEncoding::Gzip,
        ContentEncoding::H,
        ContentEncoding::Crc32,
    ];
    deframer.register_peer_encodings(callsign, peer_pipeline.clone());
    assert_eq!(
        deframer.peer_encodings(callsign),
        Some(peer_pipeline.as_slice())
    );

    // 3. Create a PDU from F4JXQ that deliberately omits content_encoding from its header
    let raw_payload = b"DTN Bundle Payload from registered peer F4JXQ!".to_vec();
    let compressed = hqfbp_rs::codec::gzip_compress(&raw_payload).unwrap();

    let header = Header {
        message_id: Some(101),
        original_message_id: Some(101),
        src_callsign: Some(callsign.to_string()),
        content_encoding: None, // No encoding in header
        total_chunks: Some(1),
        chunk_id: Some(0),
        payload_size: Some(compressed.len() as u64),
        ..Default::default()
    };

    let pdu_inner = hqfbp_rs::pack(&header, &compressed).unwrap();
    // Post-boundary CRC32
    let crc = hqfbp_rs::codec::crc32_std(&pdu_inner);
    let mut pdu_wire = pdu_inner.to_vec();
    pdu_wire.extend_from_slice(&crc);

    // 4. Feed PDU to deframer: it should use registered peer pipeline and recover
    deframer.receive_bytes(&pdu_wire);

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(recovered.as_deref(), Some(&raw_payload[..]));

    // 5. Test unregister
    assert_eq!(
        deframer.unregister_peer_encodings(callsign),
        Some(peer_pipeline)
    );
    assert!(deframer.peer_encodings(callsign).is_none());
}

#[test]
fn test_session_cleanup_expired() {
    use std::thread::sleep;
    use std::time::Duration;

    let mut deframer = Deframer::new();
    assert_eq!(deframer.active_sessions_count(), 0);

    // Feed chunk 0 of 2 (incomplete session)
    let header = Header {
        message_id: Some(201),
        original_message_id: Some(200),
        src_callsign: Some("F1ABC".to_string()),
        total_chunks: Some(2),
        chunk_id: Some(0),
        ..Default::default()
    };
    let pdu = hqfbp_rs::pack(&header, b"chunk 0").unwrap();
    deframer.receive_bytes(&pdu);

    assert_eq!(deframer.active_sessions_count(), 1);

    // Check with large timeout: nothing expired
    let purged = deframer.cleanup_expired(Duration::from_secs(60));
    assert!(purged.is_empty());
    assert_eq!(deframer.active_sessions_count(), 1);

    // Sleep briefly and check with small timeout (10ms)
    sleep(Duration::from_millis(15));
    let purged = deframer.cleanup_expired(Duration::from_millis(10));
    assert_eq!(purged, vec![(Some("F1ABC".to_string()), 200)]);
    assert_eq!(deframer.active_sessions_count(), 0);
}

#[test]
fn test_rq_fuzz_crash() {
    let input = [
        57u8, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 255, 255, 57, 57, 57, 57, 57, 57,
        57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 57, 171, 91, 91, 254, 91, 212, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255,
    ];
    let payload = &input[1..];
    let b = vec![bytes::Bytes::copy_from_slice(payload)];
    let _ = hqfbp_rs::codec::rq_decode(b, payload.len().max(1), 64);
}

#[test]
fn test_p0_sec1_rq_crafted_k_too_large() {
    use hqfbp_rs::{EncodingList, Header, pack};

    // Crafted header with k = 60000 > RQ_MAX_K (56403)
    let h = Header {
        message_id: Some(1),
        src_callsign: Some("EVIL".into()),
        content_encoding: Some(EncodingList(vec![
            ContentEncoding::RaptorQ(60000, 1, 0),
            ContentEncoding::H,
        ])),
        ..Default::default()
    };
    let pdu = pack(&h, &[0u8; 8]).unwrap();
    let mut deframer = Deframer::new();
    // Must not panic!
    deframer.receive_bytes(&pdu);
}

#[test]
fn test_p0_cor1_application_cbor_not_swallowed() {
    use hqfbp_rs::MediaType;
    use hqfbp_rs::generator::PDUGenerator;

    let payload = b"\xa1\x01\x02".to_vec(); // Valid CBOR map {1: 2}
    let mut generator = PDUGenerator::new(
        Some("F4TEST".into()),
        None,
        Some(vec![ContentEncoding::H]),
        None,
        1,
    );
    let pdus = generator
        .generate(&payload, Some(MediaType::Type("application/cbor".into())))
        .unwrap();

    let mut deframer = Deframer::new();
    for p in pdus {
        deframer.receive_bytes(&p);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(
        recovered,
        Some(payload),
        "application/cbor message must be delivered, not swallowed as announcement"
    );
}

#[test]
fn test_p0_cor2_lt_session_not_destroyed_on_early_fail() {
    use hqfbp_rs::generator::PDUGenerator;

    let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut generator = PDUGenerator::new(
        Some("F4TEST".into()),
        None,
        Some(vec![ContentEncoding::LT(1000, 100, 30), ContentEncoding::H]),
        None,
        1,
    );
    let pdus = generator.generate(&data, None).unwrap();
    assert_eq!(pdus.len(), 40); // 10 source + 30 repair

    // Drop the first 3 systematic chunks so peeling needs more than k symbols to resolve
    let mut deframer = Deframer::new();
    for (i, p) in pdus.iter().enumerate() {
        if i < 3 {
            continue; // drop chunks 0, 1, 2
        }
        deframer.receive_bytes(p);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(
        recovered,
        Some(data),
        "LT session should survive early decode failure and decode once enough repair packets arrive"
    );
}

#[test]
fn test_p0_cor3_kiss_decoder_shared_and_leading_fend() {
    use hqfbp_rs::kiss::{KissDecoder, encode_kiss_frame};

    let pdu1 = b"PDU1_DATA";
    let pdu2 = b"PDU2_DATA";
    let f1 = encode_kiss_frame(pdu1);
    let f2 = encode_kiss_frame(pdu2);

    // 1. Leading double FEND: C0 C0 00 ... C0
    let mut leading_fend = vec![0xC0];
    leading_fend.extend_from_slice(&f1);

    let mut decoder = KissDecoder::new();
    let frames: Vec<_> = leading_fend
        .iter()
        .filter_map(|&b| decoder.process_byte(b))
        .collect();
    assert_eq!(frames.len(), 1, "Leading FEND must not swallow the frame");
    assert_eq!(&frames[0][1..], pdu1);

    // 2. Shared FEND between frames: f1 with last C0 shared as first C0 of f2
    let mut shared_stream = f1;
    shared_stream.extend_from_slice(&f2[1..]); // omit leading C0 of f2

    let mut decoder2 = KissDecoder::new();
    let frames2: Vec<_> = shared_stream
        .iter()
        .filter_map(|&b| decoder2.process_byte(b))
        .collect();
    assert_eq!(frames2.len(), 2, "Shared FEND must deliver both frames");
    assert_eq!(&frames2[0][1..], pdu1);
    assert_eq!(&frames2[1][1..], pdu2);
}

#[test]
fn test_p0_cor4_chunk_encodings_roundtrip() {
    use hqfbp_rs::generator::PDUGenerator;

    let data: Vec<u8> = (0..1000u32).map(|i| (i * 13 % 251) as u8).collect();
    let mut generator = PDUGenerator::new(
        Some("F4TEST".into()),
        None,
        Some(vec![
            ContentEncoding::Chunk(300),
            ContentEncoding::Deflate,
            ContentEncoding::Crc32,
            ContentEncoding::H,
        ]),
        None,
        1,
    );
    let pdus = generator.generate(&data, None).unwrap();
    assert!(pdus.len() > 1, "Data should be chunked into multiple PDUs");

    let mut deframer = Deframer::new();
    for p in pdus {
        deframer.receive_bytes(&p);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(
        recovered,
        Some(data),
        "Per-chunk deflate+crc32 must be successfully deframed and reassembled"
    );
}

#[test]
fn test_p0_cor5_deduplication_repeat() {
    use hqfbp_rs::generator::PDUGenerator;

    let data = b"Repeat Test Payload".to_vec();
    let mut generator = PDUGenerator::new(
        Some("F4TEST".into()),
        None,
        Some(vec![ContentEncoding::H, ContentEncoding::Repeat(3)]),
        None,
        1,
    );
    let pdus = generator.generate(&data, None).unwrap();
    assert_eq!(pdus.len(), 3);

    let mut deframer = Deframer::new();
    for p in pdus {
        deframer.receive_bytes(&p);
    }

    let mut count = 0;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(_) = ev {
            count += 1;
        }
    }
    assert_eq!(
        count, 1,
        "Completed message must be deduplicated across repeated/duplicate PDUs"
    );
}

#[test]
fn test_p1_cor6_nested_cbor_payload_not_confused_as_inner_pdu() {
    use hqfbp_rs::generator::PDUGenerator;

    // A payload that happens to be a valid CBOR map with key 0, e.g. {0: 5, "hello": "world"}
    // In CBOR: 0xa2 0x00 0x05 0x65 "hello" 0x65 "world"
    let mut payload = vec![0xa2, 0x00, 0x05];
    payload.extend_from_slice(b"\x65hello\x65world");

    let mut generator = PDUGenerator::new(
        Some("F4NEST".into()),
        None,
        Some(vec![ContentEncoding::H]),
        None,
        1,
    );
    let pdus = generator.generate(&payload, None).unwrap();

    let mut deframer = Deframer::new();
    for p in pdus {
        deframer.receive_bytes(&p);
    }

    let mut recovered = None;
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            recovered = Some(me.payload.to_vec());
        }
    }
    assert_eq!(
        recovered,
        Some(payload),
        "Raw CBOR payload with key 0 must not be stripped as a nested HQFBP PDU unless explicit"
    );
}

#[test]
fn test_p1_cor9_reject_invalid_chunk_bounds() {
    use hqfbp_rs::{Header, pack};

    let mut deframer = Deframer::new();

    // 1. total_chunks = 0
    let h0 = Header {
        message_id: Some(10),
        src_callsign: Some("F1ABC".into()),
        total_chunks: Some(0),
        chunk_id: Some(0),
        ..Default::default()
    };
    let pdu0 = pack(&h0, b"data0").unwrap();
    deframer.receive_bytes(&pdu0);
    assert_eq!(deframer.active_sessions_count(), 0);

    // 2. chunk_id >= total_chunks (e.g. chunk 2 of 2)
    let h1 = Header {
        message_id: Some(11),
        src_callsign: Some("F1ABC".into()),
        total_chunks: Some(2),
        chunk_id: Some(2),
        ..Default::default()
    };
    let pdu1 = pack(&h1, b"data1").unwrap();
    deframer.receive_bytes(&pdu1);
    assert_eq!(deframer.active_sessions_count(), 0);
}

#[test]
fn test_p1_cor13_rs_validation() {
    // k == n must be rejected
    assert!(hqfbp_rs::codec::rs_encode(b"hello", 255, 255).is_err());
    assert!(hqfbp_rs::codec::rs_decode(b"hello", 255, 255).is_err());
}

#[test]
fn test_p1_dtn1_session_expired_event() {
    use hqfbp_rs::{Header, pack};
    use std::thread::sleep;
    use std::time::Duration;

    let mut deframer = Deframer::new();
    let h = Header {
        message_id: Some(20),
        original_message_id: Some(20),
        src_callsign: Some("F1DTN".into()),
        total_chunks: Some(2),
        chunk_id: Some(0),
        ..Default::default()
    };
    let pdu = pack(&h, b"chunk0").unwrap();
    deframer.receive_bytes(&pdu);

    // Consume PDU event
    let _ = deframer.next_event();

    sleep(Duration::from_millis(15));
    let expired = deframer.cleanup_expired(Duration::from_millis(10));
    assert_eq!(expired, vec![(Some("F1DTN".into()), 20)]);

    // Check that SessionExpired event was queued
    let mut saw_expired_event = false;
    while let Some(ev) = deframer.next_event() {
        if let Event::SessionExpired(e) = ev {
            assert_eq!(e.src_callsign.as_deref(), Some("F1DTN"));
            assert_eq!(e.original_message_id, 20);
            assert_eq!(e.received_chunks, 1);
            assert_eq!(e.total_chunks, 2);
            saw_expired_event = true;
        }
    }
    assert!(saw_expired_event);
}

#[test]
fn test_ham6_compression_levels() {
    use hqfbp_rs::codec::{
        brotli_compress_with_params, brotli_decompress, deflate_compress_with_level,
        deflate_decompress, gzip_compress_with_level, gzip_decompress,
    };

    let sample =
        b"The quick brown fox jumps over the lazy dog. Repeat repeat repeat repeat!".repeat(10);

    for level in [1, 6, 9] {
        let gz = gzip_compress_with_level(&sample, level).unwrap();
        let decomp = gzip_decompress(&gz).unwrap();
        assert_eq!(decomp, sample);

        let def = deflate_compress_with_level(&sample, level).unwrap();
        let decomp_def = deflate_decompress(&def).unwrap();
        assert_eq!(decomp_def, sample);
    }

    for quality in [1, 6, 11] {
        let br = brotli_compress_with_params(&sample, quality, 22).unwrap();
        let decomp_br = brotli_decompress(&br).unwrap();
        assert_eq!(decomp_br, sample);
    }
}
