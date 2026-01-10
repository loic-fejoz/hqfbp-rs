use anyhow::{Result, bail};
use std::collections::{HashMap, VecDeque};
use crate::{Header, unpack, ContentEncoding, EncodingList, MediaType};
use crate::codec::*;
use bytes::Bytes;
// Regexes moved to lib.rs

#[derive(Debug, Clone)]
pub struct PDUEvent {
    pub header: Header,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
pub struct MessageEvent {
    pub header: Header,
    pub payload: Bytes,
}

#[derive(Debug, Clone)]
pub enum Event {
    PDU(PDUEvent),
    Message(MessageEvent),
}

pub struct Deframer {
    events: VecDeque<Event>,
    sessions: HashMap<(Option<String>, u32), Session>,
    announcements: HashMap<(Option<String>, u32), Vec<ContentEncoding>>,
    not_yet_decoded: Vec<Bytes>,
}

struct Session {
    chunks: HashMap<u32, (Bytes, usize)>, // chunk_id -> (payload, quality)
    headers: Vec<Header>,
    total_chunks: u32,
    _encodings: Option<Vec<ContentEncoding>>,
}

fn ce_to_list(ce: &EncodingList) -> Vec<ContentEncoding> {
    ce.0.clone()
}

fn get_rq_info(headers: &[Header]) -> Option<(usize, u16, u32)> {
    for h in headers {
        if let Some(ce) = &h.content_encoding {
            for enc in &ce.0 {
                if let ContentEncoding::RaptorQ(rq_len, mtu, repairs) = enc {
                    return Some((*rq_len, *mtu, *repairs));
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
        let b_data = Bytes::copy_from_slice(data);
        let peek_unpack = unpack(b_data.clone());
        let mut header: Option<Header> = None;
        let mut payload: Option<Bytes> = None;
        let mut encodings: Option<Vec<ContentEncoding>> = None;
        let mut pdu_quality = 0;

        // Phase 0: Fast path for announcements
        if let Ok((h, _p)) = &peek_unpack {
            // Phase 0: Fast path for announcements
            if h.media_type() == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())) || h.media_type() == Some(MediaType::Format(60)) {
                let ce_list = h.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
                let (pre, post, has_h) = self.split_encs(&ce_list);
                let pdu_level = if has_h { &post } else { &ce_list };
                if let Ok((p2, _)) = self.apply_decodings(b_data.clone(), pdu_level) {
                   if let Ok((_p3, _)) = self.apply_decodings(p2.clone(), &pre) {
                        if let Ok((h2, p2_payload)) = unpack(p2) {
                            if let Ok((p3_payload, _)) = self.apply_decodings(p2_payload, &pre) {
                        self.handle_announcement(h2.src_callsign.clone(), &p3_payload);
                        self.events.push_back(Event::PDU(PDUEvent { header: h2, payload: p3_payload }));
                                return;
                            }
                        }
                    }
                }
            }
        }

        // Phase 1: Direct lookup if header is readable and we have an announcement
        if let Ok((h, p)) = &peek_unpack {
            if h.media_type() == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())) || h.media_type() == Some(MediaType::Format(60)) {
                header = Some(h.clone());
                payload = Some(p.clone());
                if let Some(ce) = &h.content_encoding {
                    encodings = Some(ce_to_list(ce));
                }
            } else {
                let src_callsign = h.src_callsign.clone();
                let msg_id = h.original_message_id.or(h.message_id);
                
                if let Some(mid) = msg_id {
                    let key = (src_callsign.clone(), mid);
                    if let Some(ann_encs) = self.announcements.get(&key) {
                    if let Ok((recovered_pdu, quality)) = self.strip_post_boundary(b_data.clone(), ann_encs) {
                        if let Ok((h2, p2)) = unpack(recovered_pdu.clone()) {
                            header = Some(h2);
                            payload = Some(p2);
                            pdu_quality = quality;
                            encodings = Some(ann_encs.clone());
                        } else {
                            // Valid header + valid announcement -> Accept chunk
                            header = Some(h.clone());
                            payload = Some(recovered_pdu); 
                            pdu_quality = quality;
                            encodings = Some(ann_encs.clone());
                        }
                    }
                    } else if let Some(ce) = &h.content_encoding {
                        // No announcement, but header has encodings list
                        let ice = ce_to_list(ce);
                        if let Ok((recovered_pdu, quality)) = self.strip_post_boundary(b_data.clone(), &ice) {
                            if let Ok((h2, p2)) = unpack(recovered_pdu.clone()) {
                                header = Some(h2);
                                payload = Some(p2);
                                pdu_quality = quality;
                                encodings = Some(ice.clone());
                            } else {
                                // Valid header + multi-PDU expansion in header -> Accept chunk
                                header = Some(h.clone());
                                payload = Some(recovered_pdu);
                                pdu_quality = quality;
                                encodings = Some(ice.clone());
                            }
                        } else {
                            // strip_post_boundary failed, but maybe the raw payload is okay?
                            header = Some(h.clone());
                            payload = Some(p.clone());
                            encodings = Some(ice);
                            pdu_quality = 0;
                        }
                    }
                }
            }
        }

        // Phase 2: Heuristic Loop through ALL announcements if still not recovered
        if header.is_none() || payload.is_none() {
            for ann_encs in self.announcements.values() {
                // Try to strip this announcement's encodings
                let is_rq_post = self.raptorq_is_post_boundary(ann_encs);
                
                if is_rq_post {
                    let mut try_packets = self.not_yet_decoded.clone();
                    try_packets.push(b_data.clone());
                    
                    if let Ok((recovered_data, quality)) = self.strip_post_boundary_multi(&try_packets, ann_encs) {
                        if let Ok((mut h2, p2)) = unpack(recovered_data) {
                            // Strip the post-boundary encodings we just applied from the header
                            if let Some(ce) = &mut h2.content_encoding {
                                let pre = ce.0.iter()
                                    .filter(|e| !matches!(e, ContentEncoding::H) && !matches!(e, ContentEncoding::Chunk(_)) && !matches!(e, ContentEncoding::Repeat(_)))
                                    .cloned()
                                    .collect::<Vec<_>>();
                                if pre.is_empty() {
                                    h2.content_encoding = None;
                                } else {
                                    h2.content_encoding = Some(EncodingList(pre));
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
                    if let Ok((recovered_data, quality)) = self.strip_post_boundary(b_data.clone(), ann_encs) {
                        if let Ok((h2, p2)) = unpack(recovered_data) {
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
        if let Ok((h, p)) = &peek_unpack {
            header = Some(h.clone());
            payload = Some(p.clone());
        }
        }

        let Some(header) = header else {
            self.not_yet_decoded.push(b_data);
            return;
        };
        let Some(payload) = payload else { return; };
        
        let msg_id_opt = header.original_message_id.or(header.message_id);
        let Some(orig_msg_id) = msg_id_opt else {
            self.not_yet_decoded.push(b_data);
            return;
        };

        let src_callsign = header.src_callsign.clone();


        self.events.push_back(Event::PDU(PDUEvent { header: header.clone(), payload: payload.clone() }));

        let session_key = (src_callsign.clone(), orig_msg_id);
        let total_chunks = header.total_chunks.unwrap_or(1);
        let chunk_id = header.chunk_id.unwrap_or(0);

        let mut final_pdu_quality = pdu_quality;
        let mut final_payload = payload;

        if let Some(ce) = &encodings {
            let (_, post, has_h) = self.split_encs(ce);
            let pdu_level = if has_h { &post } else { ce };
            if let Ok((p2, q)) = self.apply_decodings(final_payload.clone(), pdu_level) {
                final_payload = p2;
                final_pdu_quality += q;
            }
        }

        let session = self.sessions.entry(session_key.clone()).or_insert_with(|| Session {
            chunks: HashMap::new(),
            headers: Vec::new(),
            total_chunks,
            _encodings: encodings.clone(),
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

    fn split_encs(&self, encs: &[ContentEncoding]) -> (Vec<ContentEncoding>, Vec<ContentEncoding>, bool) {
        if let Some(pos) = encs.iter().position(|e| matches!(e, ContentEncoding::H)) {
            (encs[..pos].to_vec(), encs[pos+1..].to_vec(), true)
        } else {
            (encs.to_vec(), Vec::new(), false)
        }
    }

    fn raptorq_is_post_boundary(&self, encs: &[ContentEncoding]) -> bool {
        if let Some(pos) = encs.iter().position(|e| matches!(e, ContentEncoding::H)) {
            encs[pos+1..].iter().any(|e| matches!(e, ContentEncoding::RaptorQ(_, _, _)))
        } else {
            false
        }
    }

    fn strip_post_boundary(&self, data: Bytes, encs: &[ContentEncoding]) -> Result<(Bytes, usize)> {
        let (_, post, _) = self.split_encs(encs);
        self.apply_decodings(data, &post)
    }

    fn apply_decodings(&self, mut data: Bytes, encs: &[ContentEncoding]) -> Result<(Bytes, usize)> {
        let mut quality = 0;
        for enc in encs.iter().rev() {
            match enc {
                ContentEncoding::Gzip => {
                    data = Bytes::from(gzip_decompress(&data)?);
                }
                ContentEncoding::Brotli => data = Bytes::from(brotli_decompress(&data)?),
                ContentEncoding::Lzma => data = Bytes::from(lzma_decompress(&data)?),
                ContentEncoding::Crc16 => {
                    if data.len() < 2 { bail!("too short for crc16"); }
                    let payload = data.slice(..data.len()-2);
                    let expected = &data[data.len()-2..];
                    if crc16_ccitt(&payload) != expected { bail!("crc16 fail"); }
                    data = payload;
                    quality += 1000;
                }
                ContentEncoding::Crc32 => {
                    if data.len() < 4 { bail!("too short for crc32"); }
                    let payload = data.slice(..data.len()-4);
                    let expected = &data[data.len()-4..];
                    if crc32_std(&payload) != expected { bail!("crc32 fail"); }
                    data = payload;
                    quality += 1000;
                }
                ContentEncoding::ReedSolomon(n, k) => {
                    let (d2, corrected) = rs_decode(&data, *n, *k)?;
                    data = Bytes::from(d2);
                    let num_blocks = data.len() / k;
                    let max_correctable = ((n - k) / 2) * num_blocks;
                    quality += max_correctable.saturating_sub(corrected);
                }
                ContentEncoding::RaptorQ(_, _, _) | ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_) => {
                    // These are multi-PDU or handled during reassembly
                }
                ContentEncoding::Conv(k_val, rate) => {
                    let (d2, metric) = conv_decode(&data, *k_val, rate)?;
                    data = Bytes::from(d2);
                    quality += (data.len() * 8).saturating_sub(metric);
                }
                ContentEncoding::Scrambler(poly) => {
                    data = Bytes::from(scr_xor(&data, *poly));
                }
                _ => {}
            }
        }
        Ok((data, quality))
    }

    fn strip_post_boundary_multi(&self, data: &[Bytes], encs: &[ContentEncoding]) -> Result<(Bytes, usize)> {
        let (_, post, _) = self.split_encs(encs);
        self.apply_decodings_multi(data.to_vec(), &post)
    }

    fn apply_decodings_multi(&self, packets: Vec<Bytes>, encs: &[ContentEncoding]) -> Result<(Bytes, usize)> {
        let mut quality = 0;
        let mut data = Bytes::new();
        let mut first = true;

        for enc in encs.iter().rev() {
            match enc {
                ContentEncoding::RaptorQ(rq_len, mtu, _) => {
                    if first {
                        data = Bytes::from(rq_decode(packets.clone(), *rq_len as usize, *mtu)?);
                        first = false;
                    } else {
                        data = Bytes::from(rq_decode(vec![data.clone()], *rq_len as usize, *mtu)?);
                    }
                    quality += 10;
                }
                other => {
                    // These expect a single contiguous buffer
                    if first {
                        let mut combined = Vec::new();
                        for p in &packets {
                            combined.extend_from_slice(p);
                        }
                        data = Bytes::from(combined);
                        first = false;
                    }
                    let (d2, q) = self.apply_decodings(data, &[other.clone()])?;
                    data = d2;
                    quality += q;
                }
            }
        }
        if first {
            let mut combined = Vec::new();
            for p in &packets {
                combined.extend_from_slice(p);
            }
            data = Bytes::from(combined);
        }
        Ok((data, quality))
    }

    fn complete_message(&mut self, key: (Option<String>, u32)) {
        let Some(session) = self.sessions.remove(&key) else { return; };
        
        let mut merged = session.headers[0].clone();
        for h in &session.headers[1..] {
            let _ = merged.merge(h);
        }
        merged.strip_chunking();
        
        let rq_info = get_rq_info(&session.headers);
        let mut data;

        if let Some((rq_len, mtu, _)) = rq_info {
            let packets: Vec<Bytes> = session.chunks.into_values().map(|(p, _)| p).collect();
            if let Ok(decoded) = rq_decode(packets, rq_len, mtu) {
                data = Bytes::from(decoded);
            } else { return; }
        } else {
            let mut sorted_keys: Vec<_> = session.chunks.keys().cloned().collect();
            sorted_keys.sort();
            let mut combined = Vec::new();
            for k in sorted_keys {
                let (chunk_data, _) = session.chunks.get(&k).unwrap();
                combined.extend_from_slice(chunk_data);
            }
            data = Bytes::from(combined);
        }

        // One level of recursive unpacking if post-boundary FEC returned another PDU
        if let Ok((h_inner, p_inner)) = unpack(data.clone()) {
            // It was a nested PDU (likely post-boundary FEC decoded result)
            merged = h_inner;
            data = p_inner;
            // Strip the FEC encoding from the inner header too
            if let Some(mut el) = merged.content_encoding.take() {
                el.0.retain(|e| !matches!(e, ContentEncoding::RaptorQ(_, _, _)));
                if !el.0.is_empty() {
                    merged.content_encoding = Some(el);
                }
            }
        }

        // Apply remaining decodings (those before the last expansion point)
        if let Some(ce) = &merged.content_encoding {
            let list = ce_to_list(ce);
            let (pre, _, _) = self.split_encs(&list);
            if let Ok((p2, _)) = self.apply_decodings(data, &pre) {
                data = p2;
            } else {
                return;
            }
        }

        if let Some(size) = merged.file_size {
            if data.len() > size as usize {
                data = data.slice(..size as usize);
            }
        }
        
        self.events.push_back(Event::Message(MessageEvent {
            header: merged.clone(),
            payload: data,
        }));
        // Final cleanup of header encodings for the Message event
        if let Some(ce) = &mut merged.content_encoding {
            ce.0.retain(|e| !matches!(e, ContentEncoding::H) && !matches!(e, ContentEncoding::Chunk(_)) && !matches!(e, ContentEncoding::Repeat(_)));
            if ce.0.is_empty() {
                merged.content_encoding = None;
            }
        }
    }
}
