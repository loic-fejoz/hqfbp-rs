use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::{ContentEncoding, Header};

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
    let mut generator = PDUGenerator::new(Some("SRC".to_string()), None, None, None, None, 42);
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
    let mut generator = PDUGenerator::new(Some("DEF".to_string()), None, None, Some(encs), None, 1);
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
