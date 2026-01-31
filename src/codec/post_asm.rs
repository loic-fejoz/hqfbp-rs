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
