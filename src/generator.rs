use crate::codec::*;
use crate::error::{HqfbpError, Result};
use crate::{ContentEncoding, Header, MediaType};
use bytes::Bytes;

pub struct PDUGenerator {
    src_callsign: Option<String>,
    dst_callsign: Option<String>,
    max_payload_size: Option<usize>,
    encodings: Vec<ContentEncoding>,
    announcement_encoder: Option<Box<PDUGenerator>>,
    next_msg_id: u32,
    last_min_header_size: usize,
    last_max_header_size: usize,
    last_total_header_size: usize,
}

impl PDUGenerator {
    pub fn new(
        src_callsign: Option<String>,
        dst_callsign: Option<String>,
        max_payload_size: Option<usize>,
        encodings: Option<Vec<ContentEncoding>>,
        announcement_encodings: Option<Vec<ContentEncoding>>,
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
            last_min_header_size: 0,
            last_max_header_size: 0,
            last_total_header_size: 0,
        }
    }

    pub fn last_header_stats(&self) -> (usize, usize, usize) {
        (
            self.last_min_header_size,
            self.last_max_header_size,
            self.last_total_header_size,
        )
    }

    fn get_next_msg_id(&mut self) -> u32 {
        let id = self.next_msg_id;
        self.next_msg_id += 1;
        id
    }

    pub fn resolve_encodings(&self) -> Vec<ContentEncoding> {
        let mut encs = self.encodings.clone();

        let has_boundary = encs.iter().any(|e| matches!(e, ContentEncoding::H));
        if !has_boundary {
            encs.push(ContentEncoding::H);
        }

        let boundary_idx = encs
            .iter()
            .position(|e| matches!(e, ContentEncoding::H))
            .unwrap();
        let pre = &encs[..boundary_idx];

        let has_chunk = pre.iter().any(|e| matches!(e, ContentEncoding::Chunk(_)));

        if !has_chunk && let Some(limit) = self.max_payload_size {
            let mut new_encs = Vec::new();
            for (i, e) in encs.iter().enumerate() {
                if i == boundary_idx {
                    new_encs.push(ContentEncoding::Chunk(limit));
                }
                new_encs.push(e.clone());
            }
            return new_encs;
        }

        encs
    }

    pub fn generate(&mut self, data: &[u8], media_type: Option<MediaType>) -> Result<Vec<Bytes>> {
        let file_size = data.len() as u64;
        let mut full_encs = self.resolve_encodings();

        let mut current_chunks = vec![Bytes::copy_from_slice(data)];

        let ann_msg_id = if self.announcement_encoder.is_some() {
            Some(self.get_next_msg_id())
        } else {
            None
        };
        let data_orig_id = self.next_msg_id;

        // Initialize Context
        let mut ctx = EncodingContext {
            src_callsign: self.src_callsign.clone(),
            dst_callsign: self.dst_callsign.clone(),
            next_msg_id: self.next_msg_id,
            original_message_id: Some(data_orig_id),
            last_min_header_size: usize::MAX,
            last_max_header_size: 0,
            last_total_header_size: 0,
            file_size: Some(file_size),
            media_type: media_type.clone(),
            encodings: full_encs.clone(),
            current_index: 0,
            ..Default::default()
        };

        for (i, enc) in full_encs.iter().enumerate() {
            ctx.current_index = i;
            current_chunks = enc.encode(current_chunks, &mut ctx)?;
        }

        // Synchronize state back from context
        self.next_msg_id = ctx.next_msg_id;
        self.last_min_header_size = if ctx.last_min_header_size == usize::MAX {
            0
        } else {
            ctx.last_min_header_size
        };
        self.last_max_header_size = ctx.last_max_header_size;
        self.last_total_header_size = ctx.last_total_header_size;

        let mut final_pdus = Vec::new();
        if let (Some(ann_enc), Some(aid)) = (self.announcement_encoder.as_mut(), ann_msg_id) {
            ann_enc.next_msg_id = aid;
            // Announcement generation uses separate logic (recursive PDUGenerator),
            // but we need to pass the updated encoding list (if dynamic resolution happened).
            // The announcement body describes the data encoding.
            // ctx.encodings contains the resolved list.

            // Re-create the header template logic for announcement?
            // Existing logic:
            /*
            let mut ann_header = Header { ... };
            let mut announcement_body = Header {
                message_id: Some(data_orig_id),
                content_encoding: Some(crate::EncodingList(full_encs.clone())), // Uses full_encs
                 ...
            };
            */
            // We should use ctx.encodings which has resolved values.

            let mut header_template = Header {
                file_size: Some(file_size),
                src_callsign: self.src_callsign.clone(),
                dst_callsign: self.dst_callsign.clone(),
                ..Default::default()
            };
            header_template.set_media_type(media_type); // Needed? Announcement logic uses it?

            let mut announcement_body = Header {
                message_id: Some(data_orig_id),
                // Use resolved encodings from context
                content_encoding: Some(crate::EncodingList(ctx.encodings.clone())),
                ..Default::default()
            };
            // Announcement body carries the media type of the inner data
            announcement_body.set_media_type(header_template.media_type());

            let body_bytes = minicbor::to_vec(&announcement_body)
                .map_err(|e| HqfbpError::Other(format!("CBOR encode failed: {e}")))?;

            let ann_pdus = ann_enc.generate(
                &body_bytes,
                Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())),
            )?;
            let (a_min, a_max, a_total) = ann_enc.last_header_stats();

            // Update stats
            self.last_min_header_size = self.last_min_header_size.min(a_min);
            self.last_max_header_size = self.last_max_header_size.max(a_max);
            self.last_total_header_size += a_total;

            final_pdus.extend(ann_pdus);
        }

        final_pdus.extend(current_chunks);
        Ok(final_pdus)
    }
}
