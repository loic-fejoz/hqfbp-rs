use crate::ContentEncoding;
use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;
use raptorq::Encoder as RQEncoder;

pub struct RaptorQ {
    original_count: usize,
    mtu: u16,
    repair_count: u32,
}

impl RaptorQ {
    pub fn new(original_count: usize, mtu: u16, repair_count: u32) -> Self {
        Self {
            original_count,
            mtu,
            repair_count,
        }
    }
}

pub struct RaptorQDynamic {
    mtu: u16,
    repair_count: u32,
}

impl RaptorQDynamic {
    pub fn new(mtu: u16, repair_count: u32) -> Self {
        Self { mtu, repair_count }
    }
}

pub struct RaptorQDynamicPercent {
    mtu: u16,
    percent: u8,
}

impl RaptorQDynamicPercent {
    pub fn new(mtu: u16, percent: u8) -> Self {
        Self { mtu, percent }
    }
}

pub fn rq_encode(
    data: &[u8],
    original_count: usize,
    mtu: u16,
    repair_count: u32,
) -> Result<Vec<Bytes>, CodecError> {
    let mut padded_data = data.to_vec();
    padded_data.resize(original_count, 0);

    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let encoder = RQEncoder::new(&padded_data, oti);
    let packets = encoder.get_encoded_packets(repair_count);

    let res: Vec<Bytes> = packets
        .into_iter()
        .map(|p| Bytes::from(p.serialize()))
        .collect();
    Ok(res)
}

pub fn rq_decode(
    packets: Vec<Bytes>,
    original_count: usize,
    mtu: u16,
) -> Result<Vec<u8>, CodecError> {
    let oti = raptorq::ObjectTransmissionInformation::new(original_count as u64, mtu, 1, 1, 1);
    let mut decoder = raptorq::Decoder::new(oti);

    for packet_bytes in packets {
        if packet_bytes.len() < 4 {
            continue;
        }

        let expected_len = (mtu as usize) + 4;
        if packet_bytes.len() < expected_len {
            continue;
        }

        let p_ref = if packet_bytes.len() > expected_len {
            &packet_bytes[..expected_len]
        } else {
            packet_bytes.as_ref()
        };

        let packet = raptorq::EncodingPacket::deserialize(p_ref);
        if let Some(res) = decoder.decode(packet) {
            return Ok(res);
        }
    }
    Err(CodecError::InsufficientData(None))
}

impl Encoding for RaptorQ {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.extend(rq_encode(
                &chunk,
                self.original_count,
                self.mtu,
                self.repair_count,
            )?);
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let res = rq_decode(chunks, self.original_count, self.mtu)?;
        Ok((vec![Bytes::from(res)], 10.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}

impl Encoding for RaptorQDynamic {
    fn encode(
        &self,
        data: Vec<Bytes>,
        ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let rq_len = chunk.len();
            let resolved = ContentEncoding::RaptorQ(rq_len, self.mtu, self.repair_count);
            if ctx.current_index < ctx.encodings.len() {
                ctx.encodings[ctx.current_index] = resolved;
            }
            res.extend(rq_encode(&chunk, rq_len, self.mtu, self.repair_count)?);
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let total_len: usize = chunks.iter().map(|b| b.len()).sum();
        let res = rq_decode(chunks, total_len, self.mtu)?;
        Ok((vec![Bytes::from(res)], 10.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}

impl Encoding for RaptorQDynamicPercent {
    fn encode(
        &self,
        data: Vec<Bytes>,
        ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let rq_len = chunk.len();
            let repairs = 1.max(
                (rq_len as f32 * (self.percent as f32) / (100.0 * (self.mtu as f32))).ceil() as u32,
            );
            let resolved = ContentEncoding::RaptorQ(rq_len, self.mtu, repairs);
            if ctx.current_index < ctx.encodings.len() {
                ctx.encodings[ctx.current_index] = resolved;
            }
            res.extend(rq_encode(&chunk, rq_len, self.mtu, repairs)?);
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let total_len: usize = chunks.iter().map(|b| b.len()).sum();
        let res = rq_decode(chunks, total_len, self.mtu)?;
        Ok((vec![Bytes::from(res)], 10.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}
