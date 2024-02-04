use serde::{Serialize, Serializer};

#[derive(Serialize, Clone)]
pub struct InvokeResult {
    pub code: i32,
    pub message: String,
}

/// The error types.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// JSON error.
    // #[error(transparent)]
    // Json(#[from] serde_json::Error),
    /// IO error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    String(String),
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
