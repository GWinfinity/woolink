//! Bridge module - Integration with woofind and wootype
//!
//! 桥接层：
//! - 从 woofind 导入符号索引
//! - 与 wootype 类型系统集成
//! - 提供统一的跨包引用解析

pub mod importer;
pub mod resolver;

pub use importer::{ImportConfig, SymbolImporter};
pub use resolver::{CrossPackageResolver, ReferenceKind, ResolutionResult};

use crate::symbol::{PackageId, SymbolError, SymbolId};

/// Unified error type for bridge operations
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Symbol error: {0}")]
    Symbol(#[from] SymbolError),

    #[error("Import failed: {0}")]
    Import(String),

    #[error("Resolution failed: {0}")]
    Resolution(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),
}

pub type Result<T> = std::result::Result<T, BridgeError>;
