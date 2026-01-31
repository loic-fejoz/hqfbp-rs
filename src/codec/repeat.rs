use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct Repeat {
    count: usize,
}

impl Repeat {
    pub fn new(count: usize) -> Self {
        Self { count }
    }
}

impl Codec for Repeat {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut next_chunks = Vec::new();
        for chunk in data {
            for _ in 0..self.count {
                next_chunks.push(chunk.clone());
            }
        }
        Ok(next_chunks)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        if chunks.len() > 1 && self.count > 0 {
            let step = self.count;
            for i in (0..chunks.len()).step_by(step) {
                res.push(chunks[i].clone());
            }
        } else {
            res = chunks;
        }
        Ok((res, 1.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}
