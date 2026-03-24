//! Symbol Importer - Import symbols from external sources
//!
//! 符号导入器：
//! - 批量导入优化
//!
//! Note: woofind and wootype integration is disabled for standalone builds

use std::sync::Arc;

use rayon::prelude::*;

use super::{BridgeError, Result};
use crate::symbol::{
    DefinitionLocation, Package, PackageId, Symbol, SymbolId, SymbolKind, SymbolUniverse,
    UniverseBuilder, Visibility,
};

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

    // Note: woofind and wootype integration disabled for standalone builds
    //
    // /// Import from woofind's InvertedIndex
    // #[cfg(feature = "woofind")]
    // pub fn import_from_woofind(
    //     &mut self,
    //     index: &woofind::index::InvertedIndex,
    // ) -> Result<Arc<SymbolUniverse>> {
    //     ...
    // }
    //
    // /// Import from wootype's TypeUniverse
    // #[cfg(feature = "wootype")]
    // pub fn import_from_wootype(
    //     &mut self,
    //     _type_universe: &wootype::core::universe::TypeUniverse,
    // ) -> Result<Arc<SymbolUniverse>> {
    //     ...
    // }

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
            guard.insert_symbol(sym).map_err(BridgeError::Symbol)?;
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
        guard.insert_symbol(symbol).map_err(BridgeError::Symbol)?;
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
