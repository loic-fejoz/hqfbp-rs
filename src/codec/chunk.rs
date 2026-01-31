use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct Chunk {
    size: usize,
}

impl Chunk {
    pub fn new(size: usize) -> Self {
        Self { size }
    }
}

impl Codec for Chunk {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut next_chunks = Vec::new();
        for chunk in data {
            let mut pos = 0;
            while pos < chunk.len() {
                let end = (pos + self.size).min(chunk.len());
                next_chunks.push(chunk.slice(pos..end));
                pos = end;
            }
        }
        Ok(next_chunks)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut joined = Vec::new();
        for b in &chunks {
            joined.extend_from_slice(b);
        }
        Ok((vec![Bytes::from(joined)], 1.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}
