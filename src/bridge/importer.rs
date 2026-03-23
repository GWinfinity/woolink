//! Symbol Importer - Import symbols from woofind and wootype
//! 
//! 符号导入器：
//! - 从 woofind 的 InvertedIndex 导入符号
//! - 从 wootype 的 TypeUniverse 导入类型信息
//! - 批量导入优化

use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;

use crate::symbol::{
    SymbolUniverse, Symbol, Package, SymbolId, PackageId,
    SymbolKind, Visibility, DefinitionLocation, UniverseBuilder,
};
use super::{BridgeError, Result};

/// Import configuration
#[derive(Debug, Clone)]
pub struct ImportConfig {
    /// Include private symbols
    pub include_private: bool,
    
    /// Include documentation
    pub include_docs: bool,
    
    /// Include source locations
    pub include_locations: bool,
    
    /// Parallel import batch size
    pub batch_size: usize,
    
    /// Progress callback interval
    pub progress_interval: usize,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            include_private: false,
            include_docs: true,
            include_locations: true,
            batch_size: 10000,
            progress_interval: 1000,
        }
    }
}

/// Symbol importer from external sources
pub struct SymbolImporter {
    config: ImportConfig,
    string_pool: Vec<u8>,
    string_offset: u32,
}

impl SymbolImporter {
    pub fn new(config: ImportConfig) -> Self {
        Self {
            config,
            string_pool: Vec::with_capacity(1024 * 1024), // 1MB initial
            string_offset: 0,
        }
    }
    
    /// Import from woofind's InvertedIndex
    pub fn import_from_woofind(
        &mut self,
        index: &woofind::index::InvertedIndex,
    ) -> Result<Arc<SymbolUniverse>> {
        let start = Instant::now();
        
        tracing::info!("Starting import from woofind...");
        
        // Collect all symbols
        let mut symbols = Vec::new();
        let mut packages: std::collections::HashMap<String, PackageId> = std::collections::HashMap::new();
        let mut next_package_id = 1u32;
        
        // Process symbols by package
        for entry in index.name_index.iter() {
            for woofind_sym in entry.value() {
                // Get or create package
                let pkg_id = *packages.entry(woofind_sym.package.clone())
                    .or_insert_with(|| {
                        let id = PackageId::new(next_package_id);
                        next_package_id += 1;
                        id
                    });
                
                // Convert symbol kind
                let kind = match woofind_sym.kind {
                    woofind::index::SymbolKind::Function => SymbolKind::Function,
                    woofind::index::SymbolKind::Type => SymbolKind::Type,
                    woofind::index::SymbolKind::Interface => SymbolKind::Interface,
                    woofind::index::SymbolKind::Struct => SymbolKind::Struct,
                    woofind::index::SymbolKind::Const => SymbolKind::Const,
                    woofind::index::SymbolKind::Var => SymbolKind::Var,
                    woofind::index::SymbolKind::Method => SymbolKind::Method,
                };
                
                // Determine visibility
                let visibility = if woofind_sym.name.chars().next()
                    .map(|c: char| c.is_uppercase())
                    .unwrap_or(false) {
                    Visibility::Public
                } else {
                    Visibility::Private
                };
                
                // Skip private if not included
                if !self.config.include_private && matches!(visibility, Visibility::Private) {
                    continue;
                }
                
                // Add strings to pool
                let name_offset = self.add_string(&woofind_sym.name);
                let name_len = woofind_sym.name.len() as u16;
                
                let (doc_offset, doc_len) = if self.config.include_docs {
                    woofind_sym.doc.as_ref()
                        .map(|d| (self.add_string(d), d.len() as u16))
                        .unwrap_or((0, 0))
                } else {
                    (0, 0)
                };
                
                let (sig_offset, sig_len) = woofind_sym.signature.as_ref()
                    .map(|s| (self.add_string(s), s.len() as u16))
                    .unwrap_or((0, 0));
                
                let symbol = Symbol {
                    id: symbols.len() as u32 + 1,
                    package_id: pkg_id.as_u32(),
                    kind,
                    visibility,
                    name_offset,
                    name_len,
                    doc_offset,
                    doc_len,
                    signature_offset: sig_offset,
                    signature_len: sig_len,
                    def_file_id: 0, // Would need file table
                    def_offset: 0,
                    chain_next: 0,
                };
                
                symbols.push(symbol);
            }
        }
        
        tracing::info!("Collected {} symbols in {:?}", symbols.len(), start.elapsed());
        
        // Create universe using builder
        let mut builder = UniverseBuilder::with_capacity(symbols.len(), packages.len());
        
        // Add packages
        for (path, pkg_id) in packages {
            let name = path.rfind('/')
                .map(|i| &path[i+1..])
                .unwrap_or(&path);
            
            let path_offset = self.add_string(&path);
            let name_offset = self.add_string(name);
            
            let symbol_count = symbols.iter()
                .filter(|s| s.package_id == pkg_id.as_u32())
                .count() as u16;
            
            builder.add_package(Package {
                id: pkg_id.as_u32(),
                path_offset,
                path_len: path.len() as u16,
                name_offset,
                name_len: name.len() as u16,
                version_offset: 0,
                version_len: 0,
                first_symbol: 1,
                symbol_count,
                import_count: 0,
            });
        }
        
        // Add symbols with definitions
        for sym in symbols {
            let def = DefinitionLocation::new(sym.def_file_id, sym.def_offset);
            builder.add_symbol(sym, def);
        }
        
        let universe = builder.build();
        
        tracing::info!("Import complete in {:?}", start.elapsed());
        
        Ok(Arc::new(universe))
    }
    
    /// Import from wootype's TypeUniverse
    pub fn import_from_wootype(
        &mut self,
        _type_universe: &wootype::core::universe::TypeUniverse,
    ) -> Result<Arc<SymbolUniverse>> {
        let start = Instant::now();
        
        tracing::info!("Starting import from wootype...");
        
        // wootype 使用不同的内部结构，需要适配
        // 这里简化处理
        let universe = SymbolUniverse::new(1000);
        
        tracing::info!("Import from wootype complete in {:?}", start.elapsed());
        
        Ok(Arc::new(universe))
    }
    
    /// Batch import with parallel processing
    pub fn batch_import<I>(&mut self, items: I) -> Result<Vec<Symbol>>
    where
        I: IntoIterator<Item = (String, SymbolKind, u32)>,
    {
        let items: Vec<_> = items.into_iter().collect();
        
        // Pre-allocate strings to avoid mutable borrow in parallel closure
        let string_data: Vec<_> = items
            .iter()
            .map(|(name, _kind, _pkg_id)| {
                let name_offset = self.add_string(name);
                let name_len = name.len() as u16;
                (name_offset, name_len)
            })
            .collect();
        
        let symbols: Vec<_> = items
            .into_par_iter()
            .enumerate()
            .map(|(idx, (_name, kind, pkg_id))| {
                let (name_offset, name_len) = string_data[idx];
                
                Symbol {
                    id: (idx + 1) as u32,
                    package_id: pkg_id,
                    kind,
                    visibility: Visibility::Public,
                    name_offset,
                    name_len,
                    doc_offset: 0,
                    doc_len: 0,
                    signature_offset: 0,
                    signature_len: 0,
                    def_file_id: 0,
                    def_offset: 0,
                    chain_next: 0,
                }
            })
            .collect();
        
        Ok(symbols)
    }
    
    /// Get string pool
    pub fn string_pool(&self) -> &[u8] {
        &self.string_pool
    }
    
    /// Add string to pool, return offset
    fn add_string(&mut self, s: &str) -> u32 {
        let len = s.len();
        let offset = self.string_offset;
        
        // Check capacity
        if (offset as usize + len + 1) > self.string_pool.capacity() {
            self.string_pool.reserve(len + 1024);
        }
        
        // Append string
        self.string_pool.extend_from_slice(s.as_bytes());
        self.string_pool.push(0); // null terminate
        
        self.string_offset += (len + 1) as u32;
        offset
    }
}

impl Default for SymbolImporter {
    fn default() -> Self {
        Self::new(ImportConfig::default())
    }
}

/// Import statistics
#[derive(Debug, Clone, Default)]
pub struct ImportStats {
    pub symbols_imported: usize,
    pub packages_imported: usize,
    pub strings_pooled: usize,
    pub time_ms: u64,
}

/// Incremental importer for updates
pub struct IncrementalImporter {
    universe: Arc<SymbolUniverse>,
    config: ImportConfig,
}

impl IncrementalImporter {
    pub fn new(universe: Arc<SymbolUniverse>, config: ImportConfig) -> Self {
        Self { universe, config }
    }
    
    /// Add new symbols incrementally
    pub fn add_symbols(&self, symbols: Vec<Symbol>) -> Result<()> {
        let mut guard = self.universe.write();
        
        for sym in symbols {
            guard.insert_symbol(sym)
                .map_err(|e| BridgeError::Symbol(e))?;
        }
        
        Ok(())
    }
    
    /// Remove symbols by package
    pub fn remove_package(&self, package_id: PackageId) -> Result<()> {
        // Mark all symbols in package as removed
        // In practice, this might use tombstoning
        let _ = package_id;
        Ok(())
    }
    
    /// Update existing symbol
    pub fn update_symbol(&self, symbol: Symbol) -> Result<()> {
        // Update in place or replace
        let mut guard = self.universe.write();
        guard.insert_symbol(symbol)
            .map_err(|e| BridgeError::Symbol(e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_pool() {
        let mut importer = SymbolImporter::new(ImportConfig::default());
        
        let off1 = importer.add_string("hello");
        let off2 = importer.add_string("world");
        
        assert_eq!(off1, 0);
        assert_eq!(off2, 6); // "hello\0" = 6 bytes
        
        let pool = importer.string_pool();
        assert_eq!(&pool[0..5], b"hello");
        assert_eq!(&pool[6..11], b"world");
    }

    #[test]
    fn test_batch_import() {
        let mut importer = SymbolImporter::new(ImportConfig::default());
        
        let items = vec![
            ("Foo".to_string(), SymbolKind::Type, 1),
            ("Bar".to_string(), SymbolKind::Function, 1),
            ("Baz".to_string(), SymbolKind::Const, 2),
        ];
        
        let symbols = importer.batch_import(items).unwrap();
        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[0].id, 1);
        assert_eq!(symbols[1].id, 2);
    }
}
