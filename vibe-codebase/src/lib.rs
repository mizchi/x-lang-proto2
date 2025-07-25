//! XS Workspace - Structured codebase and incremental compilation
//!
//! This crate combines Unison-style content-addressed code storage
//! with incremental compilation using Salsa.

use thiserror::Error;

// Codebase modules
pub mod block_registry;
pub mod code_repository;
pub mod codebase;
pub mod vbin;

#[cfg(test)]
mod vbin_tests;

// Incremental compilation modules
pub mod database;
pub mod wasm_queries_simple;

// Namespace system modules
pub mod ast_command;
pub mod dependency_extractor;
pub mod differential_test_runner;
pub mod hash;
pub mod incremental_type_checker;
pub mod namespace;

// Code query modules
pub mod code_query;
pub mod query_engine;

// Pipeline processing modules
pub mod pipeline;
pub mod shell_syntax;
pub mod structured_data;
pub mod unified_parser;

// Package management modules
pub mod package;

// Re-export important types
pub use codebase::{
    Branch, Codebase, CodebaseError, CodebaseManager, EditAction, EditSession, Hash, Patch, Term,
    TypeDef,
};
pub use database::{
    CodebaseQueries, CompilerQueries, Definition, Dependencies, DependencyQueries, ExpressionId,
    ModuleId, SourcePrograms, XsDatabase,
};
use vbin::VBinStorage;

/// Workspace errors
#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("Codebase error: {0}")]
    CodebaseError(#[from] CodebaseError),

    #[error("Compilation error: {0}")]
    CompilationError(String),

    #[error("Incremental compilation error: {0}")]
    IncrementalError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Simple source file representation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceFile {
    pub path: String,
    pub content: String,
}

/// Database for incremental compilation
pub type Database = database::XsDatabaseImpl;

/// Incremental compiler for XS language
pub struct IncrementalCompiler {
    db: Database,
}

impl IncrementalCompiler {
    /// Create a new incremental compiler
    pub fn new() -> Self {
        Self {
            db: Database::new(),
        }
    }

    /// Set file content
    pub fn set_file_content(&mut self, path: String, content: String) {
        // Create a SourcePrograms instance
        let source = SourcePrograms {
            path,
            content: content.clone(),
        };
        // This will trigger re-computation if the content has changed
        self.db
            .set_source_text(source.clone(), std::sync::Arc::new(content));
    }

    /// Type check a file
    pub fn type_check(&self, path: &str) -> Result<vibe_language::Type, vibe_language::XsError> {
        // Use the type_check query
        // Set empty content for now - it should be in cache
        let source = SourcePrograms {
            path: path.to_string(),
            content: self
                .db
                .source_text(SourcePrograms {
                    path: path.to_string(),
                    content: String::new(),
                })
                .to_string(),
        };
        database::CompilerQueries::type_check(&self.db, source)
    }
}

impl Default for IncrementalCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined workspace for XS language development
pub struct Workspace {
    codebase: Codebase,
    compiler: IncrementalCompiler,
}

impl Workspace {
    /// Create a new workspace with the specified data directory
    pub fn new<P: AsRef<std::path::Path>>(data_dir: P) -> Result<Self, WorkspaceError> {
        // 優先順位: .vibes > .bin
        let xbin_path = data_dir.as_ref().join("codebase.vibes");
        let bin_path = data_dir.as_ref().join("codebase.bin");

        let codebase = if xbin_path.exists() {
            // VBin形式から読み込み
            let mut storage = VBinStorage::new(xbin_path.to_string_lossy().to_string());
            // 全体を読み込むためのヘルパーメソッドが必要
            Self::load_full_vbin(&mut storage)?
        } else if bin_path.exists() {
            Codebase::load(&bin_path)?
        } else {
            Codebase::new()
        };

        let compiler = IncrementalCompiler::new();

        Ok(Self {
            codebase,
            compiler,
        })
    }

    /// Save the workspace to disk
    pub fn save<P: AsRef<std::path::Path>>(&self, data_dir: P) -> Result<(), WorkspaceError> {
        let codebase_path = data_dir.as_ref().join("codebase.bin");
        self.codebase.save(&codebase_path)?;
        Ok(())
    }

    /// Save the workspace to disk in VBin format
    pub fn save_vbin<P: AsRef<std::path::Path>>(&self, data_dir: P) -> Result<(), WorkspaceError> {
        let xbin_path = data_dir.as_ref().join("codebase.vibes");
        let mut storage = VBinStorage::new(xbin_path.to_string_lossy().to_string());
        storage.save_full(&self.codebase).map_err(|e| {
            WorkspaceError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e))
        })?;
        Ok(())
    }

    /// Get a reference to the codebase
    pub fn codebase(&self) -> &Codebase {
        &self.codebase
    }

    /// Get a mutable reference to the codebase
    pub fn codebase_mut(&mut self) -> &mut Codebase {
        &mut self.codebase
    }

    /// Get a reference to the incremental compiler
    pub fn compiler(&self) -> &IncrementalCompiler {
        &self.compiler
    }

    /// Get a mutable reference to the incremental compiler
    pub fn compiler_mut(&mut self) -> &mut IncrementalCompiler {
        &mut self.compiler
    }


    /// Compile a file incrementally
    pub fn compile_file(
        &mut self,
        path: &str,
        content: &str,
    ) -> Result<vibe_language::Type, WorkspaceError> {
        self.compiler
            .set_file_content(path.to_string(), content.to_string());
        self.compiler
            .type_check(path)
            .map_err(|e| WorkspaceError::CompilationError(e.to_string()))
    }

    /// Add a term to the codebase
    pub fn add_term(
        &mut self,
        name: Option<String>,
        expr: vibe_language::Expr,
        ty: vibe_language::Type,
    ) -> Result<Hash, WorkspaceError> {
        Ok(self.codebase.add_term(name, expr, ty)?)
    }

    /// Run tests
    pub fn run_test(&mut self, hash: &Hash) -> Result<String, WorkspaceError> {
        let term = self.codebase.get_term(hash).ok_or_else(|| {
            WorkspaceError::CodebaseError(CodebaseError::HashNotFound(hash.to_hex()))
        })?;

        // Simple test executor - just evaluate and check if it's true
        match vibe_runtime::eval(&term.expr) {
            Ok(vibe_language::Value::Bool(true)) => Ok("Test passed".to_string()),
            Ok(vibe_language::Value::Bool(false)) => Err(WorkspaceError::CompilationError("Test failed".to_string())),
            Ok(v) => Err(WorkspaceError::CompilationError(format!("Test returned non-boolean value: {v:?}"))),
            Err(e) => Err(WorkspaceError::CompilationError(format!("Test error: {e}"))),
        }
    }

    /// Edit a term by name
    pub fn edit_term(&self, name: &str) -> Result<String, WorkspaceError> {
        Ok(self.codebase.edit(name)?)
    }

    /// Update a term after editing
    pub fn update_term(&mut self, name: &str, new_expr: &str) -> Result<Hash, WorkspaceError> {
        Ok(self.codebase.update(name, new_expr)?)
    }

    /// Create a patch from a set of changes
    pub fn create_patch(&self) -> Patch {
        Patch::new()
    }

    /// Apply a patch to the codebase
    pub fn apply_patch(&mut self, patch: &Patch) -> Result<(), WorkspaceError> {
        Ok(patch.apply(&mut self.codebase)?)
    }

    /// Load full codebase from VBin storage
    fn load_full_vbin(storage: &mut VBinStorage) -> Result<Codebase, WorkspaceError> {
        storage
            .load_full()
            .map_err(|e| WorkspaceError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))
    }
}
