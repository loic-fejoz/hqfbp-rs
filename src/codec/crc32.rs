use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;
use crc::{CRC_32_ISO_HDLC, Crc};

pub struct Crc32;

impl Crc32 {
    pub fn new() -> Self {
        Self
    }
}

pub fn crc32_std(data: &[u8]) -> [u8; 4] {
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    crc.checksum(data).to_be_bytes()
}

impl Encoding for Crc32 {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let crc = crc32_std(&chunk);
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
            if data.len() >= 4 {
                let payload = data.slice(..data.len() - 4);
                let expected = &data[data.len() - 4..];
                if crc32_std(&payload) == expected {
                    valid_len = Some(payload.len());
                }
            }

            if valid_len.is_none() && data.len() > 4 {
                let mut test_len = data.len() - 1;
                let min_len = if data.len() > 300 {
                    data.len() - 256
                } else {
                    4
                };

                while test_len >= min_len {
                    let payload_check_len = test_len - 4;
                    let payload = data.slice(..payload_check_len);
                    let expected = &data[payload_check_len..test_len];
                    if crc32_std(&payload) == expected {
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
