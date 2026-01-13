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

fn split_encs(encs: &[ContentEncoding]) -> (Vec<ContentEncoding>, Vec<ContentEncoding>, bool) {
    if let Some(pos) = encs.iter().position(|e| matches!(e, ContentEncoding::H)) {
        (encs[..pos].to_vec(), encs[pos+1..].to_vec(), true)
    } else {
        (encs.to_vec(), Vec::new(), false)
    }
}


impl Default for Deframer {
    fn default() -> Self {
        Self::new()
    }
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


    fn apply_pdu_level_decodings(&self, header: &Header, payload: Bytes) -> Result<(Bytes, usize)> {
        let ce = header.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
        let (pre, _, _has_h) = split_encs(&ce);
        
        // Late Truncation: Fix the payload size based on the header BEFORE decodings.
        // This removes padding from layers like RS or RQ that might have been applied to the PDU.
        let mut current_payload = payload;
        if let Some(size) = header.payload_size 
            && current_payload.len() > size as usize {
                current_payload = current_payload.slice(..size as usize);
        }

        let is_fragmented = header.total_chunks.unwrap_or(1) > 1 || header.chunk_id.unwrap_or(0) > 0;
        
        let mut lsi = -1;
        for (i, e) in pre.iter().enumerate() {
            if matches!(e, ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_) | ContentEncoding::RaptorQ(_, _, _) | ContentEncoding::ReedSolomon(_, _)) {
                lsi = i as i32;
            }
        }
        
        // If implicitly fragmented (Chunk hidden), assume all pre encodings are session-level
        // unless explicitly handled above.
        if is_fragmented && lsi == -1 && !pre.is_empty() {
             lsi = pre.len() as i32; 
        }

        let to_apply = if lsi != -1 { 
            if (lsi as usize) + 1 >= pre.len() {
                Vec::new()
            } else {
                pre[(lsi + 1) as usize..].to_vec() 
            }
        } else { 
            pre 
        };

        if !to_apply.is_empty() {
             let mut to_apply_rev = to_apply.clone();
             to_apply_rev.reverse();
             self.apply_decodings(current_payload, &to_apply_rev, Some(header), false)
        } else {
            Ok((current_payload, 0))
        }
    }

    pub fn receive_bytes(&mut self, data: &[u8]) {
        let b_data = Bytes::copy_from_slice(data);
        let mut header: Option<Header> = None;
        let mut payload: Option<Bytes> = None;
        let mut pdu_quality = 0;
        let mut _decoded_pdu_level = false;

        // 1. Phase 1: Recovery and PDU-level decoding
        let peek_unpack = unpack(b_data.clone());
        if let Ok((h_peek, p_peek)) = &peek_unpack {
            let src_c = h_peek.src_callsign.clone();
            let m_id = h_peek.message_id;
            
            let target_id = h_peek.original_message_id.or(m_id);
            let ce_list = if let Some(mid) = target_id {
                self.announcements.get(&(src_c.clone(), mid)).cloned()
                    .or_else(|| h_peek.content_encoding.as_ref().map(ce_to_list))
            } else {
                h_peek.content_encoding.as_ref().map(ce_to_list)
            };

            if let Some(ce) = ce_list {
                let (_, post, _) = split_encs(&ce);

                // Apply PDU-level decodings to the WHOLE PDU
                match self.apply_decodings(b_data.clone(), &post, Some(h_peek), false) {
                    Ok((clean_pdu, q)) => {
                        if let Ok((mut h2, p2)) = unpack(clean_pdu.clone()) {
                            self.strip_post_h_encodings(&mut h2);
                             // Check for announcement
                            let is_ann = h2.media_type() == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())) || 
                                         h2.media_type() == Some(MediaType::Format(60));
                            if is_ann {
                                // eprintln!("DEBUG: Phase 1 decoded Announcement!");
                            }

                            if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&h2, p2.clone()) {
                                header = Some(h2);
                                payload = Some(p3);
                                pdu_quality = q + q_gain;
                                _decoded_pdu_level = true;
                            }
                        }
                    }
                    Err(e) => {
                        let is_ann = h_peek.media_type() == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())) || 
                                     h_peek.media_type() == Some(MediaType::Format(60));
                        if is_ann {
                             eprintln!("DEBUG: Phase 1 Announcement failed: {e}");
                        }
                    }
                }
            } else {
                // No encodings known, use the peeked result as-is
                header = Some(h_peek.clone());
                payload = Some(p_peek.clone());
                _decoded_pdu_level = true;
            }
        }

        // 2. Phase 2: Heuristic Recovery
        if header.is_none() || payload.is_none() {
            let ann_keys: Vec<_> = self.announcements.keys().cloned().collect();
            let mut reclaimed_any = false;
            
            for key in ann_keys {
                let ann_encs = self.announcements.get(&key).unwrap().clone();
                let (_, post, _) = split_encs(&ann_encs);
                
                // 2a. Try decoding JUST the current packet (Per-PDU FEC)
                let mut single_success = false;
                if let Ok((clean_pdu, q)) = self.apply_decodings_multi(vec![b_data.clone()], &post, None, false)
                     && let Ok((mut h2, p2)) = unpack(clean_pdu) {
                        self.strip_post_h_encodings(&mut h2);
                        if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&h2, p2) {
                            let src_c = h2.src_callsign.clone();
                            let orig_id = h2.original_message_id.or(h2.message_id).unwrap_or(0);
                            let session_key = (src_c, orig_id);
                            let chunk_id = h2.chunk_id.unwrap_or(0);
                            let new_quality = q + q_gain;
                             
                            let already_had_better = if let Some(s) = self.sessions.get(&session_key) {
                                if let Some(existing) = s.chunks.get(&chunk_id) {
                                    existing.1 >= new_quality
                                } else { false }
                            } else { false };

                            if !already_had_better {
                                self.process_pdu(h2, p3, new_quality);
                                reclaimed_any = true;
                                single_success = true;
                            }
                        }
                }
                
                if single_success {
                    continue;
                }

                // If post contains ONLY packet-local encodings (RS, CRC, Scrambler), 
                // group decoding is useless because they would have passed Phase 1 if valid.
                // We only need to retry group decoding if there are "Combiners" (RQ, Chunk, Repeat).
                let has_combiner = post.iter().any(|e| matches!(e, 
                    ContentEncoding::RaptorQ(_,_,_) | 
                    ContentEncoding::RaptorQDynamic(_,_) | 
                    ContentEncoding::Chunk(_) | 
                    ContentEncoding::Repeat(_) |
                    ContentEncoding::H
                ));

                if !has_combiner {
                    continue;
                }

                let mut try_list = self.not_yet_decoded.clone();
                try_list.push(b_data.clone());
                match self.apply_decodings_multi(try_list, &post, None, false) {
                    Ok((clean_pdu, q)) => {
                        match unpack(clean_pdu) {
                            Ok((mut h2, p2)) => {
                                self.strip_post_h_encodings(&mut h2);
                                if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&h2, p2) {
                                    let src_c = h2.src_callsign.clone();
                                    let orig_id = h2.original_message_id.or(h2.message_id).unwrap_or(0);
                                    let session_key = (src_c, orig_id);
                                    let chunk_id = h2.chunk_id.unwrap_or(0);
                                    let new_quality = q + q_gain;
                                    
                                    let already_had_better = if let Some(s) = self.sessions.get(&session_key) {
                                        if let Some(existing) = s.chunks.get(&chunk_id) {
                                            existing.1 >= new_quality
                                        } else { false }
                                    } else { false };

                                    self.process_pdu(h2, p3, new_quality);
                                    reclaimed_any = true;
                                    if !already_had_better {
                                         self.not_yet_decoded = Vec::new();
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("DEBUG: unpack failed: {e}");
                            }
                        }
                    }
                    Err(_e) => {
                        if !post.is_empty() {
                            // eprintln!("DEBUG: Phase 2 apply_decodings_multi fail: {:?}", e);
                        }
                    }
                }
                if reclaimed_any { break; }
            }

            if !reclaimed_any {
                self.not_yet_decoded.push(b_data);
            }
            return;
        }

        self.process_pdu(header.unwrap(), payload.unwrap(), pdu_quality);
    }

    fn apply_decodings_multi(&self, input: Vec<Bytes>, encs: &[ContentEncoding], header: Option<&Header>, _header_already_unpacked: bool) -> Result<(Bytes, usize)> {
        let mut current = input;
        let mut quality = 0;
        
        for enc in encs.iter().rev() {
            match enc {
                ContentEncoding::H | ContentEncoding::Chunk(_) => {
                    let mut joined = Vec::new();
                    for b in &current { joined.extend_from_slice(b); }
                    current = vec![Bytes::from(joined)];
                }
                ContentEncoding::Identity => {}
                ContentEncoding::Repeat(count) => {
                    if current.len() > 1 {
                        current = current.into_iter().step_by(*count).collect();
                    }
                }
                ContentEncoding::RaptorQ(rq_len, mtu, _) => {
                    match rq_decode(current.clone(), *rq_len, *mtu) {
                        Ok(res) => {
                            current = vec![Bytes::from(res)];
                            quality += 10;
                        }
                        Err(e) => {
                            // Silence RQ errors here as it's expected in Phase 2
                            return Err(e);
                        }
                    }
                }
                ContentEncoding::RaptorQDynamic(mtu, _) => {
                    // For Dynamic RQ in Phase 2, we estimate rq_len from total received data
                    let total_len: usize = current.iter().map(|b| b.len()).sum();
                    // Assuming total_len is a multiple of (mtu + overhead), but we try our best
                    let res = rq_decode(current, total_len, *mtu)?; 
                    current = vec![Bytes::from(res)];
                    quality += 10;
                }
                other => {
                    if current.len() == 1 {
                        let (d, q) = self.apply_decodings(current.remove(0), std::slice::from_ref(other), header, false)?;
                        current = vec![d];
                        quality += q;
                    } else {
                        let mut next = Vec::new();
                        for b in current {
                            if let Ok((d, q)) = self.apply_decodings(b, std::slice::from_ref(other), header, false) {
                                next.push(d);
                                quality += q;
                            }
                        }
                        current = next;
                    }
                }
            }
        }
        
        let mut final_data = Vec::new();
        for b in current { final_data.extend_from_slice(&b); }
        if final_data.is_empty() { bail!("Empty data after multi-decoding"); }
        Ok((Bytes::from(final_data), quality))
    }

    fn process_pdu(&mut self, header: Header, payload: Bytes, pdu_quality: usize) {
        let _decoded_pdu_level = true; // Heuristic path always sets this to true effectively
        
        let src_callsign = header.src_callsign.clone();
        
        // 3. Dispatch Event and Handle Reassembly
        let is_ann = header.media_type() == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())) || 
                     header.media_type() == Some(MediaType::Format(60));

        if is_ann {
            let mut a_payload = payload.clone();
            // We need to know if we should apply pre-boundary decodings
            // For announcements, we usually don't have PDU-level RS in Phase 1 
            // but we might have it in Phase 2.
            // Let's assume for now that if we are here, we might need to apply pre decodings.
            if let Some(ce) = &header.content_encoding {
                let ce_list_pdu = ce_to_list(ce);
                let (pre_ann, _, has_h_ann) = split_encs(&ce_list_pdu);
                if has_h_ann && let Ok((p2, _)) = self.apply_decodings(a_payload.clone(), &pre_ann, Some(&header), false) {
                        a_payload = p2;
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
        
        eprintln!("DEBUG: apply_pdu mid={:?}, ce={:?}, total={}, chunk={}, p_len={}", header.message_id, header.content_encoding, total_chunks, chunk_id, payload.len());

        let session = self.sessions.entry(session_key.clone()).or_insert_with(|| Session {
            chunks: HashMap::new(),
            headers: Vec::new(),
            total_chunks,
        });

        let existing = session.chunks.get(&chunk_id);
        if existing.is_none() || pdu_quality > existing.unwrap().1 {
            session.chunks.insert(chunk_id, (payload, pdu_quality));
            session.headers.push(header);
        }

        let completed = if session.chunks.len() as u32 == session.total_chunks {
            true
        } else {
            // Check for RaptorQ in pre-boundary encodings for early reassembly
            let mut rq_k = None;
            for h in &session.headers {
                if let Some(ce) = &h.content_encoding {
                    let (pre, _, _) = split_encs(&ce.0);
                    for enc in pre {
                        if let ContentEncoding::RaptorQ(rq_len, mtu, _) = enc {
                            rq_k = Some(rq_len.div_ceil(mtu as usize));
                            break;
                        }
                    }
                }
                if rq_k.is_some() { break; }
            }
            if let Some(k) = rq_k {
                session.chunks.len() >= k
            } else {
                false
            }
        };

        if completed {
            self.complete_message(session_key);
        }
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    fn handle_announcement(&mut self, src: Option<String>, payload: &[u8]) {
        if let Ok(h) = minicbor::decode::<Header>(payload)
            && let (Some(mid), Some(ce)) = (h.message_id, h.content_encoding) {
                let ice = ce_to_list(&ce);
                self.announcements.insert((src, mid), ice);
        }
    }



    fn strip_post_h_encodings(&self, h: &mut Header) {
        if let Some(ce) = &mut h.content_encoding
            && let Some(pos) = ce.0.iter().position(|e| matches!(e, ContentEncoding::H)) {
                let pre = ce.0[..pos].to_vec();
                if pre.is_empty() {
                    h.content_encoding = None;
                } else {
                    h.content_encoding = Some(EncodingList(pre));
                }
        }
    }


    fn apply_decodings(&self, mut data: Bytes, encodings: &[ContentEncoding], _header: Option<&Header>, _header_already_unpacked: bool) -> Result<(Bytes, usize)> {
        let mut quality = 0;
        for enc in encodings.iter().rev() {
            match enc {
                ContentEncoding::H => {}
                ContentEncoding::Identity => {}
                ContentEncoding::Gzip => data = Bytes::from(gzip_decompress(&data)?),
                ContentEncoding::Brotli => data = Bytes::from(brotli_decompress(&data)?),
                ContentEncoding::Lzma => data = Bytes::from(lzma_decompress(&data)?),
                ContentEncoding::Crc32 => {
                    if data.len() < 4 { bail!("too short for crc32"); }
                    let payload = data.slice(..data.len()-4);
                    let expected = &data[data.len()-4..];

                    if crc32_std(&payload) != expected { 
                        bail!("crc32 fail"); 
                    }
                    data = payload;
                    quality += 1000;
                }
                ContentEncoding::Crc16 => {
                    if data.len() < 2 { bail!("too short for crc16"); }
                    let payload = data.slice(..data.len()-2);
                    let expected = &data[data.len()-2..];

                    if crc16_ccitt(&payload) != expected { 
                        bail!("crc16 fail"); 
                    }
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
                    data = Bytes::from(rq_decode(vec![data], *rq_len, *mtu)?);
                    quality += 10;
                }
                ContentEncoding::RaptorQDynamic(mtu, _) => {
                    let rq_len = data.len();
                    data = Bytes::from(rq_decode(vec![data], rq_len, *mtu)?);
                    quality += 10;
                }
                ContentEncoding::Chunk(_) => {}
                ContentEncoding::OtherString(_) | ContentEncoding::OtherInteger(_) | ContentEncoding::Deflate => {}
            }
        }
        Ok((data, quality))
    }

    fn complete_message(&mut self, key: (Option<String>, u32)) {
        let Some(session) = self.sessions.remove(&key) else { return; };
        
        let mut merged = session.header_from_first();
        for h in &session.headers[1..] {
            let _ = merged.merge(h);
        }
        merged.strip_chunking();
        
        let mut sorted_keys: Vec<_> = session.chunks.keys().cloned().collect();
        sorted_keys.sort();
        let segments: Vec<Bytes> = sorted_keys.iter().map(|k| session.chunks.get(k).unwrap().0.clone()).collect();
        
        let ce_list = merged.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
        let (pre, _post, _has_h) = split_encs(&ce_list);
        
        let mut pre_fixed = pre.clone();

        // Calculate Last Splitting Index (lsi) to identify which encodings were PDU-level
        // and thus already handled in Phase 1 (apply_pdu_level_decodings).
        // We must NOT re-apply these.
        let mut lsi = -1;
        for (i, e) in pre.iter().enumerate() {
            if matches!(e, ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_) | ContentEncoding::RaptorQ(_, _, _) | ContentEncoding::ReedSolomon(_, _)) {
                lsi = i as i32;
            }
        }
        
        // If implicit chunking (Chunk hidden) and no packet-based encodings (RQ/RS/Repeat),
        // we must join segments before applying stream encodings (gzip/etc).
        // Similar to apply_pdu_level_decodings logic.
        let has_packet_enc = pre_fixed.iter().any(|e| matches!(e, ContentEncoding::Chunk(_) | ContentEncoding::Repeat(_) | ContentEncoding::RaptorQ(_, _, _) | ContentEncoding::ReedSolomon(_, _)));
        
        if !has_packet_enc && !segments.is_empty() {
            let is_frag = merged.total_chunks.unwrap_or(1) > 1 || merged.chunk_id.unwrap_or(0) > 0;
            if !is_frag && segments.len() == 1 {
                 // Phase 1 handled it completely.
                 pre_fixed.clear();
            } else {
                // Append Chunk encoding so it runs first in reverse (joining segments)
                pre_fixed.push(ContentEncoding::Chunk(0));
            }
        } else if lsi != -1 {
            // Filter pre_fixed: only keep encodings BEFORE or AT lsi.
            if (lsi as usize) < pre_fixed.len() {
                 pre_fixed.truncate((lsi + 1) as usize);
            }
        } else {
             // lsi == -1 but has_packet_enc is TRUE? 
             // Should not happen as lsi logic matches has_packet_enc logic.
             // Unless encodings definition differs?
             // But if so, we fall through here.
             // Replicate the check just in case.
             let is_frag = merged.total_chunks.unwrap_or(1) > 1 || merged.chunk_id.unwrap_or(0) > 0;
             if !is_frag && segments.len() == 1 {
                 pre_fixed.clear();
             }
        }

        // 1. Session-level reassembly/decoding
        let mut data = match self.apply_decodings_multi(segments, &pre_fixed, Some(&merged), false) {
            Ok((d, _)) => d,
            Err(e) => {
                eprintln!("DEBUG: complete_message apply_decodings_multi failed: {e}");
                return;
            }
        };

        // 2. Handle nesting (if the reassembled content is another HQFBP PDU)
        if let Ok((h_inner, p_inner)) = unpack(data.clone()) 
            && h_inner.message_id.is_some() {
                let inner_ce = h_inner.content_encoding.as_ref().map(ce_to_list).unwrap_or_default();
                let (pre_inner, _, _) = split_encs(&inner_ce);
                if let Ok((p_inner2, _)) = self.apply_decodings(p_inner, &pre_inner, Some(&h_inner), false) {
                    merged = h_inner;
                    data = p_inner2;
                }
        }

        // 3. Truncate to file size if specified
        if let Some(size) = merged.file_size
            && data.len() > size as usize {
                data = data.slice(..size as usize);
        }
        
        // 4. Update final header: remove 'h' and all encodings up to reassembly limit
        // In this implementation, we strip all pre-boundary encodings and 'h'
        if let Some(ce) = &mut merged.content_encoding {
            let cur_list = ce.0.clone();
            let mut new_ce = Vec::new();
            let mut found_h = false;
            for e in cur_list {
                if matches!(e, ContentEncoding::H) {
                    found_h = true;
                    continue;
                }
                if !found_h { continue; } // Skip everything before and including H
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
        
        // When a message is completed, we should clear the heuristic reassembly buffer
        // to avoid "poisoning" subsequent messages with fragments from this one.
        self.not_yet_decoded = Vec::new();
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
