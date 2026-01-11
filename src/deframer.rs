use anyhow::{Result, bail};
use std::collections::{HashMap, VecDeque};
use crate::{Header, unpack, ContentEncoding, EncodingList, MediaType};
use crate::codec::*;
use bytes::Bytes;

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

    fn is_fragmented(&self, header: &Header, payload_len: usize) -> bool {
        let ce = header.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
        let (_, post, has_h) = self.split_encs(&ce);
        if !has_h { return false; }
        
        // Python only considers chunk and repeat as fragmentation markers
        let has_reassembly = post.iter().any(|e| matches!(e, ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_)));
        if has_reassembly {
            if let Some(expected) = header.payload_size {
                return (payload_len as u64) < expected;
            }
        }
        false
    }

    fn apply_pdu_level_decodings(&self, header: &Header, payload: Bytes) -> Result<(Bytes, usize)> {
        let ce = header.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
        let (pre, _, _) = self.split_encs(&ce);
        
        let mut lsi = -1;
        for (i, e) in pre.iter().enumerate() {
            if matches!(e, ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_) | ContentEncoding::RaptorQ(_, _, _) | ContentEncoding::ReedSolomon(_, _)) {
                lsi = i as i32;
            }
        }
        
        let to_apply = if lsi != -1 { 
            pre[(lsi + 1) as usize..].to_vec() 
        } else { 
            pre 
        };
        
        if !to_apply.is_empty() {
            self.apply_decodings(payload, &to_apply)
        } else {
            Ok((payload, 0))
        }
    }

    pub fn receive_bytes(&mut self, data: &[u8]) {
        let b_data = Bytes::copy_from_slice(data);
        let mut header: Option<Header> = None;
        let mut payload: Option<Bytes> = None;
        let mut pdu_quality = 0;
        let mut decoded_pdu_level = false;

        // 1. Phase 1: Direct Recovery
        let peek_unpack = unpack(b_data.clone());
        if let Ok((h_peek, p_peek)) = &peek_unpack {
            let src_c = h_peek.src_callsign.clone();
            let m_id = h_peek.message_id;
            
            let ann_encs = if let Some(mid) = m_id {
                self.announcements.get(&(src_c.clone(), mid)).cloned()
            } else { None };

            if let Some(ann_encs) = ann_encs {
                if !self.is_fragmented(h_peek, p_peek.len()) {
                    if let Ok((stripped, q)) = self.strip_post_boundary(b_data.clone(), &ann_encs) {
                        if let Ok((mut h2, p2)) = unpack(stripped) {
                            self.strip_post_h_encodings(&mut h2);
                            if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&h2, p2) {
                                header = Some(h2);
                                payload = Some(p3);
                                pdu_quality = q + q_gain;
                                decoded_pdu_level = true;
                            }
                        }
                    }
                }
            } else if !self.is_fragmented(h_peek, p_peek.len()) {
                // If systematic RS allowed peeking, we should still apply post-boundary decodings
                // if they are present in the header itself.
                let ce_peek = h_peek.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
                let (_, post, has_h) = self.split_encs(&ce_peek);
                
                if has_h && !post.is_empty() {
                    if let Ok((stripped, q)) = self.strip_post_boundary(b_data.clone(), &ce_peek) {
                        if let Ok((mut h2, p2)) = unpack(stripped) {
                            self.strip_post_h_encodings(&mut h2);
                            if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&h2, p2) {
                                header = Some(h2);
                                payload = Some(p3);
                                pdu_quality = q + q_gain;
                                decoded_pdu_level = true;
                            }
                        }
                    }
                } else if let Ok((p2, q_gain)) = self.apply_pdu_level_decodings(h_peek, p_peek.clone()) {
                    header = Some(h_peek.clone());
                    payload = Some(p2);
                    pdu_quality = q_gain;
                    decoded_pdu_level = true;
                }
            }
        }

        // 2. Phase 2: Heuristic Recovery
        if header.is_none() || payload.is_none() {
            let mut try_list = self.not_yet_decoded.clone();
            try_list.push(b_data.clone());
            
            let ann_keys: Vec<_> = self.announcements.keys().cloned().collect();
            for key in ann_keys {
                let ann_encs = self.announcements.get(&key).unwrap().clone();
                match self.strip_post_boundary_multi(&try_list, &ann_encs) {
                    Ok((stripped, q)) => {
                        match unpack(stripped) {
                            Ok((mut h2, p2)) => {
                                if !self.is_fragmented(&h2, p2.len()) {
                                    self.strip_post_h_encodings(&mut h2);
                                    if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&h2, p2) {
                                        header = Some(h2);
                                        payload = Some(p3);
                                        pdu_quality = q + q_gain;
                                        decoded_pdu_level = true;
                                        self.not_yet_decoded.clear();
                                        break;
                                    }
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    Err(_) => {}
                }
            }
        }

        if header.is_none() || payload.is_none() {
            self.not_yet_decoded.push(b_data);
            return;
        }

        let header = header.unwrap();
        let payload = payload.unwrap();
        let src_callsign = header.src_callsign.clone();
        
        // 3. Dispatch Event and Handle Reassembly
        let is_ann = header.media_type() == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())) || 
                     header.media_type() == Some(MediaType::Format(60));

        if is_ann {
            let mut a_payload = payload.clone();
            if !decoded_pdu_level {
                if let Some(ce) = &header.content_encoding {
                    let ce_list = ce_to_list(ce);
                    let (pre, _, _) = self.split_encs(&ce_list);
                    if let Ok((p2, _)) = self.apply_decodings(a_payload.clone(), &pre) {
                        a_payload = p2;
                    }
                }
            }
            self.handle_announcement(src_callsign, &a_payload);
            return;
        }

        self.events.push_back(Event::PDU(PDUEvent { header: header.clone(), payload: payload.clone() }));

        let orig_id = header.original_message_id.or(header.message_id).unwrap();
        let session_key = (src_callsign.clone(), orig_id);
        let total_chunks = header.total_chunks.unwrap_or(1);
        let chunk_id = header.chunk_id.unwrap_or(0);

        let session = self.sessions.entry(session_key.clone()).or_insert_with(|| Session {
            chunks: HashMap::new(),
            headers: Vec::new(),
            total_chunks,
        });

        let existing = session.chunks.get(&chunk_id);
        if existing.is_none() || pdu_quality >= existing.unwrap().1 {
            session.chunks.insert(chunk_id, (payload, pdu_quality));
            session.headers.push(header);
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


    fn strip_post_h_encodings(&self, h: &mut Header) {
        if let Some(ce) = &mut h.content_encoding {
            if let Some(pos) = ce.0.iter().position(|e| matches!(e, ContentEncoding::H)) {
                let pre = ce.0[..pos].to_vec();
                if pre.is_empty() {
                    h.content_encoding = None;
                } else {
                    h.content_encoding = Some(EncodingList(pre));
                }
            }
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
                ContentEncoding::H => {}
                ContentEncoding::Identity => {}
                ContentEncoding::Gzip => data = Bytes::from(gzip_decompress(&data)?),
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
                    match rs_decode(&data, *n, *k) {
                        Ok((d2, corrected)) => {
                            data = Bytes::from(d2);
                            let num_blocks = data.len() / k;
                            let max_correctable = ((n - k) / 2) * num_blocks;
                            quality += max_correctable.saturating_sub(corrected);
                        }
                        Err(e) => bail!(e),
                    }
                }
                ContentEncoding::Repeat(_count) => {
                    // In HQFBP, Repeat after the boundary (PDU-level) typically means 
                    // multiple PDUs are generated. When we have a single PDU (Bytes), 
                    // it's already one copy. Slicing it would destroy it (e.g. if it's RS encoded).
                    // We only slice at the reassembly level (apply_decodings_multi).
                }
                ContentEncoding::Conv(k_val, rate) => {
                    let (d2, metric) = conv_decode(&data, *k_val, rate)?;
                    data = Bytes::from(d2);
                    quality += (data.len() * 8).saturating_sub(metric);
                }
                ContentEncoding::Scrambler(poly) => {
                    data = Bytes::from(scr_xor(&data, *poly));
                }
                ContentEncoding::RaptorQ(rq_len, mtu, _) => {
                    data = Bytes::from(rq_decode(vec![data], *rq_len as usize, *mtu)?);
                    quality += 10;
                }
                ContentEncoding::Chunk(_) => {}
                ContentEncoding::OtherString(_) | ContentEncoding::OtherInteger(_) | ContentEncoding::Deflate => {}
            }
        }
        Ok((data, quality))
    }

    fn strip_post_boundary_multi(&self, data: &[Bytes], encs: &[ContentEncoding]) -> Result<(Bytes, usize)> {
        let (_, post, _) = self.split_encs(encs);
        self.apply_decodings_multi(data.to_vec(), &post)
    }

    fn apply_decodings_multi(&self, mut packets: Vec<Bytes>, encs: &[ContentEncoding]) -> Result<(Bytes, usize)> {
        let mut quality = 0;
        let mut data: Option<Bytes> = None;

        for enc in encs.iter().rev() {
            if let Some(mut d) = data {
                match enc {
                    ContentEncoding::Repeat(count) => {
                        let n = *count as usize;
                        if n > 1 { d = d.slice(..d.len() / n); }
                    }
                    ContentEncoding::Chunk(_) => {}
                    ContentEncoding::RaptorQ(rq_len, mtu, _) => {
                        d = Bytes::from(rq_decode(vec![d], *rq_len as usize, *mtu)?);
                        quality += 10;
                    }
                    other => {
                        let (d2, q) = self.apply_decodings(d, &[other.clone()])?;
                        d = d2;
                        quality += q;
                    }
                }
                data = Some(d);
            } else {
                match enc {
                    ContentEncoding::Repeat(count) => {
                        let n = *count as usize;
                        if n > 1 {
                             // Use step_by instead of truncate!
                             packets = packets.into_iter().step_by(n).collect();
                        }
                    }
                    ContentEncoding::RaptorQ(rq_len, mtu, _) => {
                        // Pass packets directly to rq_decode
                        // We must ensure packets are Bytes
                        if let Ok(decoded) = rq_decode(packets.clone(), *rq_len as usize, *mtu) {
                            data = Some(Bytes::from(decoded));
                            quality += 10;
                        } else {
                            // If decode fails, we can't proceed with this encoding chain effectively for this step
                            data = None;
                        }
                    }
                    ContentEncoding::Chunk(_) => {
                        let mut combined = Vec::new();
                        for p in &packets {
                            combined.extend_from_slice(p);
                        }
                        data = Some(Bytes::from(combined));
                    }
                    other => {
                        let mut combined = Vec::new();
                        for p in &packets {
                            combined.extend_from_slice(p);
                        }
                        let (d2, q) = self.apply_decodings(Bytes::from(combined), &[other.clone()])?;
                        data = Some(d2);
                        quality += q;
                    }
                }
            }
        }
        
        if let Some(d) = data {
            Ok((d, quality))
        } else {
            let mut combined = Vec::new();
            for p in &packets {
                combined.extend_from_slice(p);
            }
            Ok((Bytes::from(combined), quality))
        }
    }

    fn complete_message(&mut self, key: (Option<String>, u32)) {
        let Some(session) = self.sessions.remove(&key) else { return; };
        
        let mut merged = session.header_from_first();
        for h in &session.headers[1..] {
            let _ = merged.merge(h);
        }
        merged.strip_chunking();
        
        let rq_info = get_rq_info(&session.headers);
        let mut data;

        if let Some((rq_len, mtu, _)) = rq_info {
            let mut sorted_indices: Vec<_> = session.chunks.keys().cloned().collect();
            sorted_indices.sort();
            let packets: Vec<Bytes> = sorted_indices.iter().map(|i| session.chunks.get(i).unwrap().0.clone()).collect();
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

        let ce_list = merged.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
        let (pre, _, _) = self.split_encs(&ce_list);
        
        let mut lsi = -1;
        for (i, e) in pre.iter().enumerate() {
            if matches!(e, ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_) | ContentEncoding::RaptorQ(_, _, _) | ContentEncoding::ReedSolomon(_, _)) {
                lsi = i as i32;
            }
        }
        
        let msg_encs = if lsi != -1 { 
            let end_idx_opt = if rq_info.is_some() {
                // If we handled RaptorQ manually, we must skip it in applying decodings
                // Assuming RQ is the LSI (last reassembly step)
                if let ContentEncoding::RaptorQ(_, _, _) = pre[lsi as usize] {
                    if lsi > 0 { Some(lsi as usize - 1) } else { None }
                } else {
                    Some(lsi as usize)
                }
            } else {
                Some(lsi as usize)
            };
            
            if let Some(idx) = end_idx_opt {
                 pre[..=idx].to_vec()
            } else {
                 Vec::new()
            }
        } else { 
            Vec::new() 
        };
        
        if !msg_encs.is_empty() {
            if let Ok((d2, _)) = self.apply_decodings(data.clone(), &msg_encs) {
                data = d2;
            }
        }

        if let Ok((h_inner, p_inner)) = unpack(data.clone()) {
            if h_inner.message_id.is_some() {
                let inner_ce = h_inner.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
                let (pre_inner, _, _) = self.split_encs(&inner_ce);
                if let Ok((p_inner2, _)) = self.apply_decodings(p_inner, &pre_inner) {
                    merged = h_inner;
                    data = p_inner2;
                }
            }
        }

        if let Some(size) = merged.file_size {
            if data.len() > size as usize {
                data = data.slice(..size as usize);
            }
        }
        
        if let Some(ce) = &mut merged.content_encoding {
            let cur_list = ce.0.clone();
            let mut new_ce = Vec::new();
            for (idx, e) in cur_list.iter().enumerate() {
                if matches!(e, ContentEncoding::H) { continue; }
                if lsi != -1 && idx <= lsi as usize { continue; }
                new_ce.push(e.clone());
            }
            if new_ce.is_empty() {
                merged.content_encoding = None;
            } else {
                merged.content_encoding = Some(EncodingList(new_ce));
            }
        }

        self.events.push_back(Event::Message(MessageEvent {
            header: merged,
            payload: data,
        }));
    }
}

impl Session {
    fn header_from_first(&self) -> Header {
        if self.headers.is_empty() {
            Header::default()
        } else {
            self.headers[0].clone()
        }
    }
}
