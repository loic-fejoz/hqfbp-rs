use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;
use crc::Crc;

pub struct Crc16;

impl Default for Crc16 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc16 {
    pub fn new() -> Self {
        Self
    }
}

pub fn crc16_ccitt(data: &[u8]) -> [u8; 2] {
    const CRC_16_PYTHON: crc::Algorithm<u16> = crc::Algorithm {
        width: 16,
        poly: 0x1021,
        init: 0xffff,
        refin: false,
        refout: false,
        xorout: 0x0000,
        check: 0x29b1,
        residue: 0x0000,
    };
    let crc = Crc::<u16>::new(&CRC_16_PYTHON);
    crc.checksum(data).to_be_bytes()
}

impl Codec for Crc16 {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let crc = crc16_ccitt(&chunk);
            let mut d = chunk.to_vec();
            d.extend_from_slice(&crc);
            res.push(Bytes::from(d));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        for data in chunks {
            let mut valid_len = None;
            if data.len() >= 2 {
                let payload = data.slice(..data.len() - 2);
                let expected = &data[data.len() - 2..];
                if crc16_ccitt(&payload) == expected {
                    valid_len = Some(payload.len());
                }
            }

            if valid_len.is_none() && data.len() > 2 {
                let mut test_len = data.len() - 1;
                let min_len = if data.len() > 300 {
                    data.len() - 256
                } else {
                    2
                };

                while test_len >= min_len {
                    let payload_check_len = test_len - 2;
                    let payload = data.slice(..payload_check_len);
                    let expected = &data[payload_check_len..test_len];
                    if crc16_ccitt(&payload) == expected {
                        valid_len = Some(payload.len());
                        break;
                    }
                    test_len -= 1;
                }
            }

            if let Some(vl) = valid_len {
                res.push(data.slice(..vl));
            } else {
                return Err(CodecError::CrcMismatch);
            }
        }
        Ok((res, 1000.0))
    }
}
