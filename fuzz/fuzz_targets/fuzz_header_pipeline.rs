#![no_main]

use arbitrary::Arbitrary;
use hqfbp_rs::deframer::Deframer;
use hqfbp_rs::{ContentEncoding, EncodingList, Header, pack};
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
pub enum FuzzEncoding {
    H,
    Identity,
    Gzip,
    Deflate,
    Brotli,
    Crc16,
    Crc32,
    ReedSolomon { n: u8, k: u8 },
    RaptorQ { rq_len: u16, mtu: u8, repairs: u8 },
    LT { len: u16, mtu: u8, repairs: u8 },
    Repeat(u8),
    Scrambler(u32),
}

#[derive(Arbitrary, Debug)]
pub struct FuzzHeaderInput {
    pub message_id: Option<u32>,
    pub src_callsign: Option<String>,
    pub dst_callsign: Option<String>,
    pub file_size: Option<u64>,
    pub chunk_id: Option<u32>,
    pub original_message_id: Option<u32>,
    pub total_chunks: Option<u32>,
    pub encodings: Vec<FuzzEncoding>,
    pub payload: Vec<u8>,
}

fuzz_target!(|input: FuzzHeaderInput| {
    let encs: Vec<ContentEncoding> = input
        .encodings
        .into_iter()
        .take(6)
        .map(|fe| match fe {
            FuzzEncoding::H => ContentEncoding::H,
            FuzzEncoding::Identity => ContentEncoding::Identity,
            FuzzEncoding::Gzip => ContentEncoding::Gzip,
            FuzzEncoding::Deflate => ContentEncoding::Deflate,
            FuzzEncoding::Brotli => ContentEncoding::Brotli,
            FuzzEncoding::Crc16 => ContentEncoding::Crc16,
            FuzzEncoding::Crc32 => ContentEncoding::Crc32,
            FuzzEncoding::ReedSolomon { n, k } => {
                ContentEncoding::ReedSolomon(n as usize, k as usize)
            }
            FuzzEncoding::RaptorQ {
                rq_len,
                mtu,
                repairs,
            } => ContentEncoding::RaptorQ(rq_len as usize, mtu as u16, repairs as u32),
            FuzzEncoding::LT { len, mtu, repairs } => {
                ContentEncoding::LT(len as usize, mtu as u16, repairs as u32)
            }
            FuzzEncoding::Repeat(r) => ContentEncoding::Repeat((r % 5) as usize),
            FuzzEncoding::Scrambler(p) => ContentEncoding::Scrambler(p as u64),
        })
        .collect();

    let header = Header {
        message_id: input.message_id.or(Some(1)),
        src_callsign: input.src_callsign.map(|s| s.chars().take(12).collect()),
        dst_callsign: input.dst_callsign.map(|s| s.chars().take(12).collect()),
        content_encoding: Some(EncodingList(encs.clone())),
        file_size: input.file_size,
        chunk_id: input.chunk_id,
        original_message_id: input.original_message_id,
        total_chunks: input.total_chunks,
        payload_size: Some(input.payload.len() as u64),
        ..Default::default()
    };

    let payload = if input.payload.len() > 1024 {
        &input.payload[..1024]
    } else {
        &input.payload[..]
    };

    if let Ok(pdu) = pack(&header, payload) {
        let mut deframer = Deframer::new();
        deframer.receive_bytes(&pdu);
        while let Some(_ev) = deframer.next_event() {}
    }
});
