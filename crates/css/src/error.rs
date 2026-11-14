use std::fmt;

/// Errors that stop a whole parse. Problems with a single declaration or
/// rule are not errors: they are dropped and counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssError {
    InputTooLarge { limit: usize, actual: usize },
    LimitExceeded(&'static str),
}

impl fmt::Display for CssError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CssError::InputTooLarge { limit, actual } => {
                write!(f, "CSS of {actual} bytes exceeds the limit of {limit} bytes")
            }
            CssError::LimitExceeded(what) => write!(f, "CSS limit exceeded: {what}"),
        }
    }
}

impl std::error::Error for CssError {}