use hqfbp_rs::{ContentEncoding, EncodingList, Header, get_coap_id, pack, unpack};

#[test]
fn test_simple_pack_unpack() {
    let src_callsign = "FOSM-1".to_string();
    let content = b"Autour de la terre, je pense aux \xc3\xa9l\xc3\xa8ves scrutant l'horizon.";
    let header = Header {
        message_id: Some(1),
        src_callsign: Some(src_callsign.clone()),
        ..Default::default()
    };

    let pdu = pack(&header, content).expect("Pack failed");

    let (decoded_header, decoded_payload) = unpack(pdu).expect("Unpack failed");

    assert_eq!(decoded_header.message_id, Some(1));
    assert_eq!(decoded_header.src_callsign, Some(src_callsign));
    assert_eq!(decoded_payload.as_ref(), content);
}

#[test]
fn test_mandatory_msg_id() {
    let header = Header {
        src_callsign: Some("N0CALL".to_string()),
        ..Default::default()
    };
    let res = pack(&header, b"data");
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(matches!(
        err,
        hqfbp_rs::HqfbpError::Protocol(hqfbp_rs::ProtocolError::MissingField(field)) if field == "Message-Id"
    ));
}

#[test]
fn test_chunking_and_merging() {
    let lorem = b"Lorem ipsum dolor sit amet, consectetur adipiscing elit.";
    let mid_point = lorem.len() / 2;
    let chunk1_data = &lorem[..mid_point];
    let chunk2_data = &lorem[mid_point..];

    let h1 = Header {
        message_id: Some(1001),
        src_callsign: Some("FOSM-1".to_string()),
        original_message_id: Some(1001),
        chunk_id: Some(0),
        total_chunks: Some(2),
        file_size: Some(lorem.len() as u64),
        content_type: Some("text/plain".to_string()),
        ..Default::default()
    };

    let h2 = Header {
        message_id: Some(1002),
        original_message_id: Some(1001),
        chunk_id: Some(1),
        total_chunks: Some(2),
        repr_digest: Some(b"somehash".to_vec()),
        ..Default::default()
    };

    let pdu1 = pack(&h1, chunk1_data).expect("Pack 1 failed");
    let pdu2 = pack(&h2, chunk2_data).expect("Pack 2 failed");

    let (dec_h1, _) = unpack(pdu1).expect("Unpack 1 failed");
    let (dec_h2, _) = unpack(pdu2).expect("Unpack 2 failed");

    let mut merged = dec_h1.clone();
    merged.merge(&dec_h2).expect("Merge failed");

    // In Rust version, Content-Type "text/plain" is optimized to 0 and then omitted/not yet handled.
    // Wait, let's check what happened to "text/plain".
    // get_coap_id("text/plain;charset=utf-8") is 0.
    // In Python tests, it used "text/plain" which might not be 0 exactly if it doesn't match the key.

    assert_eq!(merged.content_type, Some("text/plain".to_string()));
    assert_eq!(merged.repr_digest, Some(b"somehash".to_vec()));
    assert_eq!(merged.file_size, Some(lorem.len() as u64));

    // Check fields are consistent
    let h3 = Header {
        src_callsign: Some("DIFFERENT".to_string()),
        ..Default::default()
    };
    assert!(merged.merge(&h3).is_err());
}

#[test]
fn test_content_encoding_optimization() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    let content = b"Compressed data";
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(content).unwrap();
    let compressed = encoder.finish().unwrap();

    let header = Header {
        message_id: Some(1),
        content_encoding: Some(EncodingList(vec![ContentEncoding::Gzip])),
        ..Default::default()
    };

    let pdu = pack(&header, &compressed).expect("Pack failed");
    let (dec_h, dec_p) = unpack(pdu).expect("Unpack failed");

    // ContentEncoding::Single("gzip") should be optimized to ContentEncoding::Integer(1) during CBOR encoding
    // When decoded, it should become ContentEncoding::Integer(1) or potentially Single("gzip") if decoded correctly.
    // My Decode implementation for ContentEncoding maps Integers back to Strings in Multiple, but what about Single?
    // Let's check lib.rs Decode for ContentEncoding.

    if let Some(el) = dec_h.content_encoding {
        assert_eq!(el.0, vec![ContentEncoding::Gzip]);
    } else {
        panic!("Missing Content-Encoding");
    }

    assert_eq!(dec_p.as_ref(), compressed);
}

#[test]
fn test_coap_id_mapping() {
    assert_eq!(get_coap_id("image/png"), Some(23));
    assert_eq!(get_coap_id("text/plain;charset=utf-8"), Some(0));
    assert_eq!(get_coap_id("unknown/type"), None);
}

#[test]
fn test_human_readable() {
    let header = Header {
        message_id: Some(1001),
        src_callsign: Some("FOSM-1".to_string()),
        content_format: Some(23), // image/png
        content_encoding: Some(EncodingList(vec![
            ContentEncoding::Gzip,
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 233),
        ])),
        file_size: Some(4032),
        ..Default::default()
    };

    let readable = header.into_human_readable();

    assert_eq!(readable.get("Message-Id").unwrap().as_u64(), Some(1001));
    assert_eq!(
        readable.get("Src-Callsign").unwrap().as_str(),
        Some("FOSM-1")
    );
    assert_eq!(
        readable.get("Content-Type").unwrap().as_str(),
        Some("image/png")
    );

    let ce = readable
        .get("Content-Encoding")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(ce[0].as_str(), Some("gzip"));
    assert_eq!(ce[1].as_str(), Some("h"));
    assert_eq!(ce[2].as_str(), Some("rs(255,233)"));
}

#[test]
fn test_pack_optimization() {
    // 1. Content-Type string to CoAP ID (png -> 23)
    let p1 = pack(
        &Header {
            message_id: Some(1),
            content_type: Some("image/png".to_string()),
            ..Default::default()
        },
        b"pngdata",
    )
    .unwrap();
    let (h1, _) = unpack(p1).unwrap();
    assert_eq!(h1.content_format, Some(23));
    assert_eq!(h1.content_type, None);

    // 2. Content-Encoding strings to ints
    let p2 = pack(
        &Header {
            message_id: Some(2),
            content_encoding: Some(EncodingList(vec![
                ContentEncoding::Gzip,
                ContentEncoding::H,
            ])),
            ..Default::default()
        },
        b"gzdata",
    )
    .unwrap();
    let (h2, _) = unpack(p2).unwrap();
    let el = h2.content_encoding.unwrap();
    assert_eq!(el.0, vec![ContentEncoding::Gzip, ContentEncoding::H]);

    // 3. Omit default Content-Format 0
    let p3 = pack(
        &Header {
            message_id: Some(3),
            content_type: Some("text/plain;charset=utf-8".to_string()),
            ..Default::default()
        },
        b"text",
    )
    .unwrap();
    let (h3, _) = unpack(p3).unwrap();
    assert_eq!(h3.content_format, None);
    assert_eq!(h3.content_type, None);
}

#[test]
fn test_crc_helpers() {
    use hqfbp_rs::codec::crc16::crc16_ccitt;
    use hqfbp_rs::codec::crc32::crc32_std;
    let data = b"hello";

    let c16 = crc16_ccitt(data);
    assert_eq!(c16.len(), 2);

    let c32 = crc32_std(data);
    assert_eq!(c32.len(), 4);

    // Manual verification of CRC appending
    let mut d16 = data.to_vec();
    d16.extend_from_slice(&c16);
    assert_eq!(d16.len(), 5 + 2);

    let mut d32 = data.to_vec();
    d32.extend_from_slice(&c32);
    assert_eq!(d32.len(), 5 + 4);
}
