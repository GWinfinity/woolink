//! SymbolUniverse - Global Symbol Table with Concurrent Access
//! 
//! 全局符号宇宙：
//! - `RwLock<SymbolUniverse>` 支持 1000+ AI Agent 线程同时读取
//! - SoA 布局存储符号数据
//! - 链式索引实现 O(1) 定义跳转
//! - 支持事务性修改和快照

use std::sync::Arc;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use dashmap::DashMap;
use scc::HashMap as SccHashMap;

use super::{
    Symbol, Package, Import, SymbolId, PackageId, SymbolKind, Visibility,
    SoAStorage, ChainedIndex, SymbolChain, DefinitionLocation,
    UniverseStats, Result, SymbolError,
};

/// Thread-safe read guard for SymbolUniverse
pub type SymbolUniverseGuard<'a> = RwLockReadGuard<'a, SymbolUniverseInner>;

/// Thread-safe write guard for SymbolUniverse
pub type SymbolUniverseWriteGuard<'a> = RwLockWriteGuard<'a, SymbolUniverseInner>;

/// Snapshot of symbol universe for speculative operations
#[derive(Debug, Clone)]
pub struct UniverseSnapshot {
    pub symbols: Vec<Symbol>,
    pub packages: Vec<Package>,
    pub imports: Vec<Import>,
    pub timestamp: u64,
}

impl UniverseSnapshot {
    pub fn empty() -> Self {
        Self {
            symbols: Vec::new(),
            packages: Vec::new(),
            imports: Vec::new(),
            timestamp: 0,
        }
    }
    
    pub fn find_symbol(&self, id: SymbolId) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.id == id.as_u32())
    }
    
    pub fn find_package(&self, id: PackageId) -> Option<&Package> {
        self.packages.iter().find(|p| p.id == id.as_u32())
    }
}

/// Inner mutable state of SymbolUniverse
pub struct SymbolUniverseInner {
    /// SoA storage for symbols
    storage: SoAStorage,
    
    /// Chained index for resolution
    index: ChainedIndex,
    
    /// Package path -> PackageId cache
    package_cache: DashMap<String, PackageId>,
    
    /// String pool for names
    string_pool: Vec<u8>,
    
    /// Timestamp counter for snapshots
    timestamp: std::sync::atomic::AtomicU64,
    
    /// Statistics
    stats: UniverseStats,
}

/// Global Symbol Universe - The central symbol table
/// 
/// Usage:
/// ```rust
/// let universe = SymbolUniverse::new(100_000);
/// 
/// // Concurrent reads (1000+ threads)
/// let guard = universe.read();
/// let symbol = guard.get_symbol(id);
/// let location = guard.jump_to_definition(id); // O(1)
/// 
/// // Exclusive writes
/// let mut guard = universe.write();
/// guard.insert_symbol(symbol);
/// ```
pub struct SymbolUniverse {
    inner: RwLock<SymbolUniverseInner>,
}

impl SymbolUniverse {
    /// Create new symbol universe with initial capacity
    pub fn new(symbol_capacity: usize) -> Self {
        let string_capacity = symbol_capacity * 64;
        let package_capacity = symbol_capacity / 100;
        
        Self {
            inner: RwLock::new(SymbolUniverseInner {
                storage: SoAStorage::new(symbol_capacity, string_capacity, package_capacity),
                index: ChainedIndex::new(),
                package_cache: DashMap::with_capacity(package_capacity),
                string_pool: Vec::with_capacity(string_capacity),
                timestamp: std::sync::atomic::AtomicU64::new(1),
                stats: UniverseStats::default(),
            }),
        }
    }
    
    /// Acquire read lock - allows concurrent reads from 1000+ threads
    #[inline]
    pub fn read(&self) -> SymbolUniverseGuard<'_> {
        self.inner.read()
    }
    
    /// Acquire write lock - exclusive access for modifications
    #[inline]
    pub fn write(&self) -> SymbolUniverseWriteGuard<'_> {
        self.inner.write()
    }
    
    /// Try to acquire read lock
    #[inline]
    pub fn try_read(&self) -> Option<SymbolUniverseGuard<'_>> {
        self.inner.try_read()
    }
    
    /// Create a snapshot for speculative operations
    pub fn snapshot(&self) -> UniverseSnapshot {
        let inner = self.read();
        inner.create_snapshot()
    }
    
    /// Get current timestamp
    pub fn timestamp(&self) -> u64 {
        self.read().timestamp()
    }
}

impl SymbolUniverseInner {
    /// Insert a symbol into the universe
    pub fn insert_symbol(&mut self, symbol: Symbol) -> Result<SymbolId> {
        let id = SymbolId::new(symbol.id);
        
        // Insert into storage
        self.storage.insert_symbol(symbol.clone())?;
        
        // Index by name
        let pkg_id = PackageId::new(symbol.package_id);
        let name = self.get_string(symbol.name_offset, symbol.name_len)?;
        self.index.index_name(pkg_id, name, id);
        
        // Create chain
        let def = DefinitionLocation::new(symbol.def_file_id, symbol.def_offset);
        self.index.insert_chain(SymbolChain::new(id, def))?;
        
        // Update stats
        self.stats.total_symbols += 1;
        
        Ok(id)
    }
    
    /// Get symbol by ID - O(1)
    #[inline]
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        self.storage.get_symbol(id)
    }
    
    /// Get symbol by ID (checked)
    pub fn require_symbol(&self, id: SymbolId) -> Result<Symbol> {
        self.get_symbol(id)
            .ok_or_else(|| SymbolError::NotFound(format!("symbol {}", id.as_u32())))
    }
    
    /// Lookup symbol by name in package - O(1) average
    pub fn lookup_symbol(&self, package: PackageId, name: &str) -> Vec<Symbol> {
        self.index.lookup_by_name(package, name)
            .into_iter()
            .filter_map(|id| self.get_symbol(id))
            .collect()
    }
    
    /// Lookup symbol across all packages
    pub fn lookup_global(&self, name: &str) -> Vec<(PackageId, Symbol)> {
        self.index.lookup_global(name)
            .into_iter()
            .filter_map(|(pkg, id)| {
                self.get_symbol(id).map(|s| (pkg, s))
            })
            .collect()
    }
    
    /// O(1) definition jump - the key feature!
    /// Returns (target_symbol, definition_location)
    pub fn jump_to_definition(&self, id: SymbolId) -> Result<(Symbol, DefinitionLocation)> {
        let (target_id, _depth, location) = self.index.resolve_chain(id)?;
        let symbol = self.require_symbol(target_id)?;
        Ok((symbol, location))
    }
    
    /// Resolve chain to final symbol
    pub fn resolve_chain(&self, id: SymbolId) -> Result<Symbol> {
        let (target_id, _, _) = self.index.resolve_chain(id)?;
        self.require_symbol(target_id)
    }
    
    /// Insert a package
    pub fn insert_package(&mut self, package: Package) -> Result<PackageId> {
        let id = PackageId::new(package.id);
        
        self.storage.insert_package(package.clone())?;
        
        // Cache by path
        let path = self.get_string(package.path_offset, package.path_len)?;
        self.package_cache.insert(path.to_string(), id);
        
        self.stats.total_packages += 1;
        
        Ok(id)
    }
    
    /// Get package by ID
    #[inline]
    pub fn get_package(&self, id: PackageId) -> Option<Package> {
        self.storage.get_package(id)
    }
    
    /// Get package by path
    pub fn find_package(&self, path: &str) -> Option<Package> {
        self.package_cache.get(path)
            .and_then(|id| self.get_package(*id))
    }
    
    /// Create alias link
    pub fn link_alias(&self, alias: SymbolId, target: SymbolId) -> Result<()> {
        self.index.link_symbol(alias, target)
    }
    
    /// Index a method for a type
    pub fn index_method(&self, type_id: SymbolId, method_id: SymbolId) {
        self.index.index_method(type_id, method_id);
    }
    
    /// Get methods for a type
    pub fn get_methods(&self, type_id: SymbolId) -> Vec<Symbol> {
        self.index.get_methods(type_id)
            .into_iter()
            .filter_map(|id| self.get_symbol(id))
            .collect()
    }
    
    /// Index interface implementation
    pub fn index_implementation(&self, interface: SymbolId, implementor: SymbolId) {
        self.index.index_implementation(interface, implementor);
    }
    
    /// Get implementations of an interface
    pub fn get_implementations(&self, interface: SymbolId) -> Vec<Symbol> {
        self.index.get_implementations(interface)
            .into_iter()
            .filter_map(|id| self.get_symbol(id))
            .collect()
    }
    
    /// Create a snapshot
    pub fn create_snapshot(&self) -> UniverseSnapshot {
        let count = self.storage.symbol_count();
        let mut symbols = Vec::with_capacity(count);
        
        for i in 0..count {
            if let Some(sym) = self.storage.get_symbol(SymbolId::new(i as u32)) {
                symbols.push(sym);
            }
        }
        
        let pkg_count = self.storage.package_count();
        let mut packages = Vec::with_capacity(pkg_count);
        
        for i in 0..pkg_count {
            if let Some(pkg) = self.storage.get_package(PackageId::new(i as u32)) {
                packages.push(pkg);
            }
        }
        
        UniverseSnapshot {
            symbols,
            packages,
            imports: Vec::new(),
            timestamp: self.timestamp(),
        }
    }
    
    /// Get current timestamp
    pub fn timestamp(&self) -> u64 {
        self.timestamp.load(std::sync::atomic::Ordering::Relaxed)
    }
    
    /// Bump timestamp
    pub fn bump_timestamp(&self) {
        self.timestamp.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    
    /// Get statistics
    pub fn stats(&self) -> UniverseStats {
        UniverseStats {
            total_symbols: self.stats.total_symbols,
            total_packages: self.stats.total_packages,
            total_imports: self.stats.total_imports,
            string_pool_size: self.string_pool.len(),
            memory_usage_bytes: self.storage.memory_usage(),
        }
    }
    
    /// Helper: get string from pool
    fn get_string(&self, offset: u32, len: u16) -> Result<&str> {
        let start = offset as usize;
        let end = start + len as usize;
        
        if end > self.string_pool.len() {
            return Err(SymbolError::InvalidId(offset));
        }
        
        std::str::from_utf8(&self.string_pool[start..end])
            .map_err(|_| SymbolError::InvalidId(offset))
    }
}

/// Builder for constructing SymbolUniverse from parsed data
pub struct UniverseBuilder {
    symbols: Vec<Symbol>,
    packages: Vec<Package>,
    chains: Vec<SymbolChain>,
}

impl UniverseBuilder {
    pub fn new() -> Self {
        Self {
            symbols: Vec::new(),
            packages: Vec::new(),
            chains: Vec::new(),
        }
    }
    
    pub fn with_capacity(symbol_capacity: usize, package_capacity: usize) -> Self {
        Self {
            symbols: Vec::with_capacity(symbol_capacity),
            packages: Vec::with_capacity(package_capacity),
            chains: Vec::with_capacity(symbol_capacity),
        }
    }
    
    pub fn add_symbol(&mut self, symbol: Symbol, definition: DefinitionLocation) {
        let id = SymbolId::new(symbol.id);
        self.symbols.push(symbol);
        self.chains.push(SymbolChain::new(id, definition));
    }
    
    pub fn add_package(&mut self, package: Package) {
        self.packages.push(package);
    }
    
    pub fn build(self) -> SymbolUniverse {
        let universe = SymbolUniverse::new(self.symbols.len().max(1000));
        
        {
            let mut inner = universe.write();
            
            // Insert packages first
            for pkg in self.packages {
                let _ = inner.insert_package(pkg);
            }
            
            // Insert symbols
            for sym in self.symbols {
                let _ = inner.insert_symbol(sym);
            }
        }
        
        universe
    }
}

impl Default for UniverseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Symbol, SymbolKind, Visibility};

    fn create_test_symbol(id: u32, name: &str) -> Symbol {
        Symbol {
            id,
            package_id: 1,
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            name_offset: 0,
            name_len: name.len() as u16,
            doc_offset: 0,
            doc_len: 0,
            signature_offset: 0,
            signature_len: 0,
            def_file_id: 1,
            def_offset: id * 100,
            chain_next: 0,
        }
    }

    #[test]
    fn test_universe_concurrent_reads() {
        let universe = SymbolUniverse::new(1000);
        
        // Insert some symbols
        {
            let mut inner = universe.write();
            for i in 1..=100 {
                let sym = create_test_symbol(i, "test");
                inner.insert_symbol(sym).unwrap();
            }
        }
        
        // Concurrent reads
        let handles: Vec<_> = (0..10).map(|_| {
            let universe = &universe;
            std::thread::spawn(move || {
                let guard = universe.read();
                let sym = guard.get_symbol(SymbolId::new(50));
                assert!(sym.is_some());
                assert_eq!(sym.unwrap().id, 50);
            })
        }).collect();
        
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_definition_jump() {
        let universe = SymbolUniverse::new(100);
        
        {
            let mut inner = universe.write();
            
            // Create target symbol
            let target = create_test_symbol(1, "Target");
            inner.insert_symbol(target).unwrap();
            
            // Create alias
            let mut alias = create_test_symbol(2, "Alias");
            alias.chain_next = 1; // Points to target
            inner.insert_symbol(alias).unwrap();
            
            // Create chain link
            inner.link_alias(SymbolId::new(2), SymbolId::new(1)).unwrap();
        }
        
        // Jump from alias should resolve to target
        let guard = universe.read();
        let (target, location) = guard.jump_to_definition(SymbolId::new(2)).unwrap();
        assert_eq!(target.id, 1);
        assert_eq!(location.offset, 100); // 1 * 100
    }
}
