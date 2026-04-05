use serde::{Deserialize, Serialize};
use smol_str::SmolStr;

/// Compact string type used for node/edge identifiers.
pub type NodeId = SmolStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeLabel {
    // Filesystem
    Project,
    Package,
    Module,
    Folder,
    File,
    // Core symbols
    Class,
    Function,
    Method,
    Variable,
    Interface,
    Enum,
    Decorator,
    Import,
    Type,
    CodeElement,
    // Analysis
    Community,
    Process,
    // Multi-language
    Struct,
    Macro,
    Typedef,
    Union,
    Namespace,
    Trait,
    Impl,
    TypeAlias,
    Const,
    Static,
    Property,
    Record,
    Delegate,
    Annotation,
    Constructor,
    Template,
    // Content
    Section,
    // Routes & tools
    Route,
    Tool,
}

impl NodeLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "Project",
            Self::Package => "Package",
            Self::Module => "Module",
            Self::Folder => "Folder",
            Self::File => "File",
            Self::Class => "Class",
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Variable => "Variable",
            Self::Interface => "Interface",
            Self::Enum => "Enum",
            Self::Decorator => "Decorator",
            Self::Import => "Import",
            Self::Type => "Type",
            Self::CodeElement => "CodeElement",
            Self::Community => "Community",
            Self::Process => "Process",
            Self::Struct => "Struct",
            Self::Macro => "Macro",
            Self::Typedef => "Typedef",
            Self::Union => "Union",
            Self::Namespace => "Namespace",
            Self::Trait => "Trait",
            Self::Impl => "Impl",
            Self::TypeAlias => "TypeAlias",
            Self::Const => "Const",
            Self::Static => "Static",
            Self::Property => "Property",
            Self::Record => "Record",
            Self::Delegate => "Delegate",
            Self::Annotation => "Annotation",
            Self::Constructor => "Constructor",
            Self::Template => "Template",
            Self::Section => "Section",
            Self::Route => "Route",
            Self::Tool => "Tool",
        }
    }

    /// Whether this label represents a callable symbol.
    pub fn is_callable(&self) -> bool {
        matches!(self, Self::Function | Self::Method | Self::Constructor)
    }

    /// Whether this label represents a type-defining symbol.
    pub fn is_type_def(&self) -> bool {
        matches!(
            self,
            Self::Class
                | Self::Interface
                | Self::Struct
                | Self::Enum
                | Self::Trait
                | Self::TypeAlias
                | Self::Record
                | Self::Union
                | Self::Delegate
                | Self::Template
        )
    }

    /// Whether this label represents a symbol (not filesystem/analysis).
    pub fn is_symbol(&self) -> bool {
        !matches!(
            self,
            Self::Project
                | Self::Package
                | Self::Module
                | Self::Folder
                | Self::File
                | Self::Community
                | Self::Process
                | Self::Section
                | Self::Import
        )
    }
}

impl std::fmt::Display for NodeLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationType {
    Contains,
    Calls,
    Inherits,
    Overrides,
    Imports,
    Uses,
    Defines,
    Decorates,
    Implements,
    Extends,
    HasMethod,
    HasProperty,
    Accesses,
    MemberOf,
    StepInProcess,
    HandlesRoute,
    Fetches,
    HandlesTool,
    EntryPointOf,
    Wraps,
    Queries,
}

impl RelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Contains => "CONTAINS",
            Self::Calls => "CALLS",
            Self::Inherits => "INHERITS",
            Self::Overrides => "OVERRIDES",
            Self::Imports => "IMPORTS",
            Self::Uses => "USES",
            Self::Defines => "DEFINES",
            Self::Decorates => "DECORATES",
            Self::Implements => "IMPLEMENTS",
            Self::Extends => "EXTENDS",
            Self::HasMethod => "HAS_METHOD",
            Self::HasProperty => "HAS_PROPERTY",
            Self::Accesses => "ACCESSES",
            Self::MemberOf => "MEMBER_OF",
            Self::StepInProcess => "STEP_IN_PROCESS",
            Self::HandlesRoute => "HANDLES_ROUTE",
            Self::Fetches => "FETCHES",
            Self::HandlesTool => "HANDLES_TOOL",
            Self::EntryPointOf => "ENTRY_POINT_OF",
            Self::Wraps => "WRAPS",
            Self::Queries => "QUERIES",
        }
    }

    pub fn from_nexus_str(s: &str) -> Option<Self> {
        match s {
            "CONTAINS" => Some(Self::Contains),
            "CALLS" => Some(Self::Calls),
            "INHERITS" => Some(Self::Inherits),
            "OVERRIDES" => Some(Self::Overrides),
            "IMPORTS" => Some(Self::Imports),
            "USES" => Some(Self::Uses),
            "DEFINES" => Some(Self::Defines),
            "DECORATES" => Some(Self::Decorates),
            "IMPLEMENTS" => Some(Self::Implements),
            "EXTENDS" => Some(Self::Extends),
            "HAS_METHOD" => Some(Self::HasMethod),
            "HAS_PROPERTY" => Some(Self::HasProperty),
            "ACCESSES" => Some(Self::Accesses),
            "MEMBER_OF" => Some(Self::MemberOf),
            "STEP_IN_PROCESS" => Some(Self::StepInProcess),
            "HANDLES_ROUTE" => Some(Self::HandlesRoute),
            "FETCHES" => Some(Self::Fetches),
            "HANDLES_TOOL" => Some(Self::HandlesTool),
            "ENTRY_POINT_OF" => Some(Self::EntryPointOf),
            "WRAPS" => Some(Self::Wraps),
            "QUERIES" => Some(Self::Queries),
            _ => None,
        }
    }
}

impl std::fmt::Display for RelationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Language enum
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language {
    TypeScript,
    JavaScript,
    Python,
    Java,
    Kotlin,
    Go,
    Rust,
    CSharp,
    C,
    CPlusPlus,
    PHP,
    Ruby,
    Swift,
    Dart,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Go => "go",
            Self::Rust => "rust",
            Self::CSharp => "csharp",
            Self::C => "c",
            Self::CPlusPlus => "cpp",
            Self::PHP => "php",
            Self::Ruby => "ruby",
            Self::Swift => "swift",
            Self::Dart => "dart",
        }
    }

    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "tsx" | "mts" | "cts" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "py" | "pyi" => Some(Self::Python),
            "java" => Some(Self::Java),
            "kt" | "kts" => Some(Self::Kotlin),
            "go" => Some(Self::Go),
            "rs" => Some(Self::Rust),
            "cs" => Some(Self::CSharp),
            "c" | "h" => Some(Self::C),
            "cpp" | "cxx" | "cc" | "hpp" | "hxx" | "hh" => Some(Self::CPlusPlus),
            "php" => Some(Self::PHP),
            "rb" => Some(Self::Ruby),
            "swift" => Some(Self::Swift),
            "dart" => Some(Self::Dart),
            _ => None,
        }
    }

    pub fn all() -> &'static [Language] {
        &[
            Self::TypeScript,
            Self::JavaScript,
            Self::Python,
            Self::Java,
            Self::Kotlin,
            Self::Go,
            Self::Rust,
            Self::CSharp,
            Self::C,
            Self::CPlusPlus,
            Self::PHP,
            Self::Ruby,
            Self::Swift,
            Self::Dart,
        ]
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Graph node
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProperties {
    pub name: SmolStr,
    pub file_path: SmolStr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    #[serde(default)]
    pub is_exported: bool,
    // Framework detection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ast_framework_multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ast_framework_reason: Option<SmolStr>,
    // Community properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heuristic_label: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohesion: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<SmolStr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<SmolStr>,
    // Process properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_type: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub communities: Option<Vec<SmolStr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point_id: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point_reason: Option<SmolStr>,
    // Method/property metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_type: Option<SmolStr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<SmolStr>,
    #[serde(default)]
    pub is_static: bool,
    #[serde(default)]
    pub is_readonly: bool,
    #[serde(default)]
    pub is_abstract: bool,
    #[serde(default)]
    pub is_async: bool,
    // Route properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_keys: Option<Vec<SmolStr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_keys: Option<Vec<SmolStr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub middleware: Option<Vec<SmolStr>>,
    // Annotations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<SmolStr>>,
}

impl NodeProperties {
    /// Create minimal properties for a filesystem node.
    pub fn file(name: impl Into<SmolStr>, file_path: impl Into<SmolStr>) -> Self {
        Self {
            name: name.into(),
            file_path: file_path.into(),
            start_line: None,
            end_line: None,
            language: None,
            is_exported: false,
            ast_framework_multiplier: None,
            ast_framework_reason: None,
            heuristic_label: None,
            cohesion: None,
            symbol_count: None,
            keywords: None,
            description: None,
            process_type: None,
            step_count: None,
            communities: None,
            entry_point_id: None,
            terminal_id: None,
            entry_point_score: None,
            entry_point_reason: None,
            parameter_count: None,
            level: None,
            return_type: None,
            declared_type: None,
            visibility: None,
            is_static: false,
            is_readonly: false,
            is_abstract: false,
            is_async: false,
            response_keys: None,
            error_keys: None,
            middleware: None,
            annotations: None,
        }
    }

    /// Create properties for a code symbol.
    pub fn symbol(
        name: impl Into<SmolStr>,
        file_path: impl Into<SmolStr>,
        language: Language,
        start_line: u32,
        end_line: u32,
    ) -> Self {
        let mut props = Self::file(name, file_path);
        props.language = Some(language);
        props.start_line = Some(start_line);
        props.end_line = Some(end_line);
        props
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub label: NodeLabel,
    pub properties: NodeProperties,
}

// ---------------------------------------------------------------------------
// Graph edge
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: NodeId,
    pub source_id: NodeId,
    pub target_id: NodeId,
    pub rel_type: RelationType,
    pub confidence: f64,
    pub reason: SmolStr,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i32>,
}

impl GraphEdge {
    pub fn new(
        source_id: impl Into<NodeId>,
        target_id: impl Into<NodeId>,
        rel_type: RelationType,
        confidence: f64,
        reason: impl Into<SmolStr>,
    ) -> Self {
        let src: NodeId = source_id.into();
        let tgt: NodeId = target_id.into();
        let id = SmolStr::new(format!("{src}-[{rel_type}]->{tgt}"));
        Self {
            id,
            source_id: src,
            target_id: tgt,
            rel_type,
            confidence,
            reason: reason.into(),
            step: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Extracted items (parse output types shared between gg-parse and gg-resolve)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedImport {
    pub source_file: SmolStr,
    pub imported_name: SmolStr,
    pub module_path: SmolStr,
    pub alias: Option<SmolStr>,
    pub is_default: bool,
    pub is_namespace: bool,
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedCall {
    pub caller_id: SmolStr,
    pub callee_name: SmolStr,
    pub receiver: Option<SmolStr>,
    pub file_path: SmolStr,
    pub line: u32,
    pub arguments_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedHeritage {
    pub child_id: SmolStr,
    pub parent_name: SmolStr,
    pub kind: HeritageKind,
    pub file_path: SmolStr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeritageKind {
    Extends,
    Implements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedAssignment {
    pub variable_name: SmolStr,
    pub assigned_type: Option<SmolStr>,
    pub scope: SmolStr,
    pub file_path: SmolStr,
    pub line: u32,
}

/// Result of parsing a single file.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub nodes: Vec<GraphNode>,
    pub imports: Vec<ExtractedImport>,
    pub calls: Vec<ExtractedCall>,
    pub heritage: Vec<ExtractedHeritage>,
    pub assignments: Vec<ExtractedAssignment>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection() {
        assert_eq!(Language::from_extension("ts"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
        assert_eq!(Language::from_extension("py"), Some(Language::Python));
        assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
        assert_eq!(Language::from_extension("txt"), None);
    }

    #[test]
    fn test_node_label_properties() {
        assert!(NodeLabel::Function.is_callable());
        assert!(NodeLabel::Method.is_callable());
        assert!(!NodeLabel::Class.is_callable());

        assert!(NodeLabel::Class.is_type_def());
        assert!(NodeLabel::Interface.is_type_def());
        assert!(!NodeLabel::Function.is_type_def());

        assert!(NodeLabel::Function.is_symbol());
        assert!(!NodeLabel::File.is_symbol());
        assert!(!NodeLabel::Community.is_symbol());
    }

    #[test]
    fn test_relation_roundtrip() {
        for rt in [
            RelationType::Contains,
            RelationType::Calls,
            RelationType::Extends,
            RelationType::Implements,
        ] {
            assert_eq!(RelationType::from_nexus_str(rt.as_str()), Some(rt));
        }
    }

    #[test]
    fn test_edge_id_generation() {
        let edge = GraphEdge::new("a", "b", RelationType::Calls, 0.9, "direct call");
        assert_eq!(edge.id.as_str(), "a-[CALLS]->b");
    }
}
