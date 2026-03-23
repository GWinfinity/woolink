//! woolink 🔗 - Global Symbol Table for Woo Ecosystem
//! 
//! 跨包符号解析系统，提供：
//! - SoA 布局：符号属性分块存储，CPU 缓存友好
//! - 并发查询：RwLock<SymbolUniverse> 支持 1000+ AI Agent 线程
//! - 惰性反序列化：rkyv 零拷贝，mmap 直接映射
//! - O(1) 定义跳转：链式符号索引
//! - Lock-free 符号链接
//! 
//! ## Quick Start
//! 
//! ```rust
//! use woolink::{SymbolUniverse, Symbol, SymbolKind, SymbolId};
//! 
//! // Create universe
//! let universe = SymbolUniverse::new(100_000);
//! 
//! // Concurrent reads (1000+ threads supported)
//! let guard = universe.read();
//! let symbol = guard.get_symbol(SymbolId::new(42));
//! let (target, location) = guard.jump_to_definition(SymbolId::new(42)).unwrap();
//! ```

pub mod symbol;

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
};

pub mod prelude {
    //! Common imports for woolink users
    
    pub use crate::symbol::{
        SymbolUniverse,
        SymbolId,
        PackageId,
        SymbolKind,
        Visibility,
        DefinitionLocation,
    };
}

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Magic number for archive files
pub const ARCHIVE_MAGIC: &[u8; 4] = b"WLSK";

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
}
