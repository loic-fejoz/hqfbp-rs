use thiserror::Error;

#[derive(Error, Debug)]
pub enum HqfbpError {
    #[error("Codec error: {0}")]
    Codec(#[from] CodecError),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("CBOR decode error: {0}")]
    CborDecode(#[from] minicbor::decode::Error),

    #[error("Parse Int error: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Parsing error: {0}")]
    Parse(String),

    #[error("Other error: {0}")]
    Other(String),

    #[error("{0}: {1}")]
    Context(String, #[source] Box<HqfbpError>),
}

pub trait HqfbpContext<T> {
    fn context(self, msg: impl Into<String>) -> std::result::Result<T, HqfbpError>;
}

impl<T, E> HqfbpContext<T> for std::result::Result<T, E>
where
    E: Into<HqfbpError>,
{
    fn context(self, msg: impl Into<String>) -> std::result::Result<T, HqfbpError> {
        self.map_err(|e| HqfbpError::Context(msg.into(), Box::new(e.into())))
    }
}

#[derive(Error, Debug)]
pub enum CodecError {
    #[error("CRC mismatch")]
    CrcMismatch,

    #[error("FEC failure: {0}")]
    FecFailure(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Insufficient symbols/data for decoding: {0:?}")]
    InsufficientData(Option<String>),

    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),
}

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("Malformed header: {0}")]
    MalformedHeader(String),

    #[error("Missing mandatory field: {0}")]
    MissingField(String),

    #[error("Inconsistent header field: {0}")]
    InconsistentField(String),
}

pub type Result<T, E = HqfbpError> = std::result::Result<T, E>;
