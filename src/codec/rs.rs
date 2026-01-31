use crate::codec::{Codec, CodecContext};
use crate::error::CodecError;
use bytes::Bytes;
use reed_solomon::Decoder as RSDecoder;
use reed_solomon::Encoder as RSEncoder;

pub struct ReedSolomon {
    n: usize,
    k: usize,
}

impl ReedSolomon {
    pub fn new(n: usize, k: usize) -> Self {
        Self { n, k }
    }
}

pub fn rs_encode(data: &[u8], n: usize, k: usize) -> Result<Vec<u8>, CodecError> {
    if n > 255 || k == 0 || k > n {
        return Err(CodecError::InvalidParameters(format!(
            "Invalid RS parameters: n={n}, k={k}"
        )));
    }
    let ecc_len = n - k;
    let encoder = RSEncoder::new(ecc_len);
    let mut encoded = Vec::with_capacity(data.len().div_ceil(k) * n);

    for chunk in data.chunks(k) {
        let mut block_for_lib = chunk.to_vec();
        block_for_lib.resize(k, 0);

        let lib_pad = 255 - n;
        let mut full_codeword_for_lib = vec![0u8; lib_pad];
        full_codeword_for_lib.extend_from_slice(&block_for_lib);

        let encoded_full_codeword = encoder.encode(&full_codeword_for_lib);
        let parity = &encoded_full_codeword[255 - ecc_len..];

        encoded.extend_from_slice(&block_for_lib);
        encoded.extend_from_slice(parity);
    }
    Ok(encoded)
}

pub fn rs_decode(data: &[u8], n: usize, k: usize) -> Result<(Vec<u8>, usize), CodecError> {
    if n > 255 || k == 0 || k >= n {
        return Err(CodecError::InvalidParameters(format!(
            "Invalid RS parameters: n={n}, k={k}"
        )));
    }
    let ecc_len = n - k;
    let decoder = RSDecoder::new(ecc_len);
    let mut decoded = Vec::with_capacity(data.len().checked_div(n).unwrap_or(0) * k);
    let mut total_corrected = 0;

    let mut i = 0;
    while i < data.len() {
        let remaining = data.len() - i;
        let block_len = if remaining >= n { n } else { remaining };
        let chunk = &data[i..i + block_len];
        i += block_len;

        if block_len <= ecc_len {
            return Err(CodecError::InsufficientData(None));
        }

        let lib_pad = 255 - n;
        let mut full_codeword = vec![0u8; lib_pad];
        full_codeword.extend_from_slice(chunk);

        if full_codeword.len() < 255 {
            let needed = 255 - full_codeword.len();
            let parity_part = &full_codeword[full_codeword.len() - ecc_len..];
            let mut new_fw = full_codeword[..full_codeword.len() - ecc_len].to_vec();
            new_fw.resize(new_fw.len() + needed, 0);
            new_fw.extend_from_slice(parity_part);
            full_codeword = new_fw;
        }

        match decoder.correct_err_count(&full_codeword, None) {
            Ok((corrected, err_count)) => {
                let dpart = &corrected[lib_pad..lib_pad + k];
                decoded.extend_from_slice(dpart);
                total_corrected += err_count;
            }
            Err(e) => {
                return Err(CodecError::FecFailure(format!("RS decode failed: {e:?}")));
            }
        }
    }
    Ok((decoded, total_corrected))
}

impl Codec for ReedSolomon {
    fn encode(&self, data: Vec<Bytes>, _ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let mut res = Vec::new();
        for chunk in data {
            res.push(Bytes::from(rs_encode(&chunk, self.n, self.k)?));
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        let mut quality = 0.0;
        for (ctx, chunk) in chunks {
            let (d, corrected) = rs_decode(&chunk, self.n, self.k)?;
            res.push((ctx, Bytes::from(d)));
            let num_blocks = chunk.len() / self.n;
            let max_correctable = ((self.n - self.k) / 2) * num_blocks;
            quality += max_correctable.saturating_sub(corrected) as f32;
        }
        Ok((res, quality))
    }
}
