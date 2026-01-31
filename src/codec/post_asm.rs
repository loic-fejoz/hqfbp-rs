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

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        for data in chunks {
            if data.ends_with(&self.sync_word) {
                res.push(data.slice(..data.len() - self.sync_word.len()));
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
