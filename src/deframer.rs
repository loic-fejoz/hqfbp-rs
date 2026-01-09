use anyhow::{Result, bail};
use std::collections::{HashMap, VecDeque};
use crate::{Header, unpack, ContentEncoding};
use crate::codec::*;
use regex::Regex;
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
pub struct PDUEvent {
    pub header: Header,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct MessageEvent {
    pub header: Header,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Event {
    PDU(PDUEvent),
    Message(MessageEvent),
}

pub struct Deframer {
    events: VecDeque<Event>,
    sessions: HashMap<(Option<String>, u32), Session>,
    announcements: HashMap<(Option<String>, u32), Vec<String>>,
    not_yet_decoded: Vec<Vec<u8>>,
}

struct Session {
    chunks: HashMap<u32, (Vec<u8>, usize)>, // chunk_id -> (payload, quality)
    headers: Vec<Header>,
    total_chunks: u32,
}

fn ce_to_list(ce: &ContentEncoding) -> Vec<String> {
    match ce {
        ContentEncoding::Single(s) => vec![s.clone()],
        ContentEncoding::Multiple(v) => v.clone(),
        ContentEncoding::Integer(i) => vec![crate::ENCODING_REGISTRY.get(i).copied().unwrap_or("unknown").to_string()],
    }
}

fn get_rq_info(headers: &[Header]) -> Option<(usize, u16, u32)> {
    for h in headers {
        if let Some(ce) = &h.content_encoding {
            let ice = ce_to_list(ce);
            for enc in ice {
                if let Some(m) = RQ_RE.captures(&enc) {
                    let rq_len: usize = m[1].parse().ok()?;
                    let mtu: u16 = m[2].parse().ok()?;
                    let repairs: u32 = m[3].parse().ok()?;
                    return Some((rq_len, mtu, repairs));
                }
            }
        }
    }
    None
}

impl Deframer {
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            sessions: HashMap::new(),
            announcements: HashMap::new(),
            not_yet_decoded: Vec::new(),
        }
    }

    pub fn receive_bytes(&mut self, data: &[u8]) {
        let peek_unpack = unpack(data);
        let mut header: Option<Header> = None;
        let mut payload: Option<Vec<u8>> = None;
        let mut pdu_quality = 0;
        let mut encodings: Option<Vec<String>> = None;

        if let Ok((h, _)) = &peek_unpack {
            let src_callsign = h.src_callsign.clone();
            let msg_id = h.original_message_id.or(h.message_id);
            
            if let Some(mid) = msg_id {
                if let Some(ann_encs) = self.announcements.get(&(src_callsign.clone(), mid)) {
                    if let Ok((recovered_data, quality)) = self.strip_post_boundary(data, ann_encs) {
                        if let Ok((h2, p2)) = unpack(&recovered_data) {
                           header = Some(h2);
                           payload = Some(p2);
                           pdu_quality = quality;
                           encodings = Some(ann_encs.clone());
                        }
                    }
                } else if let Some(ce) = &h.content_encoding {
                    let ce_list = ce_to_list(ce);
                    if let Ok((recovered_data, quality)) = self.strip_post_boundary(data, &ce_list) {
                        if let Ok((h2, p2)) = unpack(&recovered_data) {
                            header = Some(h2);
                            payload = Some(p2);
                            pdu_quality = quality;
                            encodings = Some(ce_list);
                        }
                    }
                }
            }
        }

        if header.is_none() || payload.is_none() {
            for ann_encs in self.announcements.values() {
                let mut try_data = Vec::new();
                let is_rq_post = ann_encs.iter().position(|e| e == "h" || e == "-1")
                    .map(|pos| ann_encs[pos+1..].iter().any(|e| RQ_RE.is_match(e)))
                    .unwrap_or(false);

                if is_rq_post {
                    for prev in &self.not_yet_decoded {
                        try_data.push(prev.clone());
                    }
                    try_data.push(data.to_vec());
                    
                    if let Ok((recovered_data, quality)) = self.strip_post_boundary_multi(&try_data, ann_encs) {
                        if let Ok((mut h2, p2)) = unpack(&recovered_data) {
                            // Strip the post-boundary encodings we just applied from the header
                            if let Some(ce) = &mut h2.content_encoding {
                                let (pre, _, _) = self.split_encs(&ce_to_list(ce));
                                if pre.len() == 1 {
                                    h2.content_encoding = Some(ContentEncoding::Single(pre[0].clone()));
                                } else if pre.is_empty() {
                                    h2.content_encoding = None;
                                } else {
                                    h2.content_encoding = Some(ContentEncoding::Multiple(pre));
                                }
                            }
                            header = Some(h2);
                            payload = Some(p2);
                            pdu_quality = quality;
                            encodings = Some(ann_encs.clone());
                            self.not_yet_decoded.clear();
                            break;
                        }
                    }
                } else {
                    if let Ok((recovered_data, quality)) = self.strip_post_boundary(data, ann_encs) {
                        if let Ok((h2, p2)) = unpack(&recovered_data) {
                            header = Some(h2);
                            payload = Some(p2);
                            pdu_quality = quality;
                            encodings = Some(ann_encs.clone());
                            break;
                        }
                    }
                }
            }
        }

        if header.is_none() || payload.is_none() {
            if let Ok((h, p)) = peek_unpack {
                header = Some(h);
                payload = Some(p);
            }
        }

        let Some(header) = header else {
            self.not_yet_decoded.push(data.to_vec());
            return;
        };
        let Some(payload) = payload else { return; };
        let src_callsign = header.src_callsign.clone();

        if let Some(ct) = &header.content_type {
            if ct == "application/vnd.hqfbp+cbor" {
                if let Some(ce) = &header.content_encoding {
                    let ice = ce_to_list(ce);
                    let (pre, _, _) = self.split_encs(&ice);
                    if let Ok((p2, _)) = self.apply_decodings(payload.clone(), &pre) {
                        self.handle_announcement(src_callsign.clone(), &p2);
                        self.events.push_back(Event::PDU(PDUEvent { header: header.clone(), payload: p2 }));
                        return;
                    }
                } else {
                    self.handle_announcement(src_callsign, &payload);
                    self.events.push_back(Event::PDU(PDUEvent { header: header.clone(), payload: payload.clone() }));
                    return;
                }
            }
        }

        self.events.push_back(Event::PDU(PDUEvent { header: header.clone(), payload: payload.clone() }));

        let orig_msg_id = header.original_message_id.or(header.message_id).unwrap();
        let session_key = (src_callsign.clone(), orig_msg_id);
        let total_chunks = header.total_chunks.unwrap_or(1);
        let chunk_id = header.chunk_id.unwrap_or(0);

        let mut final_pdu_quality = pdu_quality;
        let mut final_payload = payload;

        if let Some(ce) = &encodings {
            let (pre, _, _) = self.split_encs(ce);
            let last_split = pre.iter().rposition(|e| CHUNK_RE.is_match(e) || REPEAT_RE.is_match(e) || RQ_RE.is_match(e));
            let per_pdu = match last_split {
                Some(idx) => &pre[idx+1..],
                None => &pre[..],
            };
            if let Ok((p2, q)) = self.apply_decodings(final_payload.clone(), per_pdu) {
                final_payload = p2;
                final_pdu_quality += q;
            }
        }

        let session = self.sessions.entry(session_key.clone()).or_insert_with(|| Session {
            chunks: HashMap::new(),
            headers: Vec::new(),
            total_chunks,
        });

        let existing = session.chunks.get(&chunk_id);
        if existing.is_none() || final_pdu_quality >= existing.unwrap().1 {
            session.chunks.insert(chunk_id, (final_payload, final_pdu_quality));
            session.headers.push(header.clone());
        }

        let completed = if session.chunks.len() as u32 == session.total_chunks {
            true
        } else if let Some((rq_len, mtu, _)) = get_rq_info(&session.headers) {
            let k = (rq_len + mtu as usize - 1) / mtu as usize;
            session.chunks.len() >= k
        } else {
            false
        };

        if completed {
            self.complete_message(session_key);
        }
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    fn handle_announcement(&mut self, src: Option<String>, payload: &[u8]) {
        if let Ok(h) = minicbor::decode::<Header>(payload) {
            if let (Some(mid), Some(ce)) = (h.message_id, h.content_encoding) {
                let ice = ce_to_list(&ce);
                self.announcements.insert((src, mid), ice);
            }
        }
    }

    fn split_encs(&self, encs: &[String]) -> (Vec<String>, Vec<String>, bool) {
        if let Some(pos) = encs.iter().position(|e| e == "h" || e == "-1") {
            (encs[..pos].to_vec(), encs[pos+1..].to_vec(), true)
        } else {
            (encs.to_vec(), Vec::new(), false)
        }
    }

    fn strip_post_boundary(&self, data: &[u8], encs: &[String]) -> Result<(Vec<u8>, usize)> {
        let (_, post, _) = self.split_encs(encs);
        self.apply_decodings(data.to_vec(), &post)
    }

    fn apply_decodings(&self, mut data: Vec<u8>, encs: &[String]) -> Result<(Vec<u8>, usize)> {
        let mut quality = 0;
        for enc in encs.iter().rev() {
            if enc == "gzip" || enc == "1" { data = gzip_decompress(&data)?; }
            else if enc == "br" || enc == "3" { data = brotli_decompress(&data)?; }
            else if enc == "lzma" || enc == "4" { data = lzma_decompress(&data)?; }
            else if enc == "crc16" || enc == "5" {
                if data.len() < 2 { bail!("too short for crc16"); }
                let payload = &data[..data.len()-2];
                let expected = &data[data.len()-2..];
                if crc16_ccitt(payload) != expected { bail!("crc16 fail"); }
                data = payload.to_vec();
                quality += 1000;
            }
            else if enc == "crc32" || enc == "6" {
                if data.len() < 4 { bail!("too short for crc32"); }
                let payload = &data[..data.len()-4];
                let expected = &data[data.len()-4..];
                if crc32_std(payload) != expected { bail!("crc32 fail"); }
                data = payload.to_vec();
                quality += 1000;
            }
            else if let Some(m) = RS_RE.captures(enc) {
                let n: usize = m[1].parse()?;
                let k: usize = m[2].parse()?;
                let (d2, corrected) = rs_decode(&data, n, k)?;
                data = d2;
                let num_blocks = data.len() / k;
                let max_correctable = ((n - k) / 2) * num_blocks;
                quality += max_correctable.saturating_sub(corrected);
            }
            else if let Some(m) = RQ_RE.captures(enc) {
                let rq_len: usize = m[1].parse()?;
                let mtu: u16 = m[2].parse()?;
                data = rq_decode(vec![data], rq_len, mtu)?;
                quality += 10;
            }
            else if let Some(m) = CONV_RE.captures(enc) {
                let k_val: usize = m[1].parse()?;
                let rate = &m[2];
                let (d2, metric) = conv_decode(&data, k_val, rate)?;
                data = d2;
                quality += (data.len() * 8).saturating_sub(metric);
            }
            else if let Some(m) = SCR_RE.captures(enc) {
                let poly_str = &m[1];
                let poly = if poly_str.starts_with("0x") { u64::from_str_radix(&poly_str[2..], 16)? } else { poly_str.parse()? };
                data = scr_xor(&data, poly);
            }
        }
        Ok((data, quality))
    }

    fn strip_post_boundary_multi(&self, data: &[Vec<u8>], encs: &[String]) -> Result<(Vec<u8>, usize)> {
        let (_, post, _) = self.split_encs(encs);
        self.apply_decodings_multi(data.to_vec(), &post)
    }

    fn apply_decodings_multi(&self, packets: Vec<Vec<u8>>, encs: &[String]) -> Result<(Vec<u8>, usize)> {
        let mut quality = 0;
        let mut data = Vec::new();
        let mut first = true;

        for enc in encs.iter().rev() {
            if enc == "gzip" || enc == "1" || enc == "br" || enc == "3" || enc == "lzma" || enc == "4" || 
               enc == "crc16" || enc == "5" || enc == "crc32" || enc == "6" || RS_RE.is_match(enc) || 
               CONV_RE.is_match(enc) || SCR_RE.is_match(enc) {
                // These expect a single contiguous buffer
                if first {
                    data = packets.concat();
                    first = false;
                }
                let (d2, q) = self.apply_decodings(data, &[enc.clone()])?;
                data = d2;
                quality += q;
            }
            else if let Some(m) = RQ_RE.captures(enc) {
                let rq_len: usize = m[1].parse()?;
                let mtu: u16 = m[2].parse()?;
                if first {
                    data = rq_decode(packets.clone(), rq_len, mtu)?;
                    first = false;
                } else {
                    data = rq_decode(vec![data], rq_len, mtu)?;
                }
                quality += 10;
            }
        }
        if first {
            data = packets.concat();
        }
        Ok((data, quality))
    }

    fn complete_message(&mut self, key: (Option<String>, u32)) {
        let Some(session) = self.sessions.remove(&key) else { return; };
        
        // Merge headers
        let mut merged = session.headers[0].clone();
        for h in &session.headers[1..] {
            let _ = merged.merge(h);
        }
        merged.strip_chunking();
        
        let mut data = Vec::new();
        let ce_list = if let Some(ce) = &merged.content_encoding {
            ce_to_list(ce)
        } else {
            Vec::new()
        };

        let (pre, _, _) = self.split_encs(&ce_list);
        let last_split = pre.iter().rposition(|e| CHUNK_RE.is_match(e) || REPEAT_RE.is_match(e) || RQ_RE.is_match(e));
        
        let message_level_encs = match last_split {
            Some(idx) => &pre[..idx + 1],
            None => &[], // Everything was applied per-PDU in receive_bytes
        };

        let rq_info = get_rq_info(&session.headers);

        if let Some((rq_len, mtu, _)) = rq_info {
            let packets: Vec<Vec<u8>> = session.chunks.into_values().map(|(p, _)| p).collect();
            if let Ok(decoded) = rq_decode(packets, rq_len, mtu) {
                data = decoded;
                // Now apply other message-level decodings EXCEPT 'rq' which we just did
                let mut filtered_msg_encs = Vec::new();
                for enc in message_level_encs {
                    if !RQ_RE.is_match(enc) {
                        filtered_msg_encs.push(enc.clone());
                    }
                }
                if let Ok((d2, _)) = self.apply_decodings(data, &filtered_msg_encs) {
                    data = d2;
                } else { return; }
            } else { return; }
        } else {
            let mut sorted_keys: Vec<_> = session.chunks.keys().collect();
            sorted_keys.sort();
            for k in sorted_keys {
                data.extend_from_slice(&session.chunks.get(k).unwrap().0);
            }
            if let Ok((d2, _)) = self.apply_decodings(data, message_level_encs) {
                data = d2;
            } else { return; }
        }

        if let Some(size) = merged.file_size {
            if data.len() > size as usize {
                data.truncate(size as usize);
            }
        }

        if let Some(ce) = &mut merged.content_encoding {
            match ce {
                ContentEncoding::Multiple(v) => {
                    v.retain(|e| e != "h" && e != "-1" && !CHUNK_RE.is_match(e) && !REPEAT_RE.is_match(e));
                    if v.is_empty() { merged.content_encoding = None; }
                    else if v.len() == 1 { merged.content_encoding = Some(ContentEncoding::Single(v[0].clone())); }
                }
                ContentEncoding::Single(s) => {
                    if s == "h" || s == "-1" || CHUNK_RE.is_match(s) || REPEAT_RE.is_match(s) {
                        merged.content_encoding = None;
                    }
                }
                ContentEncoding::Integer(i) => {
                    if *i == -1 { merged.content_encoding = None; }
                }
            }
        }

        self.events.push_back(Event::Message(MessageEvent {
            header: merged,
            payload: data,
        }));
    }
}
