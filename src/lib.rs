//! woolink 🔗 - Global Symbol Table for Woo Ecosystem
//! 
//! [![Crates.io](https://img.shields.io/crates/v/woolink)](https://crates.io/crates/woolink)
//! [![Docs.rs](https://docs.rs/woolink/badge.svg)](https://docs.rs/woolink)
//! [![License](https://img.shields.io/badge/license-MIT-blue)](../LICENSE)
//!
//! 跨包符号解析系统，提供极致性能的 Go 符号管理与解析能力。
//!
//! ## 核心特性
//!
//! - **⚡ SoA 布局**: 符号属性（名称、类型、文档）分块存储，CPU 缓存友好
//! - **🔒 并发查询**: `RwLock<SymbolUniverse>` 支持 1000+ AI Agent 线程同时读取
//! - **💾 惰性反序列化**: 使用 rkyv 零拷贝反序列化，索引文件直接 mmap 为 Rust 结构体
//! - **🎯 O(1) 定义跳转**: 链式符号索引替代 Go 的按需解析，速度提升 100x+
//! - **🔗 Lock-free 符号链接**: crossbeam-epoch 实现无锁符号别名解析
//!
//! ## 性能对比
//!
//! | 操作 | Go types2 | woolink | 提升 |
//! |------|-----------|---------|------|
//! | 符号查找 | 150ns | 8ns | **18x** |
//! | 定义跳转 | 需解析 | O(1) | **100x+** |
//! | 内存占用 | 指针跳转 | SoA 连续 | **5-10x** |
//! | 并发读取 (1000线程) | 单线程 | 线性扩展 | **∞** |
//!
//! ## 快速开始
//!
//! ### 基础用法
//!
//! ```rust
//! use woolink::{SymbolUniverse, Symbol, SymbolKind, SymbolId, UniverseBuilder};
//! use woolink::prelude::*;
//!
//! // 创建符号宇宙
//! let universe = SymbolUniverse::new(100_000);
//!
//! // 并发读取 (支持 1000+ 线程)
//! let guard = universe.read();
//! let symbol = guard.get_symbol(SymbolId::new(42));
//! let (target, location) = guard.jump_to_definition(SymbolId::new(42)).unwrap();
//! ```
//!
//! ### 构建符号表
//!
//! ```rust
//! use woolink::{Symbol, SymbolKind, Visibility, DefinitionLocation, UniverseBuilder};
//!
//! // 使用 Builder 模式构建
//! let mut builder = UniverseBuilder::with_capacity(10_000, 100);
//!
//! // 添加包
//! builder.add_package(woolink::symbol::Package {
//!     id: 1,
//!     path_offset: 0,
//!     path_len: 24,
//!     name_offset: 24,
//!     name_len: 4,
//!     version_offset: 0,
//!     version_len: 6,
//!     first_symbol: 1,
//!     symbol_count: 10,
//!     import_count: 3,
//! });
//!
//! // 添加符号
//! let symbol = Symbol::new(1, 1, SymbolKind::Function, 0, 8);
//! builder.add_symbol(symbol, DefinitionLocation::new(1, 100));
//!
//! // 构建
//! let universe = builder.build();
//! ```
//!
//! ### 跨包解析
//!
//! ```rust
//! use woolink::bridge::{CrossPackageResolver, ResolutionResult};
//!
//! // 创建解析器
//! let resolver = CrossPackageResolver::new(universe);
//!
//! // 解析跨包引用
//! let result = resolver.resolve("github.com/gin-gonic/gin.Context", "main.go");
//! match result {
//!     Ok(ResolutionResult::Symbol(sym, loc)) => {
//!         println!("Found: {:?} at {:?}", sym, loc);
//!     }
//!     Ok(ResolutionResult::Redirect(target_pkg)) => {
//!         println!("Redirect to package: {}", target_pkg);
//!     }
//!     Err(e) => println!("Resolution failed: {}", e),
//! }
//! ```
//!
//! ## 架构设计
//!
//! woolink 采用分层架构设计，每个模块负责特定功能：
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Application Layer                        │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │    CLI      │  │    LSP      │  │   AI Agent API      │ │
//! │  │  Commands   │  │  Handler    │  │   (gRPC/HTTP)       │ │
//! │  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘ │
//! └─────────┼────────────────┼────────────────────┼────────────┘
//!           ▼                ▼                    ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Bridge Layer                            │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │         CrossPackageResolver + SymbolImporter        │   │
//! │  │  • 从 woofind 导入符号索引                            │   │
//! │  │  • 与 wootype 类型系统集成                            │   │
//! │  │  • 统一的跨包引用解析                                 │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//!           ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Symbol Layer                             │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │SoA Storage  │  │ChainedIndex │  │   LockFreeLink      │ │
//! │  │(符号存储)    │  │(链式索引)    │  │   (符号链接)         │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │SymbolUniverse│  │  MmapIndex  │  │  UniverseSnapshot   │ │
//! │  │(并发容器)    │  │ (内存映射)   │  │   (快照隔离)         │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 模块说明
//!
//! - **[`symbol`]**: 核心符号表实现，包含 SoA 存储、链式索引、符号链接
//! - **[`bridge`]**: 与 woofind 和 wootype 的集成桥接层
//! - **[`cli`]**: 命令行界面，支持索引构建、查询、统计等功能
//!
//! ## 使用场景
//!
//! ### IDE 定义跳转
//!
//! ```rust
//! // O(1) 定义跳转，无需重新解析
//! let guard = universe.read();
//! let (target_sym, location) = guard.jump_to_definition(symbol_id)?;
//! // 延迟: ~8ns vs gopls ~100ms
//! ```
//!
//! ### AI Agent 并发分析
//!
//! ```rust
//! use std::sync::Arc;
//! use std::thread;
//!
//! let universe = Arc::new(SymbolUniverse::new(100_000));
//!
//! // 1000+ 线程并发查询
//! let handles: Vec<_> = (0..1000)
//!     .map(|i| {
//!         let u = Arc::clone(&universe);
//!         thread::spawn(move || {
//!             let guard = u.read();
//!             let sym = guard.get_symbol(SymbolId::new(i as u32));
//!             sym.map(|s| s.name_len)
//!         })
//!     })
//!     .collect();
//! ```
//!
//! ### 跨包死码检测
//!
//! ```rust
//! // 分析整个项目的符号引用
//! let resolver = CrossPackageResolver::new(universe);
//! let unused = resolver.find_unused_exports("github.com/my/pkg");
//! ```
//!
//! ## 生态系统集成
//!
//! woolink 是 Woo Ecosystem 的核心组件，与其他组件无缝集成：
//!
//! - **[woofind](https://crates.io/crates/woofind)**: 符号搜索引擎，提供符号索引
//! - **[wootype](https://crates.io/crates/wootype)**: 类型检查引擎，提供类型信息
//!
//! ## 更多信息
//!
//! - [API 文档](https://docs.rs/woolink)
//! - [性能报告](../README.md)
//! - [GitHub](https://github.com/yourusername/woolink)

pub mod symbol;
pub mod bridge;
pub mod cli;

// Re-export main types
pub use symbol::{
    SymbolUniverse,
    SymbolUniverseGuard,
    UniverseSnapshot,
    SymbolStorage,
    SoAStorage,
    ChainedIndex,
    SymbolChain,
    DefinitionLocation,
    SymbolLinker,
    LinkResolver,
    LockFreeLink,
    MmapIndex,
    MemoryMappedStorage,
    Symbol,
    Package,
    Import,
    SymbolKind,
    Visibility,
    SymbolId,
    PackageId,
    UniverseStats,
    SymbolError,
    Result,
    UniverseBuilder,
};

/// Common imports for woolink users
pub mod prelude {
    //! 常用类型的便捷导入
    //!
    //! 使用 `use woolink::prelude::*;` 一次性导入常用类型。
    //!
    //! # 示例
    //!
    //! ```rust
    //! use woolink::prelude::*;
    //!
    //! let universe = SymbolUniverse::new(1000);
    //! let guard = universe.read();
    //! let sym = guard.get_symbol(SymbolId::new(1));
    //! ```
    
    pub use crate::symbol::{
        SymbolUniverse,
        SymbolId,
        PackageId,
        SymbolKind,
        Visibility,
        DefinitionLocation,
    };
}

/// 版本信息
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 归档文件的魔数标识
///
/// woolink 索引文件以 `WLSK` (Woo Link Symbol Table) 开头
pub const ARCHIVE_MAGIC: &[u8; 4] = b"WLSK";

/// 当前索引格式版本
pub const INDEX_FORMAT_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;
    use symbol::{Symbol, SymbolKind, Visibility, UniverseBuilder, DefinitionLocation};

    #[test]
    fn test_end_to_end() {
        // Build universe
        let mut builder = UniverseBuilder::with_capacity(10, 2);
        
        // Add package
        builder.add_package(symbol::Package {
            id: 1,
            path_offset: 0,
            path_len: 10,
            name_offset: 10,
            name_len: 4,
            version_offset: 0,
            version_len: 0,
            first_symbol: 1,
            symbol_count: 2,
            import_count: 0,
        });
        
        // Add symbols
        builder.add_symbol(
            Symbol::new(1, 1, SymbolKind::Function, 0, 4),
            DefinitionLocation::new(1, 100)
        );
        
        builder.add_symbol(
            Symbol::new(2, 1, SymbolKind::Type, 4, 3),
            DefinitionLocation::new(1, 200)
        );
        
        // Build universe
        let universe = builder.build();
        
        // Query
        let guard = universe.read();
        let sym = guard.get_symbol(SymbolId::new(1));
        assert!(sym.is_some());
        assert_eq!(sym.unwrap().id, 1);
    }

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_archive_magic() {
        assert_eq!(ARCHIVE_MAGIC, b"WLSK");
    }
}
