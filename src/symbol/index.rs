//! Chained Symbol Index - O(1) Definition Jump
//! 
//! 链式索引结构：
//! - 每个符号可以链接到另一个符号（如类型别名、方法接收者）
//! - 支持快速解析符号引用，无需按需解析
//! - 替代 Go types2 的 On-demand Parsing

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

use dashmap::DashMap;
use scc::HashMap as SccHashMap;

use super::{SymbolId, PackageId, Symbol, SymbolKind, Result, SymbolError};

/// Definition location (file + offset)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DefinitionLocation {
    /// File ID in the file table
    pub file_id: u32,
    
    /// Byte offset in file
    pub offset: u32,
    
    /// Line number (for display)
    pub line: u32,
    
    /// Column number (for display)
    pub column: u16,
}

impl DefinitionLocation {
    pub fn new(file_id: u32, offset: u32) -> Self {
        Self {
            file_id,
            offset,
            line: 0,
            column: 0,
        }
    }
    
    pub fn with_line_column(file_id: u32, offset: u32, line: u32, column: u16) -> Self {
        Self {
            file_id,
            offset,
            line,
            column,
        }
    }
}

/// Symbol chain node for resolution
#[derive(Debug, Clone)]
pub struct SymbolChain {
    /// This symbol
    pub symbol: SymbolId,
    
    /// Next symbol in chain (for aliases, re-exports)
    pub next: Option<SymbolId>,
    
    /// Target symbol (for references)
    pub target: Option<SymbolId>,
    
    /// Definition location for O(1) jump
    pub definition: DefinitionLocation,
    
    /// Chain depth (to detect cycles)
    pub depth: u16,
}

impl SymbolChain {
    pub fn new(symbol: SymbolId, definition: DefinitionLocation) -> Self {
        Self {
            symbol,
            next: None,
            target: None,
            definition,
            depth: 0,
        }
    }
    
    pub fn with_target(symbol: SymbolId, target: SymbolId, definition: DefinitionLocation) -> Self {
        Self {
            symbol,
            next: None,
            target: Some(target),
            definition,
            depth: 1,
        }
    }
    
    pub fn is_terminal(&self) -> bool {
        self.next.is_none() && self.target.is_none()
    }
    
    pub fn is_alias(&self) -> bool {
        self.next.is_some()
    }
}

/// Chained index for fast symbol resolution
/// 
/// Design:
/// - Lock-free lookup table: symbol_id -> chain
/// - Name-based index: (package, name) -> symbol_id
/// - Type-based chains: interface -> implementations
pub struct ChainedIndex {
    /// Symbol ID -> chain node (lock-free via scc::HashMap)
    chains: SccHashMap<u32, SymbolChain>,
    
    /// (PackageId, name_hash) -> SymbolId for fast lookup
    name_index: DashMap<(PackageId, u64), Vec<SymbolId>>,
    
    /// Type ID -> method symbols
    method_index: DashMap<SymbolId, Vec<SymbolId>>,
    
    /// Interface ID -> implementing types
    implementation_index: DashMap<SymbolId, Vec<SymbolId>>,
    
    /// Import alias -> actual symbol
    alias_index: DashMap<(PackageId, String), SymbolId>,
    
    /// Cycle detection counter
    visit_counter: AtomicU32,
}

impl ChainedIndex {
    pub fn new() -> Self {
        Self {
            chains: SccHashMap::new(),
            name_index: DashMap::with_capacity(100_000),
            method_index: DashMap::new(),
            implementation_index: DashMap::new(),
            alias_index: DashMap::new(),
            visit_counter: AtomicU32::new(1),
        }
    }
    
    /// Insert a symbol chain
    pub fn insert_chain(&self, chain: SymbolChain) -> Result<()> {
        self.chains.insert(chain.symbol.as_u32(), chain)
            .map_err(|_| SymbolError::InvalidId(chain.symbol.as_u32()))?;
        Ok(())
    }
    
    /// Get chain for symbol - O(1) lock-free
    #[inline]
    pub fn get_chain(&self, symbol: SymbolId) -> Option<SymbolChain> {
        self.chains.read(&symbol.as_u32(), |_, v| v.clone())
    }
    
    /// Index symbol by name for fast lookup
    pub fn index_name(&self, package: PackageId, name: &str, symbol: SymbolId) {
        let hash = Self::hash_name(name);
        let key = (package, hash);
        
        self.name_index
            .entry(key)
            .and_modify(|v| {
                if !v.contains(&symbol) {
                    v.push(symbol);
                }
            })
            .or_insert_with(|| vec![symbol]);
    }
    
    /// Lookup symbol by name in package - O(1) average
    pub fn lookup_by_name(&self, package: PackageId, name: &str) -> Vec<SymbolId> {
        let hash = Self::hash_name(name);
        self.name_index
            .get(&(package, hash))
            .map(|v| v.clone())
            .unwrap_or_default()
    }
    
    /// Lookup symbol across all packages
    pub fn lookup_global(&self, name: &str) -> Vec<(PackageId, SymbolId)> {
        let hash = Self::hash_name(name);
        let mut results = Vec::new();
        
        for entry in self.name_index.iter() {
            if entry.key().1 == hash {
                for &sym in entry.value() {
                    results.push((entry.key().0, sym));
                }
            }
        }
        
        results
    }
    
    /// Link symbol to target (for aliases, re-exports)
    pub fn link_symbol(&self, from: SymbolId, to: SymbolId) -> Result<()> {
        if let Some(mut chain) = self.get_chain(from) {
            chain.next = Some(to);
            chain.depth += 1;
            self.chains.update(&from.as_u32(), |_, v| {
                *v = chain.clone();
            });
            Ok(())
        } else {
            Err(SymbolError::NotFound(format!("symbol {}", from.as_u32())))
        }
    }
    
    /// Resolve chain to terminal symbol - O(depth)
    /// Returns (terminal_symbol, total_depth)
    pub fn resolve_chain(&self, start: SymbolId) -> Result<(SymbolId, usize, DefinitionLocation)> {
        let mut current = start;
        let mut depth = 0;
        let mut location = DefinitionLocation::default();
        
        // Cycle detection
        let visit_mark = self.visit_counter.fetch_add(1, Ordering::SeqCst);
        let max_depth = 100; // Prevent infinite loops
        
        while depth < max_depth {
            let chain = self.get_chain(current)
                .ok_or_else(|| SymbolError::BrokenChain(current.as_u32()))?;
            
            if depth == 0 {
                location = chain.definition;
            }
            
            // Check for terminal
            if chain.is_terminal() {
                return Ok((current, depth, location));
            }
            
            // Follow next in chain
            if let Some(next) = chain.next {
                current = next;
                depth += 1;
            } else if let Some(target) = chain.target {
                current = target;
                depth += 1;
            } else {
                return Ok((current, depth, location));
            }
        }
        
        Err(SymbolError::BrokenChain(start.as_u32()))
    }
    
    /// Index a method for a type
    pub fn index_method(&self, type_symbol: SymbolId, method: SymbolId) {
        self.method_index
            .entry(type_symbol)
            .and_modify(|v| v.push(method))
            .or_insert_with(|| vec![method]);
    }
    
    /// Get methods for a type
    pub fn get_methods(&self, type_symbol: SymbolId) -> Vec<SymbolId> {
        self.method_index
            .get(&type_symbol)
            .map(|v| v.clone())
            .unwrap_or_default()
    }
    
    /// Index an interface implementation
    pub fn index_implementation(&self, interface: SymbolId, implementor: SymbolId) {
        self.implementation_index
            .entry(interface)
            .and_modify(|v| {
                if !v.contains(&implementor) {
                    v.push(implementor);
                }
            })
            .or_insert_with(|| vec![implementor]);
    }
    
    /// Get all types implementing an interface
    pub fn get_implementations(&self, interface: SymbolId) -> Vec<SymbolId> {
        self.implementation_index
            .get(&interface)
            .map(|v| v.clone())
            .unwrap_or_default()
    }
    
    /// Register import alias
    pub fn register_alias(&self, package: PackageId, alias: &str, symbol: SymbolId) {
        self.alias_index.insert((package, alias.to_string()), symbol);
    }
    
    /// Lookup by alias
    pub fn lookup_alias(&self, package: PackageId, alias: &str) -> Option<SymbolId> {
        self.alias_index.get(&(package, alias.to_string())).map(|v| *v)
    }
    
    /// Get chain depth for a symbol
    pub fn get_depth(&self, symbol: SymbolId) -> u16 {
        self.get_chain(symbol)
            .map(|c| c.depth)
            .unwrap_or(0)
    }
    
    /// Statistics
    pub fn stats(&self) -> IndexStats {
        IndexStats {
            total_chains: self.chains.len(),
            name_entries: self.name_index.len(),
            method_entries: self.method_index.len(),
            implementation_entries: self.implementation_index.len(),
        }
    }
    
    /// FNV-1a hash for name indexing
    fn hash_name(name: &str) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x00000100000001b3;
        
        let mut hash = FNV_OFFSET;
        for byte in name.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }
}

impl Default for ChainedIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IndexStats {
    pub total_chains: usize,
    pub name_entries: usize,
    pub method_entries: usize,
    pub implementation_entries: usize,
}

/// Builder for constructing chains from parsed symbols
pub struct ChainBuilder {
    chains: HashMap<u32, SymbolChain>,
}

impl ChainBuilder {
    pub fn new() -> Self {
        Self {
            chains: HashMap::new(),
        }
    }
    
    pub fn add_terminal(&mut self, symbol: SymbolId, definition: DefinitionLocation) {
        self.chains.insert(
            symbol.as_u32(),
            SymbolChain::new(symbol, definition)
        );
    }
    
    pub fn add_alias(&mut self, alias: SymbolId, target: SymbolId, definition: DefinitionLocation) {
        self.chains.insert(
            alias.as_u32(),
            SymbolChain::with_target(alias, target, definition)
        );
    }
    
    pub fn add_link(&mut self, from: SymbolId, to: SymbolId) {
        if let Some(chain) = self.chains.get_mut(&from.as_u32()) {
            chain.next = Some(to);
            chain.depth += 1;
        }
    }
    
    pub fn build(self) -> Vec<SymbolChain> {
        self.chains.into_values().collect()
    }
}

impl Default for ChainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_resolution() {
        let index = ChainedIndex::new();
        
        // Create chain: A -> B -> C (terminal)
        let def_a = DefinitionLocation::new(1, 100);
        let def_b = DefinitionLocation::new(1, 200);
        let def_c = DefinitionLocation::new(1, 300);
        
        index.insert_chain(SymbolChain::with_target(
            SymbolId::new(1),
            SymbolId::new(2),
            def_a
        )).unwrap();
        
        index.insert_chain(SymbolChain::with_target(
            SymbolId::new(2),
            SymbolId::new(3),
            def_b
        )).unwrap();
        
        index.insert_chain(SymbolChain::new(
            SymbolId::new(3),
            def_c
        )).unwrap();
        
        // Resolve from A
        let (terminal, depth, loc) = index.resolve_chain(SymbolId::new(1)).unwrap();
        assert_eq!(terminal, SymbolId::new(3));
        assert_eq!(depth, 2);
        assert_eq!(loc.offset, 100); // Original location preserved
    }

    #[test]
    fn test_name_lookup() {
        let index = ChainedIndex::new();
        
        index.index_name(PackageId::new(1), "Foo", SymbolId::new(1));
        index.index_name(PackageId::new(1), "Bar", SymbolId::new(2));
        index.index_name(PackageId::new(2), "Foo", SymbolId::new(3));
        
        let results = index.lookup_by_name(PackageId::new(1), "Foo");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], SymbolId::new(1));
        
        let global = index.lookup_global("Foo");
        assert_eq!(global.len(), 2);
    }

    #[test]
    fn test_method_indexing() {
        let index = ChainedIndex::new();
        
        index.index_method(SymbolId::new(1), SymbolId::new(2));
        index.index_method(SymbolId::new(1), SymbolId::new(3));
        
        let methods = index.get_methods(SymbolId::new(1));
        assert_eq!(methods.len(), 2);
    }
}
