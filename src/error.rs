use thiserror::Error;

/// A required external tool or file that was not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    pub name: String,
    pub hint: String,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("usage error: {0}")]
    Usage(String),
    #[error("missing required tools: {}", .0.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", "))]
    MissingTools(Vec<Missing>),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{tool} failed (exit code {code:?}): {stderr}")]
    Tool {
        tool: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("parse error: {0}")]
    Parse(String),
    #[error("config error: {0}")]
    Config(String),
    /// A config file that exists but could not be read. Keeps the `io::Error` so callers
    /// can tell a permission problem from a parse error.
    #[error("config error: {}: {source}", path.display())]
    ConfigIo {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("job error: {0}")]
    Job(String),
    #[error("{backend}: {message}")]
    Llm {
        backend: String,
        message: String,
        transient: bool,
    },
    #[error("template {name}: {message}")]
    Template { name: String, message: String },
    #[error("interrupted")]
    Interrupted,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage(_) => 2,
            Error::Interrupted => 130,
            _ => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_usage_is_2() {
        assert_eq!(Error::Usage("x".into()).exit_code(), 2);
    }

    #[test]
    fn exit_code_interrupted_is_130() {
        assert_eq!(Error::Interrupted.exit_code(), 130);
    }

    #[test]
    fn exit_code_other_is_1() {
        assert_eq!(Error::Parse("x".into()).exit_code(), 1);
        assert_eq!(Error::MissingTools(vec![]).exit_code(), 1);
    }

    #[test]
    fn llm_error_exit_code_is_1() {
        let e = Error::Llm {
            backend: "ollama".into(),
            message: "down".into(),
            transient: true,
        };
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.to_string(), "ollama: down");
    }

    #[test]
    fn template_error_exit_code_is_1() {
        let e = Error::Template {
            name: "minutes".into(),
            message: "bad".into(),
        };
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.to_string(), "template minutes: bad");
    }
}
