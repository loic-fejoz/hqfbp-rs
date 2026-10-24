//! KISS framing and deframing implementation according to the KISS TNC protocol standard.

pub const FEND: u8 = 0xC0;
pub const FESC: u8 = 0xDB;
pub const TFEND: u8 = 0xDC;
pub const TFESC: u8 = 0xDD;
pub const MAX_KISS_FRAME_SIZE: usize = 65_536;

/// Encodes a raw PDU into a KISS data frame (Port 0).
pub fn encode_kiss_frame(pdu: &[u8]) -> Vec<u8> {
    encode_kiss_frame_with_port(pdu, 0)
}

/// Encodes a raw PDU into a KISS data frame for a specific port (0..=15).
pub fn encode_kiss_frame_with_port(pdu: &[u8], port: u8) -> Vec<u8> {
    let mut frame = Vec::with_capacity(pdu.len() + 6);
    frame.push(FEND);
    let cmd = (port & 0x0F) << 4; // Command 0 (DataFrame) on designated port
    if cmd == FEND {
        frame.push(FESC);
        frame.push(TFEND);
    } else if cmd == FESC {
        frame.push(FESC);
        frame.push(TFESC);
    } else {
        frame.push(cmd);
    }

    for &byte in pdu {
        if byte == FEND {
            frame.push(FESC);
            frame.push(TFEND);
        } else if byte == FESC {
            frame.push(FESC);
            frame.push(TFESC);
        } else {
            frame.push(byte);
        }
    }

    frame.push(FEND);
    frame
}

/// KISS Command types according to the standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KissCommand {
    DataFrame,
    TxDelay,
    P,
    SlotTime,
    TxTail,
    FullDuplex,
    SetHardware,
    Return,
    Unknown(u8),
}

impl From<u8> for KissCommand {
    fn from(val: u8) -> Self {
        match val {
            0 => KissCommand::DataFrame,
            1 => KissCommand::TxDelay,
            2 => KissCommand::P,
            3 => KissCommand::SlotTime,
            4 => KissCommand::TxTail,
            5 => KissCommand::FullDuplex,
            6 => KissCommand::SetHardware,
            15 => KissCommand::Return,
            other => KissCommand::Unknown(other),
        }
    }
}

/// Parsed KISS frame containing the port number, command type, and payload data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKissFrame {
    pub port: u8,
    pub command: KissCommand,
    pub data: Vec<u8>,
}

/// Parses a raw unwrapped KISS frame (including the command byte).
pub fn parse_kiss_frame(raw_frame: &[u8]) -> Option<ParsedKissFrame> {
    if raw_frame.is_empty() {
        return None;
    }
    let cmd_byte = raw_frame[0];
    let port = (cmd_byte >> 4) & 0x0F;
    let command = KissCommand::from(cmd_byte & 0x0F);
    let data = raw_frame[1..].to_vec();

    Some(ParsedKissFrame {
        port,
        command,
        data,
    })
}

/// Stream decoder for KISS frames.
#[derive(Debug, Default)]
pub struct KissDecoder {
    in_frame: bool,
    escaped: bool,
    buffer: Vec<u8>,
}

impl KissDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a single byte into the decoder, returning a completed frame if available.
    pub fn process_byte(&mut self, byte: u8) -> Option<Vec<u8>> {
        if byte == FEND {
            if self.in_frame && !self.buffer.is_empty() {
                let frame = std::mem::take(&mut self.buffer);
                self.escaped = false;
                // Remains in_frame = true because FEND also marks the start of the next frame
                return Some(frame);
            } else {
                // Leading or redundant FEND: start of a frame, clear buffer
                self.in_frame = true;
                self.buffer.clear();
                self.escaped = false;
                return None;
            }
        }

        if self.in_frame {
            if self.buffer.len() >= MAX_KISS_FRAME_SIZE {
                self.in_frame = false;
                self.buffer.clear();
                self.escaped = false;
                log::warn!(
                    "KISS frame exceeded max length ({MAX_KISS_FRAME_SIZE} bytes); frame dropped"
                );
                return None;
            }

            if byte == FESC {
                self.escaped = true;
            } else if self.escaped {
                if byte == TFEND {
                    self.buffer.push(FEND);
                } else if byte == TFESC {
                    self.buffer.push(FESC);
                } else {
                    // Invalid escape sequence in KISS standard: drop or push?
                    // To be robust against noise, log warning and ignore invalid escape
                    log::debug!("Invalid KISS escape sequence: FESC followed by 0x{byte:02x}");
                }
                self.escaped = false;
            } else {
                self.buffer.push(byte);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kiss_encode_decode_roundtrip_with_ports() {
        let payload = vec![0x01, 0xC0, 0x02, 0xDB, 0x03];
        for port in [0, 1, 7, 15] {
            let encoded = encode_kiss_frame_with_port(&payload, port);
            let mut decoder = KissDecoder::new();
            let mut frames = Vec::new();
            for byte in encoded {
                if let Some(frame) = decoder.process_byte(byte) {
                    frames.push(frame);
                }
            }
            assert_eq!(frames.len(), 1);
            let parsed = parse_kiss_frame(&frames[0]).expect("valid frame");
            assert_eq!(parsed.port, port);
            assert_eq!(parsed.command, KissCommand::DataFrame);
            assert_eq!(parsed.data, payload);
        }
    }

    #[test]
    fn test_kiss_commands_parsing() {
        let raw = vec![0x21, 0x05]; // Port 2, Command 1 (TxDelay), arg 5
        let parsed = parse_kiss_frame(&raw).unwrap();
        assert_eq!(parsed.port, 2);
        assert_eq!(parsed.command, KissCommand::TxDelay);
        assert_eq!(parsed.data, vec![0x05]);
    }
}
