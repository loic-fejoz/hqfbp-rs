//! KISS framing and deframing implementation according to the KISS TNC protocol standard.

pub const FEND: u8 = 0xC0;
pub const FESC: u8 = 0xDB;
pub const TFEND: u8 = 0xDC;
pub const TFESC: u8 = 0xDD;

/// Encodes a raw PDU into a KISS data frame (Port 0).
pub fn encode_kiss_frame(pdu: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(pdu.len() + 4);
    frame.push(FEND);
    frame.push(0x00); // Command Byte: Data frame, Port 0

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
        if self.in_frame {
            if byte == FEND {
                self.in_frame = false;
                if !self.buffer.is_empty() {
                    let frame = std::mem::take(&mut self.buffer);
                    return Some(frame);
                } else {
                    return None;
                }
            } else if byte == FESC {
                self.escaped = true;
            } else if self.escaped {
                if byte == TFEND {
                    self.buffer.push(FEND);
                } else if byte == TFESC {
                    self.buffer.push(FESC);
                } else {
                    self.buffer.push(byte);
                }
                self.escaped = false;
            } else {
                self.buffer.push(byte);
            }
        } else if byte == FEND {
            self.in_frame = true;
            self.buffer.clear();
            self.escaped = false;
        }
        None
    }
}
