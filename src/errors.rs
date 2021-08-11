use std::{error::Error, fmt};

/// Library errors.
#[derive(Debug)]
pub struct SdError(pub(crate) String);

impl fmt::Display for SdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "libsystemd error: {}", self.0)
    }
}

impl Error for SdError {}

impl From<&str> for SdError {
    fn from(arg: &str) -> Self {
        Self(arg.to_string())
    }
}

impl From<String> for SdError {
    fn from(arg: String) -> Self {
        Self(arg)
    }
}
