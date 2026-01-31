use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use std::io::{Read, Write};

pub struct Gzip;

impl Default for Gzip {
    fn default() -> Self {
        Self::new()
    }
}

impl Gzip {
    pub fn new() -> Self {
        Self
    }
}

pub fn gzip_compress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| CodecError::CompressionError(format!("Gzip write failed: {e}")))?;
    encoder
        .finish()
        .map_err(|e| CodecError::CompressionError(format!("Gzip finish failed: {e}")))
}

pub fn gzip_decompress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut decoder = GzDecoder::new(data);
    let mut res = Vec::new();
    decoder
        .read_to_end(&mut res)
        .map_err(|e| CodecError::CompressionError(format!("Gzip decompress failed: {e}")))?;
    Ok(res)
}

impl Codec for Gzip {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(gzip_compress(&chunk)?));
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        for (ctx, chunk) in chunks {
            res.push((ctx, Bytes::from(gzip_decompress(&chunk)?)));
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
    fn test_gzip_encode_decode() {
        let codec = Gzip::new();
        let mut ctx = CodecContext::default();
        let payload = b"Hello, world! This is a test string to compress.";
        let data = vec![Bytes::from(payload.as_slice())];

        // Encode
        let encoded = codec.encode(data.clone(), &mut ctx).unwrap();
        assert_eq!(encoded.len(), 1);
        assert_ne!(encoded[0].as_ref(), payload); // Should be different (compressed or wrapped)

        // Decode
        let decode_input = vec![(Cow::Owned(ctx.clone()), encoded[0].clone())];
        let (decoded, _) = codec.try_decode(decode_input).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].1, Bytes::from(payload.as_slice()));
    }
}
