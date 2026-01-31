use crate::error::{HqfbpError, Result};
use crate::{ContentEncoding, EncodingList, Header, MediaType};
use bytes::Bytes;
use std::collections::{HashMap, VecDeque};

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
    encoding_factory: crate::codec::CodecFactory,
}

struct Session {
    chunks: HashMap<u32, (Bytes, usize)>, // chunk_id -> (payload, quality)
    headers: Vec<Header>,

    total_chunks: u32,
}

fn ce_to_list(ce: &EncodingList) -> Vec<ContentEncoding> {
    ce.0.clone()
}

// Helper to split encodings at the boundary (header encoding)
fn split_at_boundary(
    factory: &crate::codec::CodecFactory,
    encs: &[ContentEncoding],
) -> (
    Vec<ContentEncoding>,
    Vec<ContentEncoding>,
    Option<ContentEncoding>,
) {
    if let Some(pos) = encs
        .iter()
        .rposition(|e| factory.get_encoding(e).is_header())
    {
        (
            encs[..pos].to_vec(),
            encs[pos + 1..].to_vec(),
            Some(encs[pos].clone()),
        )
    } else {
        (encs.to_vec(), Vec::new(), None)
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
            encoding_factory: crate::codec::CodecFactory::new(),
        }
    }

    fn apply_pdu_level_decodings(
        &self,
        header: &mut Header,
        payload: Bytes,
    ) -> Result<(Bytes, usize)> {
        let ce = header
            .content_encoding
            .as_ref()
            .map(ce_to_list)
            .unwrap_or_default();
        let (pre, _, _) = split_at_boundary(&self.encoding_factory, &ce);

        // Late Truncation: Fix the payload size based on the header BEFORE decodings.
        let current_payload = if let Some(payload_size) = header.payload_size {
            if payload.len() > payload_size as usize {
                payload.slice(0..payload_size as usize)
            } else {
                payload
            }
        } else {
            payload
        };

        let mut to_apply = Vec::new();
        for (_idx, enc) in pre.iter().enumerate().rev() {
            match enc {
                ContentEncoding::ReedSolomon(_, _)
                | ContentEncoding::Crc16
                | ContentEncoding::Crc32
                | ContentEncoding::Conv(_, _)
                | ContentEncoding::Golay(_, _)
                | ContentEncoding::Scrambler(_, _)
                | ContentEncoding::Chunk(_)
                | ContentEncoding::Asm(_)
                | ContentEncoding::PostAsm(_) => {
                    to_apply.push(enc.clone());
                }
                _ => break,
            }
        }
        if !to_apply.is_empty() {
            // Update header to remove what we apply
            if let Some(ce_obj) = &mut header.content_encoding {
                let n = to_apply.len();
                if n >= ce_obj.0.len() {
                    header.content_encoding = None;
                } else {
                    let new_len = ce_obj.0.len() - n;
                    ce_obj.0.truncate(new_len);
                }
            }

            let mut to_apply_rev = to_apply.clone();
            // We initially collected in reverse order (Inner-most to Outer-most of the 'pre' stack).
            // We reverse it back to 'Forward' order (Outer-most of 'pre' to Inner-most)
            // because `apply_decodings` (via `apply_decodings_multi`) iterates in `rev()` order.
            //
            // Example: pre = [Enc1, Enc2] (Enc2 wraps Enc1).
            // 1. Loop above iterates rev(): Enc2, then Enc1.
            // 2. to_apply = [Enc2, Enc1].
            // 3. to_apply_rev = [Enc1, Enc2].
            // 4. apply_decodings receives [Enc1, Enc2].
            // 5. apply_decodings_multi iterates rev(): Enc2 (decoded first), then Enc1.
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

        // 1. Phase 1: Recovery and PDU-level decoding
        // Peek with standard HQFBP header (H)
        let peek_unpack = self
            .encoding_factory
            .get_encoding(&ContentEncoding::H)
            .unpack_header(b_data.clone());

        if let Ok((h_peek, p_peek)) = &peek_unpack {
            let src_c = h_peek.src_callsign.clone();
            let m_id = h_peek.message_id;

            let target_id = h_peek.original_message_id.or(m_id);
            let ce_list = if let Some(mid) = target_id {
                self.announcements
                    .get(&(src_c.clone(), mid))
                    .cloned()
                    .or_else(|| h_peek.content_encoding.as_ref().map(ce_to_list))
            } else {
                h_peek.content_encoding.as_ref().map(ce_to_list)
            };

            if let Some(ce) = ce_list {
                let (_, post, boundary) = split_at_boundary(&self.encoding_factory, &ce);
                if let Some(_b) = &boundary {
                    // Boundary is NOT inserted into post, unpack_header handles it.
                }

                match self.apply_decodings(b_data.clone(), &post, Some(h_peek), false) {
                    Ok((clean_pdu, q)) => {
                        if let Some(b) = &boundary
                            && let Ok((mut h2, mut p2)) = self
                                .encoding_factory
                                .get_encoding(b)
                                .unpack_header(clean_pdu)
                        {
                            let match_ok = if let Some(ps) = h2.payload_size {
                                if p2.len() >= ps as usize {
                                    if p2.len() > ps as usize {
                                        p2 = p2.slice(0..ps as usize);
                                    }
                                    true
                                } else {
                                    false
                                }
                            } else {
                                true
                            };

                            if match_ok {
                                self.strip_post_boundary_encodings(&mut h2);
                                if let Ok((p3, q_gain)) =
                                    self.apply_pdu_level_decodings(&mut h2, p2.clone())
                                {
                                    header = Some(h2);
                                    payload = Some(p3);
                                    pdu_quality = q + q_gain;
                                }
                            }
                        }
                    }
                    Err(_e) => {}
                }
            } else {
                header = Some(h_peek.clone());
                payload = Some(p_peek.clone());
            }
        }

        // 2. Phase 2: Heuristic Recovery
        if header.is_none() || payload.is_none() {
            let ann_keys: Vec<_> = self.announcements.keys().cloned().collect();
            let mut reclaimed_any = false;

            for key in ann_keys {
                let ann_encs = self.announcements.get(&key).unwrap().clone();
                let (_, post, boundary) = split_at_boundary(&self.encoding_factory, &ann_encs);
                if let Some(_b) = boundary.clone() {
                    // Boundary is NOT inserted into post, unpack_header handles it.
                }

                let mut single_success = false;
                if let Ok((clean_pdu, q)) =
                    self.apply_decodings_multi(vec![b_data.clone()], &post, None, false)
                    && let Some(b) = &boundary
                    && let Ok((mut h2, mut p2)) = self
                        .encoding_factory
                        .get_encoding(b)
                        .unpack_header(clean_pdu)
                {
                    let size_ok = if let Some(ps) = h2.payload_size {
                        if p2.len() < ps as usize {
                            false
                        } else {
                            if p2.len() > ps as usize {
                                p2 = p2.slice(0..ps as usize);
                            }
                            true
                        }
                    } else {
                        true
                    };

                    if !size_ok {
                        single_success = false;
                    } else {
                        self.strip_post_boundary_encodings(&mut h2);
                        if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&mut h2, p2) {
                            let src_c = h2.src_callsign.clone();
                            let orig_id = h2.original_message_id.or(h2.message_id).unwrap_or(0);
                            let session_key = (src_c, orig_id);
                            let chunk_id = h2.chunk_id.unwrap_or(0);
                            let new_quality = q + q_gain;

                            let already_had_better =
                                if let Some(s) = self.sessions.get(&session_key) {
                                    if let Some(existing) = s.chunks.get(&chunk_id) {
                                        existing.1 >= new_quality
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };

                            if !already_had_better {
                                self.process_pdu(h2, p3, new_quality);
                                reclaimed_any = true;
                                single_success = true;
                            }
                        }
                    }
                }

                if single_success {
                    continue;
                }

                let has_combiner = post
                    .iter()
                    .any(|e| self.encoding_factory.get_encoding(e).is_chunking());

                if !has_combiner {
                    continue;
                }

                let mut try_list = self.not_yet_decoded.clone();
                try_list.push(b_data.clone());
                let _try_list_len = try_list.len();
                if let Ok((clean_pdu, q)) = self.apply_decodings_multi(try_list, &post, None, false)
                    && let Some(b) = &boundary
                    && let Ok((mut h2, mut p2)) = self
                        .encoding_factory
                        .get_encoding(b)
                        .unpack_header(clean_pdu)
                {
                    if let Some(ps) = h2.payload_size {
                        if p2.len() < ps as usize {
                            // Too short
                            continue;
                        } else if p2.len() > ps as usize {
                            p2 = p2.slice(0..ps as usize);
                        }
                    }
                    self.strip_post_boundary_encodings(&mut h2);
                    if let Ok((p3, q_gain)) = self.apply_pdu_level_decodings(&mut h2, p2) {
                        let src_c = h2.src_callsign.clone();
                        let orig_id = h2.original_message_id.or(h2.message_id).unwrap_or(0);
                        let session_key = (src_c, orig_id);
                        let chunk_id = h2.chunk_id.unwrap_or(0);
                        let new_quality = q + q_gain;

                        let already_had_better = if let Some(s) = self.sessions.get(&session_key) {
                            if let Some(existing) = s.chunks.get(&chunk_id) {
                                existing.1 >= new_quality
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if !already_had_better {
                            self.process_pdu(h2, p3, new_quality);
                            reclaimed_any = true;
                            self.not_yet_decoded = Vec::new();
                        }
                    }
                }
                if reclaimed_any {
                    break;
                }
            }

            if !reclaimed_any {
                self.not_yet_decoded.push(b_data);
            }
        } else if let (Some(h), Some(p)) = (header, payload) {
            self.process_pdu(h, p, pdu_quality);
        }

        self.not_yet_decoded.retain(|s| !s.is_empty());
        if self.not_yet_decoded.len() > 100 {
            self.not_yet_decoded.remove(0);
        }
    }

    fn apply_decodings(
        &self,
        data: Bytes,
        encodings: &[ContentEncoding],
        header: Option<&Header>,
        header_already_unpacked: bool,
    ) -> Result<(Bytes, usize)> {
        self.apply_decodings_multi(vec![data], encodings, header, header_already_unpacked)
    }

    fn apply_decodings_multi(
        &self,
        input: Vec<Bytes>,
        encs: &[ContentEncoding],
        _header: Option<&Header>,
        _header_already_unpacked: bool,
    ) -> Result<(Bytes, usize)> {
        let mut current = input;
        let mut quality = 0.0;

        for enc_enum in encs.iter().rev() {
            let encoder = self.encoding_factory.get_encoding(enc_enum);
            let (res, q) = encoder.try_decode(current)?;
            current = res;
            quality += q;
        }

        let mut final_data = Vec::new();
        for b in current {
            final_data.extend_from_slice(&b);
        }
        if final_data.is_empty() {
            return Err(HqfbpError::Other(
                "Empty data after multi-decoding".to_string(),
            ));
        }
        Ok((Bytes::from(final_data), quality as usize))
    }

    fn process_pdu(&mut self, header: Header, payload: Bytes, quality: usize) {
        let src_c = header.src_callsign.clone();

        self.events.push_back(Event::PDU(PDUEvent {
            header: header.clone(),
            payload: payload.clone(),
        }));

        let orig_id = header.original_message_id.or(header.message_id);

        if let Some(mid) = orig_id {
            let session_key = (src_c.clone(), mid);
            let chunk_id = header.chunk_id.unwrap_or(0);
            let total_chunks = header.total_chunks.unwrap_or(1);

            let mut completed = false;
            let mut is_ann = false;

            {
                let session = self.sessions.entry(session_key.clone()).or_insert(Session {
                    chunks: HashMap::new(),
                    headers: Vec::new(),
                    total_chunks,
                });

                if session.total_chunks == 1 && total_chunks > 1 {
                    session.total_chunks = total_chunks;
                }

                let current_quality = session.chunks.get(&chunk_id).map(|(_, q)| *q).unwrap_or(0);

                if !session.chunks.contains_key(&chunk_id) || quality > current_quality {
                    session.chunks.insert(chunk_id, (payload.clone(), quality));
                    session.headers.push(header.clone());

                    is_ann = header.media_type()
                        == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string()))
                        || header.media_type() == Some(MediaType::Format(60));

                    if session.chunks.len() as u32 >= session.total_chunks {
                        completed = true;
                    } else {
                        // Check for early completion if combiners are present
                        let ce_list = header
                            .content_encoding
                            .as_ref()
                            .map(ce_to_list)
                            .unwrap_or_default();
                        let has_combiner = ce_list
                            .iter()
                            .any(|e| self.encoding_factory.get_encoding(e).is_chunking());
                        if has_combiner && session.chunks.len() > 1 {
                            // Try to complete early
                            if self.complete_message(session_key.clone()) {
                                // Already completed and removed by complete_message
                                return;
                            }
                        }
                    }
                }
            }

            if is_ann {
                self.handle_announcement(src_c, &payload);
            }

            if completed {
                self.complete_message(session_key);
            }
        } else {
            self.events
                .push_back(Event::Message(MessageEvent { header, payload }));
        }
    }

    fn handle_announcement(&mut self, src: Option<String>, payload: &[u8]) {
        if let Ok(h) = minicbor::decode::<Header>(payload)
            && let (Some(mid), Some(ce)) = (h.message_id, h.content_encoding)
        {
            let ice = ce_to_list(&ce);
            self.announcements.insert((src, mid), ice);
        }
    }

    fn strip_post_boundary_encodings(&self, h: &mut Header) {
        if let Some(ce) = &mut h.content_encoding
            && let Some(pos) =
                ce.0.iter()
                    .rposition(|e| self.encoding_factory.get_encoding(e).is_header())
        {
            let pre = ce.0[..pos].to_vec();
            if pre.is_empty() {
                h.content_encoding = None;
            } else {
                h.content_encoding = Some(EncodingList(pre));
            }
        }
    }

    fn complete_message(&mut self, key: (Option<String>, u32)) -> bool {
        let Some(session) = self.sessions.get(&key) else {
            return false;
        };

        let mut merged = session.headers[0].clone();
        for h in &session.headers[1..] {
            let _ = merged.merge(h);
        }
        merged.strip_chunking();

        let mut sorted_keys: Vec<_> = session.chunks.keys().cloned().collect();
        sorted_keys.sort();
        let segments: Vec<Bytes> = sorted_keys
            .iter()
            .map(|k| session.chunks.get(k).unwrap().0.clone())
            .collect();

        let ce_list = merged
            .content_encoding
            .as_ref()
            .map(ce_to_list)
            .unwrap_or_default();
        let (pre, _, _boundary) = split_at_boundary(&self.encoding_factory, &ce_list);

        let pre_fixed = pre.clone();

        let data = match self.apply_decodings_multi(segments, &pre_fixed, Some(&merged), false) {
            Ok((d, _)) => d,
            Err(_e) => {
                return false;
            }
        };

        // If we reach here, reassembly succeeded.
        // Remove session now.
        let merged_for_event = merged.clone();
        self.sessions.remove(&key);

        if let Some(ce) = &mut merged.content_encoding {
            let cur_list = ce.0.clone();
            let mut new_ce = Vec::new();
            let mut found_boundary = false;
            for e in cur_list {
                if self.encoding_factory.get_encoding(&e).is_header() {
                    found_boundary = true;
                    continue;
                }
                if !found_boundary {
                    continue;
                }
                new_ce.push(e.clone());
            }
            if new_ce.is_empty() {
                merged.content_encoding = None;
            } else {
                merged.content_encoding = Some(EncodingList(new_ce));
            }
        }

        let is_ann = merged.media_type()
            == Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string()))
            || merged.media_type() == Some(MediaType::Format(60));

        if is_ann {
            let src_c = merged.src_callsign.clone();
            self.handle_announcement(src_c, &data);
        }

        let mut recursion_occurred = false;
        let inner_ce = merged
            .content_encoding
            .as_ref()
            .map(ce_to_list)
            .unwrap_or_default();

        let data_clone = data.clone();
        let (pre_inner, _, boundary_inner) = split_at_boundary(&self.encoding_factory, &inner_ce);

        if let Ok((p_inner, _)) =
            self.apply_decodings(data.clone(), &pre_inner, Some(&merged), false)
            && let Some(b_inner) = boundary_inner
            && let Ok((_h_inner, _)) = self
                .encoding_factory
                .get_encoding(&b_inner)
                .unpack_header(p_inner)
        {
            recursion_occurred = true;
        }

        if recursion_occurred {
            self.receive_bytes(&data_clone);
        } else {
            let mut final_data = data;
            if let Some(fs) = merged_for_event.file_size
                && final_data.len() > fs as usize
            {
                final_data = final_data.slice(0..fs as usize);
            }

            self.events.push_back(Event::Message(MessageEvent {
                header: merged_for_event,
                payload: final_data,
            }));
            self.not_yet_decoded = Vec::new();
        }
        true
    }

    pub fn next_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    pub fn register_announcement(
        &mut self,
        src: Option<String>,
        mid: u32,
        encs: Vec<ContentEncoding>,
    ) {
        self.announcements.insert((src, mid), encs);
    }
}
