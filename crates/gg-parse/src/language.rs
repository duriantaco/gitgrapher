use gg_core::config::Config;
use gg_core::error::GgResult;
use gg_core::types::{Language, ParseResult};
use std::path::Path;

/// Trait that each language must implement to support parsing.
pub trait LanguageProvider: Send + Sync {
    /// The language this provider handles.
    fn language(&self) -> Language;

    /// File extensions this provider handles.
    fn extensions(&self) -> &[&str];

    /// Parse a single file and extract all symbols, calls, imports, heritage.
    fn parse(&self, path: &Path, source: &[u8], config: &Config) -> GgResult<ParseResult>;
}
