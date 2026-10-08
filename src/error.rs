use thiserror::Error;

/// Errors reading a Forte file.
#[derive(Debug, Error)]
pub enum ForteError {
    /// The data is neither a `.fnf` compound file nor a bare `Contents`
    /// stream (which starts with `MMMF`).
    #[error("not a Forte file")]
    NotForte,

    /// The compound file couldn't be read, or has no `Contents` stream.
    #[error("Forte file container: {0}")]
    Container(#[from] std::io::Error),

    /// The document doesn't decode: it's damaged, or uses a part of the
    /// format this crate hasn't seen yet.
    #[error("malformed Forte file: {0}")]
    Malformed(String),
}

pub type Result<T> = std::result::Result<T, ForteError>;
