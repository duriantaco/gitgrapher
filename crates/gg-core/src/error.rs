use thiserror::Error;

#[derive(Error, Debug)]
pub enum GgError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error in {file}: {message}")]
    Parse { file: String, message: String },

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Search error: {0}")]
    Search(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

pub type GgResult<T> = Result<T, GgError>;
