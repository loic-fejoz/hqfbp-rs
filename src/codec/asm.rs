use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct Asm {
    sync_word: Vec<u8>,
}

impl Asm {
    pub fn new(sync_word: Vec<u8>) -> Self {
        Self { sync_word }
    }
}

impl Codec for Asm {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let mut d = self.sync_word.clone();
            d.extend_from_slice(&chunk);
            res.push(Bytes::from(d));
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        for (ctx, data) in chunks {
            if data.starts_with(&self.sync_word) {
                res.push((ctx, data.slice(self.sync_word.len()..)));
            } else {
                return Err(CodecError::FecFailure(format!(
                    "ASM sync word mismatch: expected {}",
                    hex::encode(&self.sync_word)
                )));
            }
        }
        Ok((res, 1000.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::CodecContext;
    use std::borrow::Cow;

    #[test]
    fn test_asm_encode_decode() {
        let sync = vec![0x1A, 0xCF, 0xFC, 0x1D];
        let codec = Asm::new(sync.clone());
        let mut ctx = CodecContext::default();
        ctx.current_index = 42; // arbitrary value to check context propagation

        let payload = vec![0x01, 0x02, 0x03];
        let data = vec![Bytes::from(payload.clone())];

        // Encode
        let encoded = codec.encode(data.clone(), &mut ctx).unwrap();
        assert_eq!(encoded.len(), 1);
        // Encoded should be sync + payload
        assert_eq!(encoded[0].len(), sync.len() + payload.len());
        assert_eq!(&encoded[0][..sync.len()], &sync[..]);
        assert_eq!(&encoded[0][sync.len()..], &payload[..]);

        // Decode
        let decode_input = vec![(Cow::Owned(ctx.clone()), encoded[0].clone())];
        let (decoded, _) = codec.try_decode(decode_input).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].1, Bytes::from(payload));
        // Verify context is propagated untouched
        assert_eq!(decoded[0].0.current_index, 42);
    }

    #[test]
    fn test_asm_decode_mismatch() {
        let sync = vec![0xCA, 0xFE];
        let codec = Asm::new(sync.clone());
        let ctx = CodecContext::default();

        let data = vec![Bytes::from(vec![0x00, 0x00])]; // Mismatch
        let decode_input = vec![(Cow::Owned(ctx), data[0].clone())];

        let res = codec.try_decode(decode_input);
        assert!(res.is_err());
    }
}
