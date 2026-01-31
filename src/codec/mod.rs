use crate::error::{CodecError, Result};
use crate::{ContentEncoding, MediaType};
use bytes::Bytes;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub mod asm;
pub mod ax25;
pub mod brotli;
pub mod chunk;
pub mod conv;
pub mod crc16;
pub mod crc32;
pub mod golay;
pub mod gzip;
pub mod h;
pub mod identity;
pub mod lt;
pub mod lzma;
pub mod post_asm;
pub mod repeat;
pub mod rq;
pub mod rs;
pub mod scr;

pub type DecodedChunks<'a> = Vec<(Cow<'a, CodecContext>, Bytes)>;

#[derive(Debug, Clone)]
pub struct CodecContext {
    pub src_callsign: Option<String>,
    pub dst_callsign: Option<String>,
    pub next_msg_id: u32,
    pub original_message_id: Option<u32>,
    pub last_min_header_size: usize,
    pub last_max_header_size: usize,
    pub last_total_header_size: usize,
    pub file_size: Option<u64>,
    pub payload_size: Option<u64>,
    pub media_type: Option<MediaType>,
    pub encodings: Vec<ContentEncoding>,
    pub announcement_mode: bool,
    pub current_index: usize,
}

impl Default for CodecContext {
    fn default() -> Self {
        Self {
            src_callsign: None,
            dst_callsign: None,
            next_msg_id: 0,
            original_message_id: None,
            last_min_header_size: usize::MAX,
            last_max_header_size: 0,
            last_total_header_size: 0,
            file_size: None,
            payload_size: None,
            media_type: None,
            encodings: Vec::new(),
            announcement_mode: false,
            current_index: 0,
        }
    }
}

impl From<&crate::Header> for CodecContext {
    fn from(h: &crate::Header) -> Self {
        Self {
            src_callsign: h.src_callsign.clone(),
            dst_callsign: h.dst_callsign.clone(),
            next_msg_id: h.message_id.unwrap_or(0), // Or logical logic
            original_message_id: h.original_message_id,
            // Header doesn't have min/max header size, keep defaults or track?
            last_min_header_size: usize::MAX,
            last_max_header_size: 0,
            last_total_header_size: 0,
            file_size: h.file_size,
            payload_size: None, // Do NOT propagate payload_size from header (chunk size). PostAsm can set it.
            media_type: h.media_type(),
            encodings: h
                .content_encoding
                .as_ref()
                .map(|l| l.0.clone())
                .unwrap_or_default(),
            announcement_mode: false, // Infer from media type?
            current_index: h.chunk_id.unwrap_or(0) as usize,
        }
    }
}

pub trait Codec: Send + Sync {
    fn encode(&self, data: Vec<Bytes>, ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError>;
    fn try_decode<'a>(
        &self,
        chunks: DecodedChunks<'a>,
    ) -> Result<(DecodedChunks<'a>, f32), CodecError>;

    fn decode(&self, chunks: Vec<Bytes>) -> Result<Vec<Bytes>, CodecError> {
        let ctx = CodecContext::default();
        let input = chunks
            .into_iter()
            .map(|b| (Cow::Owned(ctx.clone()), b))
            .collect();
        self.try_decode(input)
            .map(|(res, _)| res.into_iter().map(|(_, b)| b).collect())
    }
    fn is_chunking(&self) -> bool {
        false
    }
    fn is_header(&self) -> bool {
        false
    }
    fn unpack_header(&self, _data: Bytes) -> Result<(crate::Header, Bytes), CodecError> {
        Err(CodecError::FecFailure("Not a header encoding".to_string()))
    }
}

pub struct CodecFactory {
    cache: Mutex<HashMap<ContentEncoding, Arc<dyn Codec>>>,
}

impl CodecFactory {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn get_encoding(&self, enc: &ContentEncoding) -> Arc<dyn Codec> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.get(enc) {
            return Arc::clone(cached);
        }

        let encoder: Arc<dyn Codec> = match enc {
            ContentEncoding::H => Arc::new(h::H::new()),
            ContentEncoding::Identity => Arc::new(identity::Identity::new()),
            ContentEncoding::Gzip => Arc::new(gzip::Gzip::new()),
            ContentEncoding::Brotli => Arc::new(brotli::Brotli::new()),
            ContentEncoding::Lzma => Arc::new(lzma::Lzma::new()),
            ContentEncoding::Crc16 => Arc::new(crc16::Crc16::new()),
            ContentEncoding::Crc32 => Arc::new(crc32::Crc32::new()),
            ContentEncoding::ReedSolomon(n, k) => Arc::new(rs::ReedSolomon::new(*n, *k)),
            ContentEncoding::RaptorQ(len, mtu, rep) => Arc::new(rq::RaptorQ::new(*len, *mtu, *rep)),
            ContentEncoding::RaptorQDynamic(mtu, rep) => {
                Arc::new(rq::RaptorQDynamic::new(*mtu, *rep))
            }
            ContentEncoding::RaptorQDynamicPercent(mtu, p) => {
                Arc::new(rq::RaptorQDynamicPercent::new(*mtu, *p))
            }
            ContentEncoding::LT(len, mtu, rep) => Arc::new(lt::LT::new(*len, *mtu, *rep)),
            ContentEncoding::LTDynamic(mtu, rep) => Arc::new(lt::LTDynamic::new(*mtu, *rep)),
            ContentEncoding::Conv(k, r) => Arc::new(conv::Conv::new(*k, r.clone())),
            ContentEncoding::Golay(n, k) => Arc::new(golay::Golay::new(*n, *k)),
            ContentEncoding::Scrambler(p, s) => Arc::new(scr::Scrambler::new(*p, *s)),
            ContentEncoding::Asm(w) => Arc::new(asm::Asm::new(w.clone())),
            ContentEncoding::PostAsm(w) => Arc::new(post_asm::PostAsm::new(w.clone())),
            ContentEncoding::Chunk(s) => Arc::new(chunk::Chunk::new(*s)),
            ContentEncoding::Repeat(n) => Arc::new(repeat::Repeat::new(*n)),
            ContentEncoding::Ax25 => Arc::new(ax25::Ax25::new()),
            _ => Arc::new(identity::Identity::new()), // Fallback
        };

        cache.insert(enc.clone(), Arc::clone(&encoder));
        encoder
    }
}

impl Default for CodecFactory {
    fn default() -> Self {
        Self::new()
    }
}
