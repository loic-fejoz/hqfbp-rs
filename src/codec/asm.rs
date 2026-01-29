use crate::codec::{Encoding, EncodingContext};
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

impl Encoding for Asm {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            let mut d = self.sync_word.clone();
            d.extend_from_slice(&chunk);
            res.push(Bytes::from(d));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        for data in chunks {
            if data.starts_with(&self.sync_word) {
                res.push(data.slice(self.sync_word.len()..));
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
