use crate::{
    Header,
    codec::{Codec, CodecContext, CodecError},
};
use bytes::Bytes;

pub struct Ax25 {}

impl Default for Ax25 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ax25 {
    pub fn new() -> Self {
        Self {}
    }

    fn encode_address(callsign_ssid: &str, is_last: bool) -> [u8; 7] {
        let mut addr = [0x40; 7]; // Space-padded (0x20 << 1 = 0x40)
        let parts: Vec<&str> = callsign_ssid.split('-').collect();
        let callsign = parts[0].to_uppercase();
        let ssid: u8 = if parts.len() > 1 {
            parts[1].parse().unwrap_or(0)
        } else {
            0
        };

        for (i, c) in callsign.chars().take(6).enumerate() {
            addr[i] = (c as u8) << 1;
        }

        // SSID byte: 011SSIDH where H is the extension bit
        // Standard says: bit 0 is extension bit (0 = more, 1 = last)
        // bits 1-4 are SSID
        // bits 5-6 are 1
        addr[6] = 0x60 | ((ssid & 0x0F) << 1) | (if is_last { 0x01 } else { 0x00 });
        addr
    }

    fn decode_address(addr: &[u8]) -> (String, bool) {
        let mut callsign = String::new();
        for &b in addr.iter().take(6) {
            let c = (b >> 1) as char;
            if c != ' ' {
                callsign.push(c);
            }
        }
        let ssid = (addr[6] >> 1) & 0x0F;
        let is_last = (addr[6] & 0x01) != 0;
        let full = if ssid > 0 {
            format!("{}-{}", callsign, ssid)
        } else {
            callsign
        };
        (full, is_last)
    }
}

impl Codec for Ax25 {
    fn encode(&self, data: Vec<Bytes>, ctx: &mut CodecContext) -> Result<Vec<Bytes>, CodecError> {
        let src = ctx.src_callsign.as_deref().unwrap_or("N0CALL");
        let dst = ctx.dst_callsign.as_deref().unwrap_or("QST");

        let mut ax25_header = Vec::with_capacity(16);
        ax25_header.extend_from_slice(&Self::encode_address(dst, false));
        ax25_header.extend_from_slice(&Self::encode_address(src, true));
        ax25_header.push(0x03); // Control: UI frame
        ax25_header.push(0xF0); // PID: No layer 3

        let mut res = Vec::new();

        for chunk_data in data.iter() {
            let mut d = ax25_header.clone();
            // d.extend_from_slice(&packed);
            d.extend_from_slice(chunk_data);
            res.push(Bytes::from(d));
        }
        Ok(res)
    }

    fn try_decode<'a>(
        &self,
        chunks: Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>,
    ) -> Result<(Vec<(std::borrow::Cow<'a, CodecContext>, Bytes)>, f32), CodecError> {
        let mut res = Vec::new();
        for (mut ctx, c) in chunks {
            let (h, data) = self.unpack_header(c)?;
            // Update context with header info
            *ctx.to_mut() = CodecContext::from(&h);
            res.push((ctx, data));
        }
        Ok((res, 1000.0))
    }

    fn unpack_header(&self, data: Bytes) -> Result<(crate::Header, Bytes), CodecError> {
        let mut h = Header::default();
        if data.len() < 16 {
            return Err(CodecError::InsufficientData(Some(
                "AX.25 frame too short".to_string(),
            )));
        }

        // In simplified mode, we expect Dst(7), Src(7), Control(0x03), PID(0xF0)
        let control = data[14];
        let pid = data[15];

        if control != 0x03 || pid != 0xF0 {
            return Err(CodecError::FecFailure(format!(
                "Invalid AX.25 header: Control=0x{:02x}, PID=0x{:02x}",
                control, pid
            )));
        }

        // Verify extension bits
        let (_, dst_last) = Self::decode_address(&data[0..7]);
        let (_, src_last) = Self::decode_address(&data[7..14]);

        if dst_last {
            return Err(CodecError::FecFailure(
                "Invalid AX.25 header: Dst has extension bit set".to_string(),
            ));
        }
        if !src_last {
            return Err(CodecError::FecFailure(
                "Invalid AX.25 header: Src does not have extension bit set (simplified mode)"
                    .to_string(),
            ));
        }

        h.dst_callsign = Some(Self::decode_address(&data[0..7]).0);
        h.src_callsign = Some(Self::decode_address(&data[7..14]).0);

        Ok((h, data.slice(16..)))
    }

    fn is_header(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::CodecContext;
    use std::borrow::Cow;

    #[test]
    fn test_ax25_encode_decode() {
        let codec = Ax25::new();
        let mut ctx = CodecContext::default();
        ctx.src_callsign = Some("MYCALL".to_string());
        ctx.dst_callsign = Some("URCALL".to_string());

        let payload = b"Hello AX.25";
        let data = vec![Bytes::from(payload.as_slice())];

        // Encode
        let encoded = codec.encode(data.clone(), &mut ctx).unwrap();
        assert_eq!(encoded.len(), 1);
        // Header (16 bytes) + Payload
        assert_eq!(encoded[0].len(), 16 + payload.len());

        // Decode
        // Context for decode doesn't need to match encode context initially,
        // as Ax25 unpack logic extracts info from header.
        let decode_ctx = CodecContext::default();
        let decode_input = vec![(Cow::Owned(decode_ctx.clone()), encoded[0].clone())];

        let (decoded, _) = codec.try_decode(decode_input).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].1, Bytes::from(payload.as_slice()));

        // Verify context update from header
        let res_ctx = &decoded[0].0;
        assert_eq!(res_ctx.src_callsign.as_deref(), Some("MYCALL"));
        assert_eq!(res_ctx.dst_callsign.as_deref(), Some("URCALL"));
    }
}
