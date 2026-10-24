use bytes::Bytes;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::kiss::{KissCommand, KissDecoder, encode_kiss_frame_with_port, parse_kiss_frame};
use hqfbp_rs::{ContentEncoding, EncodingList, Header, pack, unpack};
use proptest::prelude::*;

proptest! {
    #[test]
    fn prop_kiss_framing_roundtrip(
        payload in proptest::collection::vec(any::<u8>(), 0..2048),
        port in 0u8..16u8
    ) {
        let encoded = encode_kiss_frame_with_port(&payload, port);
        let mut decoder = KissDecoder::new();
        let mut frames = Vec::new();

        for byte in encoded {
            if let Some(frame) = decoder.process_byte(byte) {
                frames.push(frame);
            }
        }

        prop_assert_eq!(frames.len(), 1);
        let parsed = parse_kiss_frame(&frames[0]).expect("Valid KISS frame");
        prop_assert_eq!(parsed.port, port);
        prop_assert_eq!(parsed.command, KissCommand::DataFrame);
        prop_assert_eq!(parsed.data, payload);
    }

    #[test]
    fn prop_pack_unpack_roundtrip(
        msg_id in any::<u32>(),
        callsign in "[A-Z0-9]{3,7}",
        file_size in any::<Option<u64>>(),
        chunk_id in any::<Option<u32>>(),
        total_chunks in any::<Option<u32>>(),
        payload in proptest::collection::vec(any::<u8>(), 0..1024)
    ) {
        let header = Header {
            message_id: Some(msg_id),
            src_callsign: Some(callsign.clone()),
            dst_callsign: None,
            content_format: None,
            content_type: None,
            content_encoding: Some(EncodingList(vec![ContentEncoding::H])),
            repr_digest: None,
            content_digest: None,
            file_size,
            chunk_id,
            original_message_id: if chunk_id.is_some() { Some(msg_id) } else { None },
            total_chunks,
            payload_size: Some(payload.len() as u64),
        };

        if let Ok(packed) = pack(&header, &payload) {
            let (unpacked_header, unpacked_payload) = unpack(packed).expect("Unpack should succeed");
            prop_assert_eq!(unpacked_header.message_id, Some(msg_id));
            prop_assert_eq!(unpacked_header.src_callsign, Some(callsign));
            prop_assert_eq!(unpacked_header.file_size, file_size);
            prop_assert_eq!(unpacked_header.chunk_id, chunk_id);
            prop_assert_eq!(unpacked_header.total_chunks, total_chunks);
            prop_assert_eq!(unpacked_payload.as_ref(), payload.as_slice());
        }
    }

    #[test]
    fn prop_generator_deframer_roundtrip(
        data in proptest::collection::vec(any::<u8>(), 1..1000),
        use_chunking in any::<bool>(),
        use_crc in any::<bool>()
    ) {
        let mut encs = Vec::new();
        if use_chunking {
            encs.push(ContentEncoding::Chunk(128));
        }
        encs.push(ContentEncoding::H);
        if use_crc {
            encs.push(ContentEncoding::Crc32);
        }

        let mut generator = PDUGenerator::new(
            Some("PROP1".to_string()),
            None,
            Some(encs),
            None,
            100,
        );

        let pdus = generator.generate(&data, None).expect("Generate should succeed");
        prop_assert!(!pdus.is_empty());

        let mut deframer = Deframer::new();
        for pdu in pdus {
            deframer.receive_bytes(&pdu);
        }

        let mut received = None;
        while let Some(ev) = deframer.next_event() {
            if let Event::Message(me) = ev {
                received = Some(me.payload);
            }
        }

        prop_assert_eq!(received, Some(Bytes::copy_from_slice(&data)));
    }
}
