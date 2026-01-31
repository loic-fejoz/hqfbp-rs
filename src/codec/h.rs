use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use crate::{EncodingList, Header, pack};
use bytes::Bytes;

pub struct H;

impl Default for H {
    fn default() -> Self {
        Self::new()
    }
}

impl H {
    pub fn new() -> Self {
        Self
    }
}

impl Codec for H {
    fn encode(&self, data: Vec<Bytes>, ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let total_chunks = data.len() as u32;
        let mut new_chunks = Vec::new();
        let data_orig_id = ctx.next_msg_id;

        let mut header_template = Header {
            file_size: ctx.file_size,
            src_callsign: ctx.src_callsign.clone(),
            dst_callsign: ctx.dst_callsign.clone(),
            ..Default::default()
        };
        header_template.set_media_type(ctx.media_type.clone());

        for (idx, chunk_data) in data.iter().enumerate() {
            let mut header = header_template.clone();

            let msg_id = if idx == 0 {
                let id = data_orig_id;
                if ctx.next_msg_id == id {
                    ctx.next_msg_id += 1;
                }
                id
            } else {
                let id = ctx.next_msg_id;
                ctx.next_msg_id += 1;
                id
            };

            if total_chunks > 1 {
                header.total_chunks = Some(total_chunks);
                header.chunk_id = Some(idx as u32);
                header.original_message_id = Some(data_orig_id);
            }
            header.message_id = Some(msg_id);

            if idx > 0 {
                header.set_media_type(None);
            }

            header.content_encoding = Some(EncodingList(ctx.encodings.clone()));
            header.payload_size = Some(chunk_data.len() as u64);

            let packed = pack(&header, chunk_data)
                .map_err(|e| CodecError::InvalidParameters(e.to_string()))?;
            let h_size = packed.len() - chunk_data.len();
            ctx.last_min_header_size = ctx.last_min_header_size.min(h_size);
            ctx.last_max_header_size = ctx.last_max_header_size.max(h_size);
            ctx.last_total_header_size += h_size;

            new_chunks.push(packed);
        }
        Ok(new_chunks)
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

    fn is_header(&self) -> bool {
        true
    }

    fn unpack_header(&self, data: Bytes) -> Result<(crate::Header, Bytes), CodecError> {
        crate::unpack(data).map_err(|e| CodecError::FecFailure(e.to_string()))
    }
}
