use crate::codec::*;
use crate::{ContentEncoding, Header, MediaType, pack};
use anyhow::Result;
use bytes::Bytes;

pub struct PDUGenerator {
    src_callsign: Option<String>,
    dst_callsign: Option<String>,
    encodings: Vec<ContentEncoding>,
    announcement_encoder: Option<Box<PDUGenerator>>,
    next_msg_id: u32,
}

impl PDUGenerator {
    pub fn new(
        src_callsign: Option<String>,
        dst_callsign: Option<String>,
        encodings: Option<Vec<ContentEncoding>>,
        announcement_encodings: Option<Vec<ContentEncoding>>,
        initial_msg_id: u32,
    ) -> Self {
        let ann_encoder = announcement_encodings.map(|e| {
            Box::new(PDUGenerator::new(
                src_callsign.clone(),
                dst_callsign.clone(),
                Some(e),
                None,
                initial_msg_id,
            ))
        });

        Self {
            src_callsign,
            dst_callsign,
            encodings: encodings.unwrap_or_default(),
            announcement_encoder: ann_encoder,
            next_msg_id: initial_msg_id,
        }
    }

    /// Automatically configures chunking so that generated PDUs do not exceed `max_pdu_size`.
    pub fn with_max_pdu_size(mut self, max_pdu_size: usize) -> Result<Self> {
        let chunk_size = calculate_max_chunk_size(
            max_pdu_size,
            &self.encodings,
            self.src_callsign.as_deref(),
            self.dst_callsign.as_deref(),
        )?;

        if !self
            .encodings
            .iter()
            .any(|e| matches!(e, ContentEncoding::H))
        {
            self.encodings.push(ContentEncoding::H);
        }
        let h_idx = self
            .encodings
            .iter()
            .position(|e| matches!(e, ContentEncoding::H))
            .unwrap();

        let mut existing_chunk_idx = None;
        for (i, enc) in self.encodings[..h_idx].iter().enumerate() {
            if matches!(enc, ContentEncoding::Chunk(_)) {
                existing_chunk_idx = Some(i);
                break;
            }
        }

        if let Some(idx) = existing_chunk_idx {
            if let ContentEncoding::Chunk(ref mut cur_size) = self.encodings[idx] {
                *cur_size = (*cur_size).min(chunk_size);
            }
        } else {
            self.encodings
                .insert(h_idx, ContentEncoding::Chunk(chunk_size));
        }

        Ok(self)
    }

    fn get_next_msg_id(&mut self) -> u32 {
        let id = self.next_msg_id;
        self.next_msg_id = self.next_msg_id.wrapping_add(1);
        id
    }

    fn apply_encodings(&self, data: Bytes, encs: &[ContentEncoding]) -> Result<Vec<Bytes>> {
        let mut current_data = data.to_vec();
        for enc in encs {
            match enc {
                ContentEncoding::H => {}
                ContentEncoding::Identity => {}
                ContentEncoding::Gzip => {
                    current_data = gzip_compress(&current_data)?;
                }
                ContentEncoding::Deflate => {
                    current_data = deflate_compress(&current_data)?;
                }
                ContentEncoding::Brotli => {
                    current_data = brotli_compress(&current_data)?;
                }
                ContentEncoding::Lzma => {
                    current_data = lzma_compress(&current_data)?;
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
                    current_data = rs_encode(&current_data, *n, *k)?;
                }
                ContentEncoding::RaptorQ(rq_len, mtu, repairs) => {
                    let res = rq_encode(&current_data, *rq_len, *mtu, *repairs)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::RaptorQDynamic(mtu, repairs) => {
                    let rq_len = current_data.len();
                    let res = rq_encode(&current_data, rq_len, *mtu, *repairs)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::LT(len, mtu, repairs) => {
                    let res = lt_encode(&current_data, *len, *mtu, *repairs)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::LTDynamic(mtu, repairs) => {
                    let len = current_data.len();
                    let res = lt_encode(&current_data, len, *mtu, *repairs)?;
                    return Ok(res.into_iter().collect());
                }
                ContentEncoding::Conv(k, rate) => {
                    current_data = conv_encode(&current_data, *k, rate)?;
                }
                ContentEncoding::Scrambler(poly) => {
                    current_data = scr_xor(&current_data, *poly);
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
                            self.next_msg_id = self.next_msg_id.wrapping_add(1);
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

                    new_chunks.push(pack(&header, chunk_data)?);
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
            final_pdus.extend(ann_pdus);
        }

        final_pdus.extend(current_chunks);
        Ok(final_pdus)
    }
}

/// Calculates the maximum payload chunk size such that the final generated PDUs
/// (including CBOR headers and post-`H` FEC expansions) will not exceed `max_pdu_size`.
pub fn calculate_max_chunk_size(
    max_pdu_size: usize,
    encodings: &[ContentEncoding],
    src_callsign: Option<&str>,
    dst_callsign: Option<&str>,
) -> Result<usize> {
    let mut resolved = encodings.to_vec();
    if !resolved.iter().any(|e| matches!(e, ContentEncoding::H)) {
        resolved.push(ContentEncoding::H);
    }

    let h_idx = resolved
        .iter()
        .position(|e| matches!(e, ContentEncoding::H))
        .unwrap();
    let post_h = &resolved[h_idx + 1..];

    let mut available = max_pdu_size;
    for enc in post_h.iter().rev() {
        match enc {
            ContentEncoding::Crc32 => {
                available = available
                    .checked_sub(4)
                    .ok_or_else(|| anyhow::anyhow!("max_pdu_size too small for CRC32"))?;
            }
            ContentEncoding::Crc16 => {
                available = available
                    .checked_sub(2)
                    .ok_or_else(|| anyhow::anyhow!("max_pdu_size too small for CRC16"))?;
            }
            ContentEncoding::ReedSolomon(n, k) => {
                if *n == 0 || *k == 0 || *k >= *n {
                    anyhow::bail!("Invalid RS parameters ({n}, {k})");
                }
                let num_blocks = available / n;
                if num_blocks == 0 {
                    anyhow::bail!("max_pdu_size {available} is smaller than RS block length {n}");
                }
                available = num_blocks * k;
            }
            ContentEncoding::Conv(_k, rate) if rate == "1/2" => {
                if available < 2 {
                    anyhow::bail!("max_pdu_size too small for Conv(1/2)");
                }
                available = (available - 1) / 2;
            }
            _ => {}
        }
    }

    // Add Chunk placeholder to encodings for accurate header size estimation
    if !resolved
        .iter()
        .take(h_idx)
        .any(|e| matches!(e, ContentEncoding::Chunk(_)))
    {
        resolved.insert(h_idx, ContentEncoding::Chunk(available));
    }

    // Conservative header overhead estimation using a multi-chunk header template
    let dummy_header = Header {
        message_id: Some(65535),
        src_callsign: src_callsign.map(|s| s.to_string()),
        dst_callsign: dst_callsign.map(|s| s.to_string()),
        content_encoding: Some(crate::EncodingList(resolved)),
        file_size: Some(10_000_000),
        chunk_id: Some(1000),
        original_message_id: Some(65535),
        total_chunks: Some(1000),
        payload_size: Some(available as u64),
        ..Default::default()
    };
    let dummy_pack = pack(&dummy_header, &vec![0u8; available.min(1024)])?;
    let header_overhead = (dummy_pack.len() - available.min(1024)) + 4; // +4 bytes safety margin

    if available <= header_overhead {
        anyhow::bail!(
            "max_pdu_size leaves no room for payload after header overhead ({header_overhead} bytes)"
        );
    }

    Ok(available - header_overhead)
}
