use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct PostAsm {
    sync_word: Vec<u8>,
}

impl PostAsm {
    pub fn new(sync_word: Vec<u8>) -> Self {
        Self { sync_word }
    }
}

impl Codec for PostAsm {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let mut d = chunk.to_vec();
            d.extend_from_slice(&self.sync_word);
            res.push(Bytes::from(d));
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        for (mut ctx, data) in chunks {
            // Reverse search for the sync word
            let found_pos = data
                .windows(self.sync_word.len())
                .rposition(|window| window == self.sync_word);

            if let Some(pos) = found_pos {
                // Determine the end of the payload (start of sync word)
                // We truncate everything after 'pos' including sync word and potential garbage
                let payload = data.slice(0..pos);

                // Update context
                ctx.to_mut().payload_size = Some(payload.len() as u64);

                res.push((ctx, payload));
            } else {
                return Err(CodecError::FecFailure(format!(
                    "Post-ASM sync word mismatch: expected {}",
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
    fn test_post_asm_decode() {
        let sync = vec![0xCA, 0xFE];
        let codec = PostAsm::new(sync.clone());
        let ctx = CodecContext::default();

        let payload = vec![1, 2, 3, 4];
        let mut data = payload.clone();
        data.extend_from_slice(&sync);
        data.push(0x99);

        let decode_input = vec![(Cow::Owned(ctx.clone()), Bytes::from(data))];
        let (decoded, _) = codec.try_decode(decode_input).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].1, Bytes::from(payload));

        assert_eq!(decoded[0].0.payload_size, Some(4));
    }
}
