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

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        if chunks.is_empty() {
            return Ok((Vec::new(), 1.0));
        }
        let ctx = chunks[0].0.clone();
        let mut joined = Vec::new();
        for (_, b) in chunks {
            joined.extend_from_slice(&b);
        }
        Ok((vec![(ctx, Bytes::from(joined))], 1.0))
    }

    fn is_chunking(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::CodecContext;
    use std::borrow::Cow;

    #[test]
    fn test_chunk_encode() {
        let codec = Chunk::new(2);
        let mut ctx = CodecContext::default();
        let data = vec![Bytes::from("hello")];
        let res = codec.encode(data, &mut ctx).unwrap();
        // "he", "ll", "o"
        assert_eq!(res.len(), 3);
        assert_eq!(res[0], Bytes::from("he"));
        assert_eq!(res[1], Bytes::from("ll"));
        assert_eq!(res[2], Bytes::from("o"));
    }

    #[test]
    fn test_chunk_decode() {
        let codec = Chunk::new(2);
        let ctx = CodecContext::default();
        let data = vec![
            (Cow::Owned(ctx.clone()), Bytes::from("he")),
            (Cow::Owned(ctx.clone()), Bytes::from("ll")),
            (Cow::Owned(ctx), Bytes::from("o")),
        ];
        let (res, _) = codec.try_decode(data).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, Bytes::from("hello"));
    }
}
