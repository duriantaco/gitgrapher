pub mod go;
pub mod javascript;
pub mod language;
pub mod python;
pub mod rust;
pub mod scanner;
pub mod typescript;

use gg_core::config::Config;
use gg_core::error::{GgError, GgResult};
use gg_core::types::{Language, ParseResult};
use language::LanguageProvider;
use std::path::Path;

/// Registry of all language providers.
pub struct LanguageRegistry {
    providers: Vec<Box<dyn LanguageProvider>>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            providers: Vec::new(),
        };
        reg.register(Box::new(typescript::TypeScriptProvider::new()));
        reg.register(Box::new(javascript::JavaScriptProvider::new()));
        reg.register(Box::new(python::PythonProvider::new()));
        reg.register(Box::new(go::GoProvider::new()));
        reg.register(Box::new(rust::RustProvider::new()));
        reg
    }

    pub fn register(&mut self, provider: Box<dyn LanguageProvider>) {
        self.providers.push(provider);
    }

    /// Detect the language of a file from its extension.
    pub fn detect(&self, path: &Path) -> Option<Language> {
        let ext = path.extension()?.to_str()?;
        Language::from_extension(ext)
    }

    /// Get the provider for a language.
    pub fn get(&self, lang: Language) -> Option<&dyn LanguageProvider> {
        self.providers
            .iter()
            .find(|p| p.language() == lang)
            .map(|p| p.as_ref())
    }

    /// Parse a single file using the appropriate language provider.
    pub fn parse_file(&self, path: &Path, source: &[u8], config: &Config) -> GgResult<ParseResult> {
        let lang = self.detect(path).ok_or_else(|| {
            GgError::UnsupportedLanguage(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("unknown")
                    .to_string(),
            )
        })?;

        let provider = self
            .get(lang)
            .ok_or_else(|| GgError::UnsupportedLanguage(lang.to_string()))?;

        provider.parse(path, source, config)
    }

    /// Get all supported languages.
    pub fn supported_languages(&self) -> Vec<Language> {
        self.providers.iter().map(|p| p.language()).collect()
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}
