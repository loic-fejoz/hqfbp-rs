use anyhow::Result;
use regex::Regex;
use crate::{Header, pack};
use crate::codec::*;
use lazy_static::lazy_static;

lazy_static! {
    static ref RS_RE: Regex = Regex::new(r"rs\((\d+),\s*(\d+)\)").unwrap();
    static ref RQ_RE: Regex = Regex::new(r"rq\((\d+),\s*(\d+),\s*(\d+)\)").unwrap();
    static ref CONV_RE: Regex = Regex::new(r"conv\((\d+),\s*(\d+/\d+)\)").unwrap();
    static ref SCR_RE: Regex = Regex::new(r"scr\((0x[0-9a-fA-F]+|\d+)\)").unwrap();
    static ref CHUNK_RE: Regex = Regex::new(r"chunk\((\d+)\)").unwrap();
    static ref REPEAT_RE: Regex = Regex::new(r"repeat\((\d+)\)").unwrap();
}

#[derive(Debug, Clone)]
pub enum EncValue {
    Integer(i8),
    String(String),
}

impl EncValue {
    pub fn is_boundary(&self) -> bool {
        match self {
            EncValue::Integer(i) => *i == -1,
            EncValue::String(s) => s == "h",
        }
    }
}

pub struct PDUGenerator {
    pub src_callsign: Option<String>,
    pub dst_callsign: Option<String>,
    pub max_payload_size: Option<usize>,
    pub encodings: Vec<EncValue>,
    pub announcement_encoder: Option<Box<PDUGenerator>>,
    next_msg_id: u32,
}

impl PDUGenerator {
    pub fn new(
        src_callsign: Option<String>,
        dst_callsign: Option<String>,
        max_payload_size: Option<usize>,
        encodings: Option<Vec<EncValue>>,
        announcement_encodings: Option<Vec<EncValue>>,
        initial_msg_id: u32,
    ) -> Self {
        let ann_encoder = announcement_encodings.map(|e| {
            Box::new(PDUGenerator::new(
                src_callsign.clone(),
                dst_callsign.clone(),
                None,
                Some(e),
                None,
                initial_msg_id,
            ))
        });

        Self {
            src_callsign,
            dst_callsign,
            max_payload_size,
            encodings: encodings.unwrap_or_default(),
            announcement_encoder: ann_encoder,
            next_msg_id: initial_msg_id,
        }
    }

    fn get_next_msg_id(&mut self) -> u32 {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        id
    }

    fn apply_encodings(&self, data: &[u8], encodings: &[EncValue]) -> Result<Vec<Vec<u8>>> {
        let mut current_data = data.to_vec();
        for enc in encodings {
            match enc {
                EncValue::Integer(1) => current_data = gzip_compress(&current_data)?,
                EncValue::Integer(3) => current_data = brotli_compress(&current_data)?,
                EncValue::Integer(4) => current_data = lzma_compress(&current_data)?,
                EncValue::Integer(5) => {
                    let crc = crc16_ccitt(&current_data);
                    current_data.extend_from_slice(&crc);
                }
                EncValue::Integer(6) => {
                    let crc = crc32_std(&current_data);
                    current_data.extend_from_slice(&crc);
                }
                EncValue::String(s) => {
                    if s == "gzip" { current_data = gzip_compress(&current_data)?; }
                    else if s == "br" { current_data = brotli_compress(&current_data)?; }
                    else if s == "lzma" { current_data = lzma_compress(&current_data)?; }
                    else if s == "crc16" {
                        let crc = crc16_ccitt(&current_data);
                        current_data.extend_from_slice(&crc);
                    }
                    else if s == "crc32" {
                        let crc = crc32_std(&current_data);
                        current_data.extend_from_slice(&crc);
                    }
                    else if let Some(m) = RS_RE.captures(s) {
                        let n: usize = m[1].parse()?;
                        let k: usize = m[2].parse()?;
                        current_data = rs_encode(&current_data, n, k)?;
                    }
                    else if let Some(m) = RQ_RE.captures(s) {
                        let rq_len: usize = m[1].parse()?;
                        let mtu: u16 = m[2].parse()?;
                        let repairs: u32 = m[3].parse()?;
                        return Ok(rq_encode(&current_data, rq_len, mtu, repairs)?);
                    }
                    else if let Some(m) = CONV_RE.captures(s) {
                        let k: usize = m[1].parse()?;
                        let rate = &m[2];
                        current_data = conv_encode(&current_data, k, rate)?;
                    }
                    else if let Some(m) = SCR_RE.captures(s) {
                        let poly_str = &m[1];
                        let poly = if poly_str.starts_with("0x") {
                            u64::from_str_radix(&poly_str[2..], 16)?
                        } else {
                            poly_str.parse()?
                        };
                        current_data = scr_xor(&current_data, poly);
                    }
                }
                _ => {}
            }
        }
        Ok(vec![current_data])
    }

    fn resolve_encodings(&self) -> Vec<EncValue> {
        let mut encs = self.encodings.clone();
        
        let has_boundary = encs.iter().any(|e| e.is_boundary());
        if !has_boundary {
            encs.push(EncValue::Integer(-1));
        }

        let boundary_idx = encs.iter().position(|e| e.is_boundary()).unwrap();
        let pre = &encs[..boundary_idx];
        
        let mut has_chunk = pre.iter().any(|e| {
            if let EncValue::String(s) = e {
                CHUNK_RE.is_match(s)
            } else { false }
        });

        if !has_chunk {
            for (i, e) in pre.iter().enumerate() {
                if let EncValue::String(s) = e {
                    if let Some(m) = RS_RE.captures(s) {
                        let k = &m[2];
                        encs.insert(i, EncValue::String(format!("chunk({})", k)));
                        has_chunk = true;
                        break;
                    }
                    if RQ_RE.is_match(s) {
                        has_chunk = true;
                        break;
                    }
                }
            }
        }

        if !has_chunk {
            if let Some(size) = self.max_payload_size {
                let current_b_idx = encs.iter().position(|e| e.is_boundary()).unwrap();
                encs.insert(current_b_idx, EncValue::String(format!("chunk({})", size)));
            }
        }

        encs
    }

    pub fn generate(&mut self, data: &[u8], content_type: Option<String>) -> Result<Vec<Vec<u8>>> {
        let file_size = data.len() as u64;
        let full_encs = self.resolve_encodings();
        let mut current_chunks = vec![data.to_vec()];
        
        let ann_msg_id = if self.announcement_encoder.is_some() {
            Some(self.get_next_msg_id())
        } else {
            None
        };
        let data_orig_id = self.next_msg_id;

        let header_template = Header {
            file_size: Some(file_size),
            src_callsign: self.src_callsign.clone(),
            dst_callsign: self.dst_callsign.clone(),
            content_type,
            ..Default::default()
        };

        for i in 0..full_encs.len() {
            let enc = &full_encs[i];
            if enc.is_boundary() {
                let total_chunks = current_chunks.len() as u32;
                let mut new_chunks = Vec::new();
                for (idx, chunk_data) in current_chunks.iter().enumerate() {
                    let mut header = header_template.clone();
                    
                    let msg_id = if idx == 0 {
                        let id = data_orig_id;
                        if self.next_msg_id == id { self.next_msg_id += 1; }
                        id
                    } else {
                        self.get_next_msg_id()
                    };

                    if total_chunks > 1 {
                        header.total_chunks = Some(total_chunks);
                        header.chunk_id = Some(idx as u32);
                        header.original_message_id = Some(data_orig_id);
                    }
                    header.message_id = Some(msg_id);
                    
                    if idx > 0 {
                        header.content_type = None;
                        header.content_format = None;
                    }

                    // Content-Encoding for this PDU
                    let ce_strings: Vec<String> = full_encs.iter().map(|ev| match ev {
                        EncValue::Integer(i) => crate::ENCODING_REGISTRY.get(i).copied().unwrap_or("unknown").to_string(),
                        EncValue::String(s) => s.clone(),
                    }).collect();
                    if ce_strings.len() == 1 {
                        header.content_encoding = Some(crate::ContentEncoding::Single(ce_strings[0].clone()));
                    } else {
                        header.content_encoding = Some(crate::ContentEncoding::Multiple(ce_strings));
                    }
                    header.payload_size = Some(chunk_data.len() as u64);

                    new_chunks.push(pack(&header, chunk_data)?);
                }
                current_chunks = new_chunks;
            } else if let EncValue::String(s) = enc {
                if let Some(m) = CHUNK_RE.captures(s) {
                    let size: usize = m[1].parse()?;
                    let mut next_chunks = Vec::new();
                    for chunk in current_chunks {
                        for sub in chunk.chunks(size) {
                            next_chunks.push(sub.to_vec());
                        }
                    }
                    current_chunks = next_chunks;
                } else if let Some(m) = REPEAT_RE.captures(s) {
                    let count: usize = m[1].parse()?;
                    let mut next_chunks = Vec::new();
                    for chunk in current_chunks {
                        for _ in 0..count {
                            next_chunks.push(chunk.clone());
                        }
                    }
                    current_chunks = next_chunks;
                } else {
                    // Transformation
                    let mut next_chunks = Vec::new();
                    for c in &current_chunks {
                        let mut actual_enc = s.clone();
                        if s.starts_with("rq(dlen,") {
                            actual_enc = s.replace("rq(dlen,", &format!("rq({},", c.len()));
                        }
                        let transformed = self.apply_encodings(c, &[EncValue::String(actual_enc)])?;
                        next_chunks.extend(transformed);
                    }
                    current_chunks = next_chunks;
                }
            } else {
                // Integer registry transformation
                let mut next_chunks = Vec::new();
                for c in &current_chunks {
                    let transformed = self.apply_encodings(c, &[enc.clone()])?;
                    next_chunks.extend(transformed);
                }
                current_chunks = next_chunks;
            }
        }

        let mut final_pdus = Vec::new();
        if let (Some(ann_enc), Some(aid)) = (self.announcement_encoder.as_mut(), ann_msg_id) {
            ann_enc.next_msg_id = aid;
            // Announcement payload: {0: data_orig_id, 5: full_encs}
            let mut ann_header = Header {
                message_id: Some(data_orig_id), // Wait, announcement usually refers to the future message-id
                ..Default::default()
            };
            let ce_strings: Vec<String> = full_encs.iter().map(|ev| match ev {
                EncValue::Integer(i) => crate::ENCODING_REGISTRY.get(i).copied().unwrap_or("unknown").to_string(),
                EncValue::String(s) => s.clone(),
            }).collect();
            ann_header.content_encoding = Some(crate::ContentEncoding::Multiple(ce_strings));
            
            let ann_payload = minicbor::to_vec(&ann_header)?;
            let ann_pdus = ann_enc.generate(&ann_payload, Some("application/vnd.hqfbp+cbor".to_string()))?;
            final_pdus.extend(ann_pdus);
        }

        final_pdus.extend(current_chunks);
        Ok(final_pdus)
    }
}
