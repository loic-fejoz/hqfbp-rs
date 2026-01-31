pub mod error;

pub use crate::error::{CodecError, HqfbpError, ProtocolError, Result};

use bytes::Bytes;
use minicbor::{Decode, Decoder, Encode, Encoder};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

pub mod codec;
pub mod deframer;
pub mod generator;
pub mod random_encoding;
pub mod random_sensible_encoding;

#[derive(Debug, Clone, PartialEq)]
pub enum MediaType {
    Format(u16),
    Type(String),
}

impl MediaType {
    pub fn to_mime(&self) -> String {
        match self {
            MediaType::Format(id) => rev_coap_content_formats()
                .get(id)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("application/x-coap-{id}")),
            MediaType::Type(s) => s.clone(),
        }
    }

    pub fn canonicalize(self) -> Self {
        match self {
            MediaType::Type(s) => {
                if let Some(id) = get_coap_id(&s) {
                    MediaType::Format(id)
                } else {
                    MediaType::Type(s)
                }
            }
            other => other,
        }
    }
}

impl std::fmt::Display for MediaType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_mime())
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Header {
    pub message_id: Option<u32>,
    pub src_callsign: Option<String>,
    pub dst_callsign: Option<String>,
    pub content_format: Option<u16>,
    pub content_type: Option<String>,
    pub content_encoding: Option<EncodingList>,
    pub repr_digest: Option<Vec<u8>>,
    pub content_digest: Option<Vec<u8>>,
    pub file_size: Option<u64>,
    pub chunk_id: Option<u32>,
    pub original_message_id: Option<u32>,
    pub total_chunks: Option<u32>,
    pub payload_size: Option<u64>,
}

impl<'b, C> Decode<'b, C> for Header {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let mut h = Header::default();
        if let Some(len) = d.map()? {
            for _ in 0..len {
                let key = d.u32()?;
                match key {
                    0 => h.message_id = Some(d.u32()?),
                    1 => h.src_callsign = Some(d.str()?.to_string()),
                    2 => h.dst_callsign = Some(d.str()?.to_string()),
                    3 => h.content_format = Some(d.u16()?),
                    4 => h.content_type = Some(d.str()?.to_string()),
                    5 => h.content_encoding = Some(d.decode()?),
                    6 => h.repr_digest = Some(d.bytes()?.to_vec()),
                    7 => h.content_digest = Some(d.bytes()?.to_vec()),
                    8 => h.file_size = Some(d.u64()?),
                    9 => h.chunk_id = Some(d.u32()?),
                    10 => h.original_message_id = Some(d.u32()?),
                    11 => h.total_chunks = Some(d.u32()?),
                    12 => h.payload_size = Some(d.u64()?),
                    _ => {
                        d.skip()?;
                    }
                }
            }
        } else {
            while d.datatype()? != minicbor::data::Type::Break {
                let key = d.u32()?;
                match key {
                    0 => h.message_id = Some(d.u32()?),
                    1 => h.src_callsign = Some(d.str()?.to_string()),
                    2 => h.dst_callsign = Some(d.str()?.to_string()),
                    3 => h.content_format = Some(d.u16()?),
                    4 => h.content_type = Some(d.str()?.to_string()),
                    5 => h.content_encoding = Some(d.decode()?),
                    6 => h.repr_digest = Some(d.bytes()?.to_vec()),
                    7 => h.content_digest = Some(d.bytes()?.to_vec()),
                    8 => h.file_size = Some(d.u64()?),
                    9 => h.chunk_id = Some(d.u32()?),
                    10 => h.original_message_id = Some(d.u32()?),
                    11 => h.total_chunks = Some(d.u32()?),
                    12 => h.payload_size = Some(d.u64()?),
                    _ => {
                        d.skip()?;
                    }
                }
            }
            d.skip()?;
        }
        Ok(h)
    }
}

impl<C> Encode<C> for Header {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let mut fields = Vec::new();
        if self.message_id.is_some() {
            fields.push(0);
        }
        if self.src_callsign.is_some() {
            fields.push(1);
        }
        if self.dst_callsign.is_some() {
            fields.push(2);
        }
        if self.content_format.is_some() {
            fields.push(3);
        }
        if self.content_type.is_some() {
            fields.push(4);
        }
        if self.content_encoding.is_some() {
            fields.push(5);
        }
        if self.repr_digest.is_some() {
            fields.push(6);
        }
        if self.content_digest.is_some() {
            fields.push(7);
        }
        if self.file_size.is_some() {
            fields.push(8);
        }
        if self.chunk_id.is_some() {
            fields.push(9);
        }
        if self.original_message_id.is_some() {
            fields.push(10);
        }
        if self.total_chunks.is_some() {
            fields.push(11);
        }
        if self.payload_size.is_some() {
            fields.push(12);
        }

        e.map(fields.len() as u64)?;
        for key in fields {
            match key {
                0 => {
                    e.u8(0)?.u32(self.message_id.unwrap())?;
                }
                1 => {
                    e.u8(1)?.str(self.src_callsign.as_ref().unwrap())?;
                }
                2 => {
                    e.u8(2)?.str(self.dst_callsign.as_ref().unwrap())?;
                }
                3 => {
                    e.u8(3)?.u16(self.content_format.unwrap())?;
                }
                4 => {
                    e.u8(4)?.str(self.content_type.as_ref().unwrap())?;
                }
                5 => {
                    e.u8(5)?.encode(self.content_encoding.as_ref().unwrap())?;
                }
                6 => {
                    e.u8(6)?.bytes(self.repr_digest.as_ref().unwrap())?;
                }
                7 => {
                    e.u8(7)?.bytes(self.content_digest.as_ref().unwrap())?;
                }
                8 => {
                    e.u8(8)?.u64(self.file_size.unwrap())?;
                }
                9 => {
                    e.u8(9)?.u32(self.chunk_id.unwrap())?;
                }
                10 => {
                    e.u8(10)?.u32(self.original_message_id.unwrap())?;
                }
                11 => {
                    e.u8(11)?.u32(self.total_chunks.unwrap())?;
                }
                12 => {
                    e.u8(12)?.u64(self.payload_size.unwrap())?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Header {
    pub fn merge(&mut self, other: &Header) -> Result<()> {
        macro_rules! merge_field {
            ($field:ident, $consistent:expr) => {
                if let Some(val) = &other.$field {
                    if let Some(existing) = &self.$field {
                        if $consistent && existing != val {
                            return Err(ProtocolError::InconsistentField(
                                stringify!($field).to_string(),
                            )
                            .into());
                        }
                    } else {
                        self.$field = Some(val.clone());
                    }
                }
            };
        }

        merge_field!(src_callsign, true);
        merge_field!(dst_callsign, false);
        merge_field!(content_format, false);
        merge_field!(content_type, false);
        merge_field!(content_encoding, false);
        merge_field!(repr_digest, true);
        merge_field!(content_digest, false);
        merge_field!(file_size, true);
        merge_field!(payload_size, false);

        Ok(())
    }

    pub fn strip_chunking(&mut self) {
        self.message_id = None;
        self.chunk_id = None;
        self.original_message_id = None;
        self.total_chunks = None;
    }

    pub fn media_type(&self) -> Option<MediaType> {
        if let Some(f) = self.content_format {
            Some(MediaType::Format(f))
        } else {
            self.content_type
                .as_ref()
                .map(|s| MediaType::Type(s.clone()))
        }
    }

    pub fn set_media_type(&mut self, mt: Option<MediaType>) {
        if let Some(m) = mt {
            match m.canonicalize() {
                MediaType::Format(f) => {
                    self.content_format = Some(f);
                    self.content_type = None;
                }
                MediaType::Type(s) => {
                    self.content_format = None;
                    self.content_type = Some(s);
                }
            }
        } else {
            self.content_format = None;
            self.content_type = None;
        }
    }

    pub fn into_human_readable(self) -> HashMap<String, serde_json::Value> {
        let mut m = HashMap::new();
        if let Some(v) = &self.message_id {
            m.insert("Message-Id".to_string(), (*v).into());
        }
        if let Some(v) = &self.src_callsign {
            m.insert("Src-Callsign".to_string(), v.clone().into());
        }
        if let Some(v) = &self.dst_callsign {
            m.insert("Dst-Callsign".to_string(), v.clone().into());
        }

        if let Some(mt) = self.media_type() {
            m.insert("Content-Type".to_string(), mt.to_mime().into());
        }

        if let Some(ce) = self.content_encoding {
            let list: Vec<serde_json::Value> = ce.0.iter().map(|e| e.to_string().into()).collect();
            if list.len() == 1 {
                m.insert("Content-Encoding".to_string(), list[0].clone());
            } else {
                m.insert("Content-Encoding".to_string(), list.into());
            }
        }

        if let Some(v) = &self.repr_digest {
            m.insert("Repr-Digest".to_string(), hex::encode(v).into());
        }
        if let Some(v) = &self.content_digest {
            m.insert("Content-Digest".to_string(), hex::encode(v).into());
        }
        if let Some(v) = &self.file_size {
            m.insert("File-Size".to_string(), (*v).into());
        }
        if let Some(v) = &self.chunk_id {
            m.insert("Chunk-Id".to_string(), (*v).into());
        }
        if let Some(v) = &self.original_message_id {
            m.insert("Original-Message-Id".to_string(), (*v).into());
        }
        if let Some(v) = &self.total_chunks {
            m.insert("Total-Chunks".to_string(), (*v).into());
        }
        if let Some(v) = &self.payload_size {
            m.insert("Payload-Size".to_string(), (*v).into());
        }

        m
    }
}

fn get_rs_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rs\((\d+),\s*(\d+)\)").unwrap())
}
fn get_rq_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rq\((\d+),\s*(\d+),\s*(\d+)\)").unwrap())
}
fn get_rq_dyn_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rq\(dlen,\s*(\d+),\s*(\d+)\)").unwrap())
}
fn get_rq_dyn_perc_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rq\(dlen,\s*(\d+),\s*(\d+)%\)").unwrap())
}
fn get_lt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"lt\((\d+),\s*(\d+),\s*(\d+)\)").unwrap())
}
fn get_lt_dyn_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"lt\(dlen,\s*(\d+),\s*(\d+)\)").unwrap())
}
fn get_conv_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"conv\((\d+),\s*(\d+/\d+)\)").unwrap())
}
fn get_scr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"scr\((0x[0-9a-fA-F]+|\d+)(,\s*(0x[0-9a-fA-F]+|\d+))?\)").unwrap()
    })
}
fn get_golay_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"golay(\((\d+),\s*(\d+)\))?").unwrap())
}
fn get_chunk_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"chunk\((\d+)\)").unwrap())
}
fn get_repeat_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"repeat\((\d+)\)").unwrap())
}
fn get_asm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^asm\((0x[0-9a-fA-F]+|\d+)\)$").unwrap())
}
fn get_post_asm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^post_asm\((0x[0-9a-fA-F]+|\d+)\)$").unwrap())
}
fn get_ax25_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ax\.25$").unwrap())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContentEncoding {
    H,
    Identity,
    Gzip,
    Deflate,
    Brotli,
    Lzma,
    Crc16,
    Crc32,
    ReedSolomon(usize, usize),
    RaptorQ(usize, u16, u32),
    RaptorQDynamic(u16, u32),
    RaptorQDynamicPercent(u16, u8),
    LT(usize, u16, u32),
    LTDynamic(u16, u32),
    Conv(usize, String),
    Golay(usize, usize),
    Scrambler(u64, Option<u64>),
    Asm(Vec<u8>),
    PostAsm(Vec<u8>),
    Chunk(usize),
    Repeat(usize),
    Ax25,
    OtherString(String),
    OtherInteger(i8),
}

impl std::fmt::Display for ContentEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentEncoding::H => write!(f, "h"),
            ContentEncoding::Identity => write!(f, "identity"),
            ContentEncoding::Gzip => write!(f, "gzip"),
            ContentEncoding::Deflate => write!(f, "deflate"),
            ContentEncoding::Brotli => write!(f, "br"),
            ContentEncoding::Lzma => write!(f, "lzma"),
            ContentEncoding::Crc16 => write!(f, "crc16"),
            ContentEncoding::Crc32 => write!(f, "crc32"),
            ContentEncoding::ReedSolomon(n, k) => write!(f, "rs({n},{k})"),
            ContentEncoding::RaptorQ(len, mtu, rep) => write!(f, "rq({len},{mtu},{rep})"),
            ContentEncoding::RaptorQDynamic(mtu, rep) => write!(f, "rq(dlen,{mtu},{rep})"),
            ContentEncoding::RaptorQDynamicPercent(mtu, percent) => {
                write!(f, "rq(dlen,{mtu},{percent}%)")
            }
            ContentEncoding::LT(len, mtu, rep) => write!(f, "lt({len},{mtu},{rep})"),
            ContentEncoding::LTDynamic(mtu, rep) => write!(f, "lt(dlen,{mtu},{rep})"),
            ContentEncoding::Conv(k, r) => write!(f, "conv({k},{r})"),
            ContentEncoding::Golay(n, k) => {
                if *n == 24 && *k == 12 {
                    write!(f, "golay")
                } else {
                    write!(f, "golay({n},{k})")
                }
            }
            ContentEncoding::Scrambler(p, s) => {
                if let Some(seed) = s {
                    write!(f, "scr(0x{p:x}, 0x{seed:x})")
                } else {
                    write!(f, "scr(0x{p:x})")
                }
            }
            ContentEncoding::Asm(w) => write!(f, "asm(0x{})", hex::encode(w)),
            ContentEncoding::PostAsm(w) => write!(f, "post_asm(0x{})", hex::encode(w)),
            ContentEncoding::Chunk(s) => write!(f, "chunk({s})"),
            ContentEncoding::Repeat(n) => write!(f, "repeat({n})"),
            ContentEncoding::Ax25 => write!(f, "ax.25"),
            ContentEncoding::OtherString(s) => write!(f, "{s}"),
            ContentEncoding::OtherInteger(i) => write!(f, "{i}"),
        }
    }
}

impl TryFrom<&str> for ContentEncoding {
    type Error = HqfbpError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s == "h" || s == "-1" {
            Ok(ContentEncoding::H)
        } else if s == "identity" || s == "0" {
            Ok(ContentEncoding::Identity)
        } else if s == "gzip" || s == "1" {
            Ok(ContentEncoding::Gzip)
        } else if s == "deflate" || s == "2" {
            Ok(ContentEncoding::Deflate)
        } else if s == "br" || s == "3" {
            Ok(ContentEncoding::Brotli)
        } else if s == "lzma" || s == "4" {
            Ok(ContentEncoding::Lzma)
        } else if s == "crc16" || s == "5" {
            Ok(ContentEncoding::Crc16)
        } else if s == "crc32" || s == "6" {
            Ok(ContentEncoding::Crc32)
        } else if s == "ax.25" || s == "41" || get_ax25_re().is_match(s) {
            Ok(ContentEncoding::Ax25)
        } else if let Some(m) = get_rs_re().captures(s) {
            Ok(ContentEncoding::ReedSolomon(m[1].parse()?, m[2].parse()?))
        } else if let Some(m) = get_rq_re().captures(s) {
            Ok(ContentEncoding::RaptorQ(
                m[1].parse()?,
                m[2].parse()?,
                m[3].parse()?,
            ))
        } else if let Some(m) = get_rq_dyn_re().captures(s) {
            Ok(ContentEncoding::RaptorQDynamic(
                m[1].parse()?,
                m[2].parse()?,
            ))
        } else if let Some(m) = get_rq_dyn_perc_re().captures(s) {
            Ok(ContentEncoding::RaptorQDynamicPercent(
                m[1].parse()?,
                m[2].parse()?,
            ))
        } else if let Some(m) = get_lt_re().captures(s) {
            Ok(ContentEncoding::LT(
                m[1].parse()?,
                m[2].parse()?,
                m[3].parse()?,
            ))
        } else if let Some(m) = get_lt_dyn_re().captures(s) {
            Ok(ContentEncoding::LTDynamic(m[1].parse()?, m[2].parse()?))
        } else if let Some(m) = get_conv_re().captures(s) {
            Ok(ContentEncoding::Conv(m[1].parse()?, m[2].to_string()))
        } else if get_golay_re().captures(s).is_some() {
            let m = get_golay_re().captures(s).unwrap();
            if let (Some(n), Some(k)) = (m.get(2), m.get(3)) {
                Ok(ContentEncoding::Golay(
                    n.as_str().parse()?,
                    k.as_str().parse()?,
                ))
            } else {
                Ok(ContentEncoding::Golay(24, 12))
            }
        } else if let Some(m) = get_scr_re().captures(s) {
            let parse_val = |val: &str| -> Result<u64> {
                if let Some(stripped) = val.strip_prefix("0x") {
                    u64::from_str_radix(stripped, 16).map_err(|e| HqfbpError::Parse(e.to_string()))
                } else {
                    val.parse()
                        .map_err(|e: std::num::ParseIntError| HqfbpError::Parse(e.to_string()))
                }
            };
            let p = parse_val(&m[1])?;
            let seed = if let Some(sm) = m.get(3) {
                Some(parse_val(sm.as_str())?)
            } else {
                None
            };
            Ok(ContentEncoding::Scrambler(p, seed))
        } else if let Some(m) = get_chunk_re().captures(s) {
            Ok(ContentEncoding::Chunk(m[1].parse()?))
        } else if let Some(m) = get_repeat_re().captures(s) {
            Ok(ContentEncoding::Repeat(m[1].parse()?))
        } else if let Some(m) = get_asm_re().captures(s) {
            let val = &m[1];
            let bytes = if let Some(hex_str) = val.strip_prefix("0x") {
                hex::decode(hex_str).map_err(|e| HqfbpError::Parse(e.to_string()))?
            } else {
                let n: u64 = val
                    .parse()
                    .map_err(|e: std::num::ParseIntError| HqfbpError::Parse(e.to_string()))?;
                if n == 0 {
                    vec![0]
                } else {
                    let mut b = n.to_be_bytes().to_vec();
                    // Remove leading zeros
                    let leading = b.iter().position(|&x| x != 0).unwrap_or(b.len() - 1);
                    b.drain(0..leading);
                    b
                }
            };
            Ok(ContentEncoding::Asm(bytes))
        } else if let Some(m) = get_post_asm_re().captures(s) {
            let val = &m[1];
            let bytes = if let Some(hex_str) = val.strip_prefix("0x") {
                hex::decode(hex_str).map_err(|e| HqfbpError::Parse(e.to_string()))?
            } else {
                let n: u64 = val
                    .parse()
                    .map_err(|e: std::num::ParseIntError| HqfbpError::Parse(e.to_string()))?;
                if n == 0 {
                    vec![0]
                } else {
                    let mut b = n.to_be_bytes().to_vec();
                    // Remove leading zeros
                    let leading = b.iter().position(|&x| x != 0).unwrap_or(b.len() - 1);
                    b.drain(0..leading);
                    b
                }
            };
            Ok(ContentEncoding::PostAsm(bytes))
        } else {
            Ok(ContentEncoding::OtherString(s.to_string()))
        }
    }
}

impl TryFrom<i8> for ContentEncoding {
    type Error = HqfbpError;
    fn try_from(i: i8) -> Result<Self, Self::Error> {
        match i {
            -1 => Ok(ContentEncoding::H),
            0 => Ok(ContentEncoding::Identity),
            1 => Ok(ContentEncoding::Gzip),
            2 => Ok(ContentEncoding::Deflate),
            3 => Ok(ContentEncoding::Brotli),
            4 => Ok(ContentEncoding::Lzma),
            5 => Ok(ContentEncoding::Crc16),
            6 => Ok(ContentEncoding::Crc32),
            54 => Ok(ContentEncoding::Asm(Vec::new())),
            56 => Ok(ContentEncoding::PostAsm(Vec::new())), // Should not really happen from i8 unless it's just the ID
            41 => Ok(ContentEncoding::Ax25),
            _ => Ok(ContentEncoding::OtherInteger(i)),
        }
    }
}

impl From<ContentEncoding> for i8 {
    fn from(val: ContentEncoding) -> Self {
        match val {
            ContentEncoding::H => -1,
            ContentEncoding::Identity => 0,
            ContentEncoding::Gzip => 1,
            ContentEncoding::Deflate => 2,
            ContentEncoding::Brotli => 3,
            ContentEncoding::Lzma => 4,
            ContentEncoding::Crc16 => 5,
            ContentEncoding::Crc32 => 6,
            ContentEncoding::Asm(ref w) if w.is_empty() => 54,
            ContentEncoding::PostAsm(ref w) if w.is_empty() => 56,
            ContentEncoding::Ax25 => 41,
            ContentEncoding::OtherInteger(i) => i,
            _ => 127, // Fallback for complex ones that don't have an ID
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EncodingList(pub Vec<ContentEncoding>);

impl<'b, C> Decode<'b, C> for EncodingList {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            minicbor::data::Type::U8
            | minicbor::data::Type::U16
            | minicbor::data::Type::U32
            | minicbor::data::Type::U64
            | minicbor::data::Type::I8
            | minicbor::data::Type::I16
            | minicbor::data::Type::I32
            | minicbor::data::Type::I64
            | minicbor::data::Type::Int => {
                let i = d.i8()?;
                Ok(EncodingList(vec![ContentEncoding::try_from(i).map_err(
                    |_| minicbor::decode::Error::message("invalid enum"),
                )?]))
            }
            minicbor::data::Type::String => {
                let s = d.str()?;
                Ok(EncodingList(vec![ContentEncoding::try_from(s).map_err(
                    |_| minicbor::decode::Error::message("invalid string"),
                )?]))
            }
            minicbor::data::Type::Array => {
                let mut v = Vec::new();
                let len = d.array()?.unwrap_or(0);
                for _ in 0..len {
                    match d.datatype()? {
                        minicbor::data::Type::U8
                        | minicbor::data::Type::U16
                        | minicbor::data::Type::U32
                        | minicbor::data::Type::U64
                        | minicbor::data::Type::I8
                        | minicbor::data::Type::I16
                        | minicbor::data::Type::I32
                        | minicbor::data::Type::I64
                        | minicbor::data::Type::Int => {
                            let i = d.i8()?;
                            v.push(
                                ContentEncoding::try_from(i).map_err(|_| {
                                    minicbor::decode::Error::message("invalid enum")
                                })?,
                            );
                        }
                        _ => {
                            let s = d.str()?;
                            v.push(
                                ContentEncoding::try_from(s).map_err(|_| {
                                    minicbor::decode::Error::message("invalid string")
                                })?,
                            );
                        }
                    }
                }
                Ok(EncodingList(v))
            }
            _ => Err(minicbor::decode::Error::type_mismatch(
                minicbor::data::Type::String,
            )),
        }
    }
}

impl<C> Encode<C> for EncodingList {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        let filtered: Vec<_> = self
            .0
            .iter()
            .filter(|e| !matches!(e, ContentEncoding::Chunk(_)))
            .collect();
        if filtered.len() == 1 {
            match filtered[0] {
                ContentEncoding::OtherString(s) => {
                    e.str(s)?;
                }
                other => {
                    let i: i8 = other.clone().into();
                    if i != 127 {
                        e.i8(i)?;
                    } else {
                        e.str(&other.to_string())?;
                    }
                }
            }
        } else if filtered.is_empty() {
            // If all were chunks, it becomes identity? Or just omit.
            // Usually there's at least 'h'.
            e.i8(0)?; // identity
        } else {
            e.array(filtered.len() as u64)?;
            for item in filtered {
                let i: i8 = item.clone().into();
                if i != 127 {
                    e.i8(i)?;
                } else {
                    e.str(&item.to_string())?;
                }
            }
        }
        Ok(())
    }
}

pub fn get_coap_id(mimetype: &str) -> Option<u16> {
    coap_content_formats().get(mimetype).copied()
}

pub fn pack(header: &Header, payload: &[u8]) -> Result<Bytes> {
    let mut h = header.clone();

    // 1. Optimize Content-Type to Content-Format
    h.set_media_type(h.media_type());

    // 2. Omit default Content-Format 0
    if h.content_format == Some(0) {
        h.content_format = None;
    }

    // 3. Ensure Message-Id is present
    if h.message_id.is_none() {
        return Err(ProtocolError::MissingField("Message-Id".to_string()).into());
    }

    // 4. Update Payload-Size
    h.payload_size = Some(payload.len() as u64);

    let mut buf = Vec::new();
    let mut encoder = Encoder::new(&mut buf);
    encoder
        .encode(&h)
        .map_err(|e| ProtocolError::MalformedHeader(format!("Header encode failed: {e}")))?;
    let header_len = buf.len();
    buf.extend_from_slice(payload);
    log::debug!(
        "pack h_len={}, p_len={}, total={}",
        header_len,
        payload.len(),
        buf.len()
    );
    Ok(Bytes::from(buf))
}

pub fn unpack(data: Bytes) -> Result<(Header, Bytes)> {
    let mut decoder = Decoder::new(&data);
    let header: Header = match decoder.decode() {
        Ok(h) => h,
        Err(e) => {
            return Err(
                ProtocolError::MalformedHeader(format!("Header decode failed: {e}")).into(),
            );
        }
    };
    if header.message_id.is_none()
        && header.content_type.as_deref() != Some("application/vnd.hqfbp+cbor")
    {
        return Err(ProtocolError::MissingField("message_id or content_type".to_string()).into());
    }
    let pos = decoder.position();
    let payload = data.slice(pos..);

    Ok((header, payload))
}

pub fn hqfbp_cbor_keys() -> &'static HashMap<&'static str, u8> {
    static MAP: OnceLock<HashMap<&'static str, u8>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("Message-Id", 0);
        m.insert("Src-Callsign", 1);
        m.insert("Dst-Callsign", 2);
        m.insert("Content-Format", 3);
        m.insert("Content-Type", 4);
        m.insert("Content-Encoding", 5);
        m.insert("Repr-Digest", 6);
        m.insert("Content-Digest", 7);
        m.insert("File-Size", 8);
        m.insert("Chunk-Id", 9);
        m.insert("Original-Message-Id", 10);
        m.insert("Total-Chunks", 11);
        m.insert("Payload-Size", 12);
        m
    })
}

pub fn rev_keys() -> &'static HashMap<u8, &'static str> {
    static MAP: OnceLock<HashMap<u8, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (k, v) in hqfbp_cbor_keys().iter() {
            m.insert(*v, *k);
        }
        m
    })
}

pub fn coap_content_formats() -> &'static HashMap<&'static str, u16> {
    static MAP: OnceLock<HashMap<&'static str, u16>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("text/plain;charset=utf-8", 0);
        m.insert("application/link-format", 40);
        m.insert("application/xml", 41);
        m.insert("application/octet-stream", 42);
        m.insert("application/json", 50);
        m.insert("application/cbor", 60);
        m.insert("application/senml+json", 110);
        m.insert("application/senml-exi", 111);
        m.insert("application/senml+cbor", 112);
        m.insert("application/sensml+json", 113);
        m.insert("application/sensml-exi", 114);
        m.insert("application/sensml+cbor", 115);
        m.insert("image/gif", 21);
        m.insert("image/jpeg", 22);
        m.insert("image/png", 23);
        m.insert("image/tiff", 24);
        m.insert("image/svg+xml", 30);
        m.insert("application/cose-key", 101);
        m.insert("application/cose-key-set", 102);
        m.insert("application/or-tecap", 116);
        m
    })
}

pub fn rev_coap_content_formats() -> &'static HashMap<u16, &'static str> {
    static MAP: OnceLock<HashMap<u16, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (k, v) in coap_content_formats().iter() {
            m.insert(*v, *k);
        }
        m
    })
}

pub fn encoding_registry() -> &'static HashMap<i8, &'static str> {
    static MAP: OnceLock<HashMap<i8, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert(-1, "h");
        m.insert(0, "identity");
        m.insert(1, "gzip");
        m.insert(2, "deflate");
        m.insert(3, "br");
        m.insert(4, "lzma");
        m.insert(5, "crc16");
        m.insert(6, "crc32");
        m
    })
}

pub fn rev_encoding_registry() -> &'static HashMap<&'static str, i8> {
    static MAP: OnceLock<HashMap<&'static str, i8>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (k, v) in encoding_registry().iter() {
            m.insert(*v, *k);
        }
        m
    })
}

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
