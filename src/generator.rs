use crate::codec::*;
use crate::error::{HqfbpError, Result};
use crate::{ContentEncoding, Header, MediaType, pack};
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

    fn apply_encodings(&self, data: Bytes, encs: &[ContentEncoding]) -> Result<Vec<Bytes>> {
        let mut current_data = data.to_vec();
        for enc in encs {
            match enc {
                ContentEncoding::H => {}
                ContentEncoding::Identity => {}
                ContentEncoding::Gzip => {
                    current_data = gzip_compress(&current_data).map_err(HqfbpError::Codec)?;
                }
                ContentEncoding::Brotli => {
                    current_data = brotli_compress(&current_data).map_err(HqfbpError::Codec)?;
                }
                ContentEncoding::Lzma => {
                    current_data = lzma_compress(&current_data).map_err(HqfbpError::Codec)?;
                }
                ContentEncoding::Crc16 => {
                    let crc = crc16_ccitt(&current_data);
                    current_data.extend_from_slice(&crc);
                }
                ContentEncoding::Crc32 => {
                    let crc = crc32_std(&current_data);
                    current_data.extend_from_slice(&crc);
                }
                ContentEncoding::ReedSolomon(n, k) => {
                    current_data = rs_encode(&current_data, *n, *k).map_err(HqfbpError::Codec)?;
                }
                ContentEncoding::RaptorQ(rq_len, mtu, repairs) => {
                    let res = rq_encode(&current_data, *rq_len, *mtu, *repairs)
                        .map_err(HqfbpError::Codec)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::RaptorQDynamic(mtu, repairs) => {
                    let rq_len = current_data.len();
                    let res = rq_encode(&current_data, rq_len, *mtu, *repairs)
                        .map_err(HqfbpError::Codec)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::RaptorQDynamicPercent(mtu, percent) => {
                    let rq_len = current_data.len();
                    let repairs = 1.max(
                        (rq_len as f32 * (*percent as f32) / (100.0 * (*mtu as f32))).ceil() as u32,
                    );
                    let res = rq_encode(&current_data, rq_len, *mtu, repairs)
                        .map_err(HqfbpError::Codec)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::LT(len, mtu, repairs) => {
                    let res = lt_encode(&current_data, *len, *mtu, *repairs)
                        .map_err(HqfbpError::Codec)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::LTDynamic(mtu, repairs) => {
                    let len = current_data.len();
                    let res =
                        lt_encode(&current_data, len, *mtu, *repairs).map_err(HqfbpError::Codec)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::Conv(k, rate) => {
                    current_data =
                        conv_encode(&current_data, *k, rate).map_err(HqfbpError::Codec)?;
                }
                ContentEncoding::Scrambler(poly, seed) => {
                    current_data = scr_xor(&current_data, *poly, *seed);
                }
                ContentEncoding::Golay(_, _) => {
                    current_data = golay_encode(&current_data).map_err(HqfbpError::Codec)?;
                }
                _ => {}
            }
        }
        Ok(vec![Bytes::from(current_data)])
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

        self.last_min_header_size = usize::MAX;
        self.last_max_header_size = 0;
        self.last_total_header_size = 0;

        let mut header_template = Header {
            file_size: Some(file_size),
            src_callsign: self.src_callsign.clone(),
            dst_callsign: self.dst_callsign.clone(),
            ..Default::default()
        };
        header_template.set_media_type(media_type);

        for i in 0..full_encs.len() {
            let enc = full_encs[i].clone();
            if matches!(enc, ContentEncoding::H) {
                let total_chunks = current_chunks.len() as u32;
                let mut new_chunks = Vec::new();
                for (idx, chunk_data) in current_chunks.iter().enumerate() {
                    let mut header = header_template.clone();

                    let msg_id = if idx == 0 {
                        let id = data_orig_id;
                        if self.next_msg_id == id {
                            self.next_msg_id += 1;
                        }
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
                        header.set_media_type(None);
                    }

                    header.content_encoding = Some(crate::EncodingList(full_encs.clone()));
                    header.payload_size = Some(chunk_data.len() as u64);

                    let packed = pack(&header, chunk_data)?;
                    let h_size = packed.len() - chunk_data.len();
                    self.last_min_header_size = self.last_min_header_size.min(h_size);
                    self.last_max_header_size = self.last_max_header_size.max(h_size);
                    self.last_total_header_size += h_size;

                    new_chunks.push(packed);
                }
                current_chunks = new_chunks;
            } else if let ContentEncoding::Chunk(size) = enc {
                let mut next_chunks = Vec::new();
                for chunk in current_chunks {
                    let mut pos = 0;
                    while pos < chunk.len() {
                        let end = (pos + size).min(chunk.len());
                        next_chunks.push(chunk.slice(pos..end));
                        pos = end;
                    }
                }
                current_chunks = next_chunks;
            } else if let ContentEncoding::Repeat(count) = enc {
                let mut next_chunks = Vec::new();
                for chunk in current_chunks {
                    for _ in 0..count {
                        next_chunks.push(chunk.clone());
                    }
                }
                current_chunks = next_chunks;
            } else {
                let mut next_chunks = Vec::new();
                for c in &current_chunks {
                    let mut actual_enc = enc.clone();
                    if let ContentEncoding::RaptorQDynamic(mtu, repairs) = actual_enc {
                        actual_enc = ContentEncoding::RaptorQ(c.len(), mtu, repairs);
                        // Update full_encs for the header
                        full_encs[i] = actual_enc.clone();
                    } else if let ContentEncoding::LTDynamic(mtu, repairs) = actual_enc {
                        actual_enc = ContentEncoding::LT(c.len(), mtu, repairs);
                        full_encs[i] = actual_enc.clone();
                    }
                    let transformed = self.apply_encodings(c.clone(), &[actual_enc])?;
                    next_chunks.extend(transformed);
                }
                current_chunks = next_chunks;
            }
        }

        let mut final_pdus = Vec::new();
        if let (Some(ann_enc), Some(aid)) = (self.announcement_encoder.as_mut(), ann_msg_id) {
            ann_enc.next_msg_id = aid;
            let mut ann_header = Header {
                message_id: Some(aid),
                ..header_template.clone()
            };
            ann_header.set_media_type(Some(MediaType::Type(
                "application/vnd.hqfbp+cbor".to_string(),
            )));

            let mut announcement_body = Header {
                message_id: Some(data_orig_id),
                content_encoding: Some(crate::EncodingList(full_encs.clone())),
                ..Default::default()
            };
            announcement_body.set_media_type(header_template.media_type());

            let body_bytes = minicbor::to_vec(&announcement_body).unwrap();
            let ann_pdus = ann_enc.generate(
                &body_bytes,
                Some(MediaType::Type("application/vnd.hqfbp+cbor".to_string())),
            )?;
            let (a_min, a_max, a_total) = ann_enc.last_header_stats();
            self.last_min_header_size = self.last_min_header_size.min(a_min);
            self.last_max_header_size = self.last_max_header_size.max(a_max);
            self.last_total_header_size += a_total;

            final_pdus.extend(ann_pdus);
        }

        self.last_min_header_size = if self.last_min_header_size == usize::MAX {
            0
        } else {
            self.last_min_header_size
        };

        final_pdus.extend(current_chunks);
        Ok(final_pdus)
    }
}
