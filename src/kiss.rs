//! KISS framing and deframing implementation according to the KISS TNC protocol standard.

pub const FEND: u8 = 0xC0;
pub const FESC: u8 = 0xDB;
pub const TFEND: u8 = 0xDC;
pub const TFESC: u8 = 0xDD;
pub const MAX_KISS_FRAME_SIZE: usize = 65_536;

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
