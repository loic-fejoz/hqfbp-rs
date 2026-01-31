use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;
use std::io::Cursor;

pub struct Lzma;

impl Default for Lzma {
    fn default() -> Self {
        Self::new()
    }
}

impl Lzma {
    pub fn new() -> Self {
        Self
    }
}

pub fn lzma_compress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut res = Vec::new();
    lzma_rs::xz_compress(&mut Cursor::new(data), &mut res)
        .map_err(|e| CodecError::CompressionError(format!("XZ compress failed: {e}")))?;
    Ok(res)
}

pub fn lzma_decompress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut res = Vec::new();
    lzma_rs::xz_decompress(&mut Cursor::new(data), &mut res)
        .map_err(|e| CodecError::CompressionError(format!("XZ decompress failed: {e}")))?;
    Ok(res)
}

impl Codec for Lzma {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(lzma_compress(&chunk)?));
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        for (ctx, chunk) in chunks {
            res.push((ctx, Bytes::from(lzma_decompress(&chunk)?)));
        }
        Ok((res, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::CodecContext;
    use std::borrow::Cow;

    #[test]
    fn test_lzma_encode_decode() {
        let codec = Lzma::new();
        let mut ctx = CodecContext::default();
        let payload = b"LZMA/XZ compression test. Longer strings compress better.";
        let data = vec![Bytes::from(payload.as_slice())];

        let encoded = codec.encode(data.clone(), &mut ctx).unwrap();
        assert_eq!(encoded.len(), 1);

        let decode_input = vec![(Cow::Owned(ctx.clone()), encoded[0].clone())];
        let (decoded, _) = codec.try_decode(decode_input).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].1, Bytes::from(payload.as_slice()));
    }
}
