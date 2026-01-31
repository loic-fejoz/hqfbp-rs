use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;
use std::io::{Read, Write};

pub struct Brotli;

impl Default for Brotli {
    fn default() -> Self {
        Self::new()
    }
}

impl Brotli {
    pub fn new() -> Self {
        Self
    }
}

pub fn brotli_compress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut res = Vec::new();
    let mut writer = brotli::CompressorWriter::new(&mut res, 4096, 6, 22);
    writer
        .write_all(data)
        .map_err(|e| CodecError::CompressionError(format!("Brotli write failed: {e}")))?;
    writer
        .flush()
        .map_err(|e| CodecError::CompressionError(format!("Brotli flush failed: {e}")))?;
    drop(writer);
    Ok(res)
}

pub fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut res = Vec::new();
    let mut reader = brotli::Decompressor::new(data, 4096);
    reader
        .read_to_end(&mut res)
        .map_err(|e| CodecError::CompressionError(format!("Brotli decompress failed: {e}")))?;
    Ok(res)
}

impl Codec for Brotli {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(brotli_compress(&chunk)?));
        }
        Ok(res)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        let mut res = Vec::new();
        for chunk in chunks {
            res.push(Bytes::from(brotli_decompress(&chunk)?));
        }
        Ok((res, 1.0))
    }
}
