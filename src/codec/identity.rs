use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct Identity;

impl Default for Identity {
    fn default() -> Self {
        Self::new()
    }
}

impl Identity {
    pub fn new() -> Self {
        Self
    }
}

impl Codec for Identity {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        Ok(data)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        Ok((chunks, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::CodecContext;
    use std::borrow::Cow;

    #[test]
    fn test_identity_encode() {
        let codec = Identity::new();
        let mut ctx = CodecContext::default();
        let data = vec![Bytes::from("hello"), Bytes::from("world")];
        let res = codec.encode(data.clone(), &mut ctx).unwrap();
        assert_eq!(res, data);
    }

    #[test]
    fn test_identity_decode() {
        let codec = Identity::new();
        let ctx = CodecContext::default();
        let data = vec![
            (Cow::Owned(ctx.clone()), Bytes::from("hello")),
            (Cow::Owned(ctx.clone()), Bytes::from("world")),
        ];
        let (res, quality) = codec.try_decode(data.clone()).unwrap();
        assert_eq!(quality, 1.0);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].1, Bytes::from("hello"));
        assert_eq!(res[1].1, Bytes::from("world"));
        // Check context is untouched (using address as proxy for identity of context object or just fields)
        // Since we passed Owned, we got Owned back.
        // Let's verify a field.
        assert_eq!(res[0].0.src_callsign, ctx.src_callsign);
    }
}
