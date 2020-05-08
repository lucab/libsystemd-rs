use thiserror::Error;

/// Library errors.
#[derive(Error, Debug)]
#[error("libsystemd error: {0}")]
pub struct SdError(pub(crate) String);

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
