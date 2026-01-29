use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;
use std::io::Cursor;

pub struct Lzma;

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

impl Encoding for Lzma {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(lzma_compress(&chunk)?));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        for chunk in chunks {
            res.push(Bytes::from(lzma_decompress(&chunk)?));
        }
        Ok((res, 1.0))
    }
}
