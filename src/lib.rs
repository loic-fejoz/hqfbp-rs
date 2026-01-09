use minicbor::{Encode, Decode, Decoder, Encoder};
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use lazy_static::lazy_static;

pub mod codec;
pub mod generator;
pub mod deframer;

#[derive(Debug, Default, Clone, Encode, Decode)]
#[cbor(map)]
pub struct Header {
    #[n(0)] pub message_id: Option<u32>,
    #[n(1)] pub src_callsign: Option<String>,
    #[n(2)] pub dst_callsign: Option<String>,
    #[n(3)] pub content_format: Option<u16>,
    #[n(4)] pub content_type: Option<String>,
    #[n(5)] pub content_encoding: Option<ContentEncoding>,
    #[n(6)] pub repr_digest: Option<Vec<u8>>,
    #[n(7)] pub content_digest: Option<Vec<u8>>,
    #[n(8)] pub file_size: Option<u64>,
    #[n(9)] pub chunk_id: Option<u32>,
    #[n(10)] pub original_message_id: Option<u32>,
    #[n(11)] pub total_chunks: Option<u32>,
    #[n(12)] pub payload_size: Option<u64>,
}

impl Header {
    pub fn merge(&mut self, other: &Header) -> Result<()> {
        macro_rules! merge_field {
            ($field:ident, $consistent:expr) => {
                if let Some(val) = &other.$field {
                    if let Some(existing) = &self.$field {
                        if $consistent && existing != val {
                            bail!(concat!("Inconsistent header field: ", stringify!($field)));
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

    pub fn into_human_readable(self) -> HashMap<String, serde_json::Value> {
        let mut m = HashMap::new();
        if let Some(v) = self.message_id { m.insert("Message-Id".to_string(), v.into()); }
        if let Some(v) = self.src_callsign { m.insert("Src-Callsign".to_string(), v.into()); }
        if let Some(v) = self.dst_callsign { m.insert("Dst-Callsign".to_string(), v.into()); }
        
        let mut final_ct = self.content_type.clone();
        if let Some(cf) = self.content_format {
            if let Some(ct) = REV_COAP_CONTENT_FORMATS.get(&cf) {
                final_ct = Some(ct.to_string());
            }
        }
        if let Some(ct) = final_ct { m.insert("Content-Type".to_string(), ct.into()); }

        if let Some(ce) = self.content_encoding {
            match ce {
                ContentEncoding::Single(s) => { m.insert("Content-Encoding".to_string(), s.into()); }
                ContentEncoding::Multiple(v) => { m.insert("Content-Encoding".to_string(), v.into()); }
                ContentEncoding::Integer(i) => {
                    let s = ENCODING_REGISTRY.get(&i).map(|&s| s.to_string()).unwrap_or_else(|| i.to_string());
                    m.insert("Content-Encoding".to_string(), s.into());
                }
            }
        }
        
        if let Some(v) = self.repr_digest { m.insert("Repr-Digest".to_string(), hex::encode(v).into()); }
        if let Some(v) = self.content_digest { m.insert("Content-Digest".to_string(), hex::encode(v).into()); }
        if let Some(v) = self.file_size { m.insert("File-Size".to_string(), v.into()); }
        if let Some(v) = self.chunk_id { m.insert("Chunk-Id".to_string(), v.into()); }
        if let Some(v) = self.original_message_id { m.insert("Original-Message-Id".to_string(), v.into()); }
        if let Some(v) = self.total_chunks { m.insert("Total-Chunks".to_string(), v.into()); }
        if let Some(v) = self.payload_size { m.insert("Payload-Size".to_string(), v.into()); }

        m
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentEncoding {
    Single(String),
    Multiple(Vec<String>),
    Integer(i8),
}

// Custom encoding/decoding for ContentEncoding as it can be int, string or array
impl<'b, C> Decode<'b, C> for ContentEncoding {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            minicbor::data::Type::U8 | minicbor::data::Type::U16 | minicbor::data::Type::U32 | minicbor::data::Type::U64 |
            minicbor::data::Type::I8 | minicbor::data::Type::I16 | minicbor::data::Type::I32 | minicbor::data::Type::I64 |
            minicbor::data::Type::Int => {
                Ok(ContentEncoding::Integer(d.i8()?))
            }
            minicbor::data::Type::String => {
                Ok(ContentEncoding::Single(d.str()?.to_string()))
            }
            minicbor::data::Type::Array => {
                let mut v = Vec::new();
                let len = d.array()?.unwrap_or(0);
                for _ in 0..len {
                    match d.datatype()? {
                        minicbor::data::Type::U8 | minicbor::data::Type::U16 | minicbor::data::Type::U32 | minicbor::data::Type::U64 |
                        minicbor::data::Type::I8 | minicbor::data::Type::I16 | minicbor::data::Type::I32 | minicbor::data::Type::I64 |
                        minicbor::data::Type::Int => {
                            let i = d.i8()?;
                            v.push(ENCODING_REGISTRY.get(&i).copied().unwrap_or("unknown").to_string());
                        }
                        _ => {
                            let s = d.str()?;
                            v.push(s.to_string());
                        }
                    }
                }
                Ok(ContentEncoding::Multiple(v))
            }
            _ => Err(minicbor::decode::Error::type_mismatch(minicbor::data::Type::String)),
        }
    }
}

impl<C> Encode<C> for ContentEncoding {
    fn encode<W: minicbor::encode::Write>(&self, e: &mut Encoder<W>, _ctx: &mut C) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            ContentEncoding::Integer(i) => { e.i8(*i)?; }
            ContentEncoding::Single(s) => {
                if let Some(&i) = REV_ENCODING_REGISTRY.get(s.as_str()) {
                    e.i8(i)?;
                } else {
                    e.str(s)?;
                }
            }
            ContentEncoding::Multiple(v) => {
                if v.len() == 1 {
                    let s = &v[0];
                    if let Some(&i) = REV_ENCODING_REGISTRY.get(s.as_str()) {
                        e.i8(i)?;
                    } else {
                        e.str(s)?;
                    }
                } else {
                    e.array(v.len() as u64)?;
                    for s in v {
                        if let Some(&i) = REV_ENCODING_REGISTRY.get(s.as_str()) {
                            e.i8(i)?;
                        } else {
                            e.str(s)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub fn get_coap_id(mimetype: &str) -> Option<u16> {
    COAP_CONTENT_FORMATS.get(mimetype).copied()
}

pub fn pack(header: &Header, payload: &[u8]) -> Result<Vec<u8>> {
    let mut h = header.clone();
    
    // 1. Optimize Content-Type to Content-Format
    if let Some(ct) = &h.content_type {
        if let Some(coap_id) = get_coap_id(ct) {
            h.content_format = Some(coap_id);
            h.content_type = None;
        }
    }
    
    // 2. Omit default Content-Format 0
    if h.content_format == Some(0) {
        h.content_format = None;
    }
    
    // 3. Ensure Message-Id is present
    if h.message_id.is_none() {
        bail!("Message-Id is mandatory in HQFBP header");
    }
    
    // 4. Update Payload-Size
    h.payload_size = Some(payload.len() as u64);

    let mut buf = Vec::new();
    let mut encoder = Encoder::new(&mut buf);
    encoder.encode(&h).map_err(|e| anyhow!("Header encode failed: {}", e))?;
    buf.extend_from_slice(payload);
    Ok(buf)
}

pub fn unpack(data: &[u8]) -> Result<(Header, Vec<u8>)> {
    let mut decoder = Decoder::new(data);
    let header: Header = match decoder.decode() {
        Ok(h) => h,
        Err(e) => {
            bail!("Header decode failed: {}", e);
        }
    };
    let pos = decoder.position();
    let payload = data[pos..].to_vec();
    
    // Trim payload based on payload_size if present
    if let Some(size) = header.payload_size {
        if payload.len() > size as usize {
            return Ok((header, payload[..size as usize].to_vec()));
        }
    }
    
    Ok((header, payload))
}

lazy_static! {
    pub static ref HQFBP_CBOR_KEYS: HashMap<&'static str, u8> = {
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
    };

    pub static ref REV_KEYS: HashMap<u8, &'static str> = {
        let mut m = HashMap::new();
        for (k, v) in HQFBP_CBOR_KEYS.iter() {
            m.insert(*v, *k);
        }
        m
    };

    pub static ref COAP_CONTENT_FORMATS: HashMap<&'static str, u16> = {
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
    };

    pub static ref REV_COAP_CONTENT_FORMATS: HashMap<u16, &'static str> = {
        let mut m = HashMap::new();
        for (k, v) in COAP_CONTENT_FORMATS.iter() {
            m.insert(*v, *k);
        }
        m
    };

    pub static ref ENCODING_REGISTRY: HashMap<i8, &'static str> = {
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
    };

    pub static ref REV_ENCODING_REGISTRY: HashMap<&'static str, i8> = {
        let mut m = HashMap::new();
        for (k, v) in ENCODING_REGISTRY.iter() {
            m.insert(*v, *k);
        }
        m
    };
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
