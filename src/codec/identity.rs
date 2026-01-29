use crate::codec::{Encoding, EncodingContext};
use crate::error::CodecError;
use bytes::Bytes;

pub struct Identity;

impl Identity {
    pub fn new() -> Self {
        Self
    }
}

impl Encoding for Identity {
    fn encode(
        &self,
        data: Vec<Bytes>,
        _ctx: &mut EncodingContext,
    ) -> Result<Vec<Bytes>, CodecError> {
        Ok(data)
    }

    fn try_decode(&self, chunks: Vec<Bytes>) -> Result<(Vec<Bytes>, f32), CodecError> {
        Ok((chunks, 1.0))
    }
}
