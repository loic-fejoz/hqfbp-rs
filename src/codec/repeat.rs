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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::CodecContext;
    use std::borrow::Cow;

    #[test]
    fn test_repeat_encode() {
        let codec = Repeat::new(3);
        let mut ctx = CodecContext::default();
        let data = vec![Bytes::from("abc")];
        let res = codec.encode(data, &mut ctx).unwrap();
        assert_eq!(res.len(), 3);
        assert_eq!(res[0], Bytes::from("abc"));
        assert_eq!(res[1], Bytes::from("abc"));
        assert_eq!(res[2], Bytes::from("abc"));
    }

    #[test]
    fn test_repeat_decode() {
        let codec = Repeat::new(3);
        let ctx = CodecContext::default();
        let data = vec![
            (Cow::Owned(ctx.clone()), Bytes::from("abc")),
            (Cow::Owned(ctx.clone()), Bytes::from("abc")),
            (Cow::Owned(ctx), Bytes::from("abc")),
        ];
        let (res, _) = codec.try_decode(data).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].1, Bytes::from("abc"));
    }
}
