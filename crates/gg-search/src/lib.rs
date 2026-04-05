use gg_graph::GraphStore;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::*;
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("Tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("Query parse error: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),
}

/// A search result with score.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_id: String,
    pub name: String,
    pub label: String,
    pub file_path: String,
    pub line: u32,
    pub score: f32,
    pub is_exported: bool,
}

/// BM25 full-text search engine backed by Tantivy.
pub struct SearchEngine {
    index: Index,
    #[allow(dead_code)]
    schema: Schema,
    f_node_id: Field,
    f_name: Field,
    f_label: Field,
    f_file_path: Field,
    f_file_name: Field,
    f_line: Field,
    f_exported: Field,
}

impl SearchEngine {
    /// Build a search index from a graph store.
    pub fn build(store: &GraphStore) -> Result<Self, SearchError> {
        let mut schema_builder = Schema::builder();
        let f_node_id = schema_builder.add_text_field("node_id", STRING | STORED);
        let f_name = schema_builder.add_text_field("name", TEXT | STORED);
        let f_label = schema_builder.add_text_field("label", STRING | STORED);
        let f_file_path = schema_builder.add_text_field("file_path", TEXT | STORED);
        let f_file_name = schema_builder.add_text_field("file_name", TEXT | STORED);
        let f_line = schema_builder.add_u64_field("line", INDEXED | STORED);
        let f_exported = schema_builder.add_u64_field("exported", INDEXED | STORED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema.clone());
        let mut writer: IndexWriter = index.writer(50_000_000)?; // 50MB heap

        for node in store.nodes() {
            if !node.label.is_symbol() {
                continue;
            }

            let file_name = std::path::Path::new(node.properties.file_path.as_str())
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");

            writer.add_document(doc!(
                f_node_id => node.id.as_str(),
                f_name => node.properties.name.as_str(),
                f_label => node.label.as_str(),
                f_file_path => node.properties.file_path.as_str(),
                f_file_name => file_name,
                f_line => node.properties.start_line.unwrap_or(0) as u64,
                f_exported => if node.properties.is_exported { 1u64 } else { 0u64 },
            ))?;
        }

        writer.commit()?;

        Ok(Self {
            index,
            schema,
            f_node_id,
            f_name,
            f_label,
            f_file_path,
            f_file_name,
            f_line,
            f_exported,
        })
    }

    /// Search for symbols by query string. Uses BM25 ranking.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, SearchError> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let searcher = reader.searcher();

        // Search across name, file_path, and file_name fields
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![self.f_name, self.f_file_path, self.f_file_name],
        );

        let parsed = query_parser.parse_query(query)?;
        let top_docs = searcher.search(&parsed, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc: tantivy::TantivyDocument = searcher.doc(doc_address)?;

            let node_id = doc
                .get_first(self.f_node_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = doc
                .get_first(self.f_name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let label = doc
                .get_first(self.f_label)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let file_path = doc
                .get_first(self.f_file_path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let line = doc
                .get_first(self.f_line)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let is_exported = doc
                .get_first(self.f_exported)
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                == 1;

            results.push(SearchResult {
                node_id,
                name,
                label,
                file_path,
                line,
                score,
                is_exported,
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gg_core::types::{GraphNode, Language, NodeLabel, NodeProperties};
    use smol_str::SmolStr;

    fn make_store() -> GraphStore {
        let mut store = GraphStore::new();
        let nodes = vec![
            (
                "fn1",
                "handleAuth",
                NodeLabel::Function,
                "src/auth.ts",
                10,
                true,
            ),
            (
                "fn2",
                "handleLogin",
                NodeLabel::Function,
                "src/auth.ts",
                25,
                true,
            ),
            (
                "fn3",
                "validateToken",
                NodeLabel::Function,
                "src/auth/token.ts",
                5,
                true,
            ),
            (
                "cls1",
                "UserService",
                NodeLabel::Class,
                "src/services/user.ts",
                1,
                true,
            ),
            (
                "fn4",
                "getUser",
                NodeLabel::Method,
                "src/services/user.ts",
                15,
                true,
            ),
            (
                "fn5",
                "parseConfig",
                NodeLabel::Function,
                "src/config.ts",
                1,
                false,
            ),
        ];
        for (id, name, label, file, line, exported) in nodes {
            let mut props =
                NodeProperties::symbol(name, file, Language::TypeScript, line, line + 10);
            props.is_exported = exported;
            store.add_node(GraphNode {
                id: SmolStr::new(id),
                label,
                properties: props,
            });
        }
        store
    }

    #[test]
    fn test_search_by_name() {
        let store = make_store();
        let engine = SearchEngine::build(&store).unwrap();
        let results = engine.search("handleAuth", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "handleAuth");
    }

    #[test]
    fn test_search_multiple_results() {
        let store = make_store();
        let engine = SearchEngine::build(&store).unwrap();
        // Search for a term in the file path — both auth.ts functions should match
        let results = engine.search("auth.ts", 10).unwrap();
        assert!(results.len() >= 2, "Expected >=2, got {}", results.len());
    }

    #[test]
    fn test_search_by_file() {
        let store = make_store();
        let engine = SearchEngine::build(&store).unwrap();
        let results = engine.search("auth", 10).unwrap();
        // Should find functions in auth.ts and auth/token.ts
        assert!(results.len() >= 2);
    }

    #[test]
    fn test_search_no_results() {
        let store = make_store();
        let engine = SearchEngine::build(&store).unwrap();
        let results = engine.search("nonexistent_xyz", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_returns_metadata() {
        let store = make_store();
        let engine = SearchEngine::build(&store).unwrap();
        let results = engine.search("UserService", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "UserService");
        assert_eq!(results[0].label, "Class");
        assert!(results[0].is_exported);
    }
}
