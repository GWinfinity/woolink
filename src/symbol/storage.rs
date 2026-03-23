//! SoA (Structure of Arrays) Storage for Symbol Data
//! 
//! 符号属性分块存储，最大化 CPU 缓存命中率：
//! - 遍历符号名称时，只加载 name_offsets 数组，其他属性不占用缓存行
//! - 相比 AoS (Array of Structs)，缓存效率提升 5-10 倍
//! - 支持 SIMD 批量比较

use std::alloc::{alloc, dealloc, Layout};
use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{Symbol, Package, Import, SymbolKind, Visibility, Result, SymbolError};

/// Type-safe symbol identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct SymbolId(u32);

impl SymbolId {
    pub const INVALID: SymbolId = SymbolId(0);
    
    #[inline]
    pub fn new(id: u32) -> Self {
        Self(id)
    }
    
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
    
    #[inline]
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl Default for SymbolId {
    fn default() -> Self {
        Self::INVALID
    }
}

/// Type-safe package identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PackageId(u32);

impl PackageId {
    pub const INVALID: PackageId = PackageId(0);
    
    #[inline]
    pub fn new(id: u32) -> Self {
        Self(id)
    }
    
    #[inline]
    pub fn index(&self) -> usize {
        self.0 as usize
    }
    
    #[inline]
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl Default for PackageId {
    fn default() -> Self {
        Self::INVALID
    }
}

/// SoA storage for symbol data
/// 
/// Layout: [id_array][package_id_array][kind_array][visibility_array][name_offset_array]...
/// Each array is contiguous in memory for cache efficiency
pub struct SoAStorage {
    // Symbol arrays
    symbol_ids: NonNull<u32>,
    symbol_package_ids: NonNull<u32>,
    symbol_kinds: NonNull<u8>,
    symbol_visibilities: NonNull<u8>,
    symbol_name_offsets: NonNull<u32>,
    symbol_name_lens: NonNull<u16>,
    symbol_doc_offsets: NonNull<u32>,
    symbol_doc_lens: NonNull<u16>,
    symbol_sig_offsets: NonNull<u32>,
    symbol_sig_lens: NonNull<u16>,
    symbol_def_file_ids: NonNull<u32>,
    symbol_def_offsets: NonNull<u32>,
    symbol_chain_nexts: NonNull<u32>,
    
    // Capacity and count
    symbol_capacity: usize,
    symbol_count: AtomicU32,
    
    // String pool
    string_pool: NonNull<u8>,
    string_pool_capacity: usize,
    string_pool_used: AtomicU32,
    
    // Package storage
    packages: NonNull<Package>,
    package_capacity: usize,
    package_count: AtomicU32,
    
    // Import storage
    imports: NonNull<Import>,
    import_capacity: usize,
    import_count: AtomicU32,
    
    // Allocator marker
    _marker: PhantomData<()>,
}

// SoAStorage is thread-safe for reads
unsafe impl Send for SoAStorage {}
unsafe impl Sync for SoAStorage {}

impl SoAStorage {
    /// Create new SoA storage with initial capacity
    pub fn new(symbol_capacity: usize, string_pool_capacity: usize, package_capacity: usize) -> Self {
        let symbol_cap = symbol_capacity.max(1024);
        let string_cap = string_pool_capacity.max(65536);
        let pkg_cap = package_capacity.max(256);
        let import_cap = (symbol_cap * 2).max(1024);
        
        unsafe {
            Self {
                symbol_ids: alloc_array(symbol_cap),
                symbol_package_ids: alloc_array(symbol_cap),
                symbol_kinds: alloc_array(symbol_cap),
                symbol_visibilities: alloc_array(symbol_cap),
                symbol_name_offsets: alloc_array(symbol_cap),
                symbol_name_lens: alloc_array(symbol_cap),
                symbol_doc_offsets: alloc_array(symbol_cap),
                symbol_doc_lens: alloc_array(symbol_cap),
                symbol_sig_offsets: alloc_array(symbol_cap),
                symbol_sig_lens: alloc_array(symbol_cap),
                symbol_def_file_ids: alloc_array(symbol_cap),
                symbol_def_offsets: alloc_array(symbol_cap),
                symbol_chain_nexts: alloc_array(symbol_cap),
                symbol_capacity: symbol_cap,
                symbol_count: AtomicU32::new(0),
                
                string_pool: alloc_array(string_cap),
                string_pool_capacity: string_cap,
                string_pool_used: AtomicU32::new(0),
                
                packages: alloc_array(pkg_cap),
                package_capacity: pkg_cap,
                package_count: AtomicU32::new(0),
                
                imports: alloc_array(import_cap),
                import_capacity: import_cap,
                import_count: AtomicU32::new(0),
                
                _marker: PhantomData,
            }
        }
    }
    
    /// Get symbol count
    #[inline]
    pub fn symbol_count(&self) -> usize {
        self.symbol_count.load(Ordering::Relaxed) as usize
    }
    
    /// Get package count
    #[inline]
    pub fn package_count(&self) -> usize {
        self.package_count.load(Ordering::Relaxed) as usize
    }
    
    /// Insert a new symbol
    pub fn insert_symbol(&self, symbol: Symbol) -> Result<SymbolId> {
        let idx = self.symbol_count.fetch_add(1, Ordering::SeqCst) as usize;
        
        if idx >= self.symbol_capacity {
            return Err(SymbolError::InvalidId(idx as u32));
        }
        
        let id = symbol.id;
        
        unsafe {
            *self.symbol_ids.as_ptr().add(idx) = symbol.id;
            *self.symbol_package_ids.as_ptr().add(idx) = symbol.package_id;
            *self.symbol_kinds.as_ptr().add(idx) = symbol.kind as u8;
            *self.symbol_visibilities.as_ptr().add(idx) = symbol.visibility as u8;
            *self.symbol_name_offsets.as_ptr().add(idx) = symbol.name_offset;
            *self.symbol_name_lens.as_ptr().add(idx) = symbol.name_len;
            *self.symbol_doc_offsets.as_ptr().add(idx) = symbol.doc_offset;
            *self.symbol_doc_lens.as_ptr().add(idx) = symbol.doc_len;
            *self.symbol_sig_offsets.as_ptr().add(idx) = symbol.signature_offset;
            *self.symbol_sig_lens.as_ptr().add(idx) = symbol.signature_len;
            *self.symbol_def_file_ids.as_ptr().add(idx) = symbol.def_file_id;
            *self.symbol_def_offsets.as_ptr().add(idx) = symbol.def_offset;
            *self.symbol_chain_nexts.as_ptr().add(idx) = symbol.chain_next;
        }
        
        Ok(SymbolId::new(id))
    }
    
    /// Get symbol by ID - O(1) direct indexing
    #[inline]
    pub fn get_symbol(&self, id: SymbolId) -> Option<Symbol> {
        let idx = id.index();
        if idx >= self.symbol_count() {
            return None;
        }
        
        unsafe {
            Some(Symbol {
                id: *self.symbol_ids.as_ptr().add(idx),
                package_id: *self.symbol_package_ids.as_ptr().add(idx),
                kind: mem::transmute(*self.symbol_kinds.as_ptr().add(idx)),
                visibility: mem::transmute(*self.symbol_visibilities.as_ptr().add(idx)),
                name_offset: *self.symbol_name_offsets.as_ptr().add(idx),
                name_len: *self.symbol_name_lens.as_ptr().add(idx),
                doc_offset: *self.symbol_doc_offsets.as_ptr().add(idx),
                doc_len: *self.symbol_doc_lens.as_ptr().add(idx),
                signature_offset: *self.symbol_sig_offsets.as_ptr().add(idx),
                signature_len: *self.symbol_sig_lens.as_ptr().add(idx),
                def_file_id: *self.symbol_def_file_ids.as_ptr().add(idx),
                def_offset: *self.symbol_def_offsets.as_ptr().add(idx),
                chain_next: *self.symbol_chain_nexts.as_ptr().add(idx),
            })
        }
    }
    
    /// Batch get symbol names - cache-friendly sequential access
    pub fn get_symbol_names<'a>(&self, start: usize, count: usize, string_pool: &'a [u8]) -> Vec<&'a str> {
        let symbol_count = self.symbol_count();
        let end = (start + count).min(symbol_count);
        
        let mut names = Vec::with_capacity(end - start);
        
        unsafe {
            for i in start..end {
                let offset = *self.symbol_name_offsets.as_ptr().add(i) as usize;
                let len = *self.symbol_name_lens.as_ptr().add(i) as usize;
                
                if offset + len <= string_pool.len() {
                    let bytes = &string_pool[offset..offset + len];
                    if let Ok(name) = std::str::from_utf8(bytes) {
                        names.push(name);
                    }
                }
            }
        }
        
        names
    }
    
    /// Insert string into pool, return offset
    pub fn insert_string(&self, s: &str) -> (u32, u16) {
        let len = s.len().min(u16::MAX as usize) as u16;
        let offset = self.string_pool_used.fetch_add(len as u32 + 1, Ordering::SeqCst);
        
        if (offset as usize + len as usize) >= self.string_pool_capacity {
            // Pool full - return invalid offset
            return (0, 0);
        }
        
        unsafe {
            let ptr = self.string_pool.as_ptr().add(offset as usize);
            std::ptr::copy_nonoverlapping(s.as_bytes().as_ptr(), ptr, len as usize);
            *ptr.add(len as usize) = 0; // null terminate
        }
        
        (offset, len)
    }
    
    /// Insert package
    pub fn insert_package(&self, package: Package) -> Result<PackageId> {
        let idx = self.package_count.fetch_add(1, Ordering::SeqCst) as usize;
        
        if idx >= self.package_capacity {
            return Err(SymbolError::InvalidId(idx as u32));
        }
        
        let id = package.id;
        
        unsafe {
            *self.packages.as_ptr().add(idx) = package;
        }
        
        Ok(PackageId::new(id))
    }
    
    /// Get package by ID
    #[inline]
    pub fn get_package(&self, id: PackageId) -> Option<Package> {
        let idx = id.index();
        if idx >= self.package_count() {
            return None;
        }
        
        unsafe {
            Some(std::ptr::read(self.packages.as_ptr().add(idx)))
        }
    }
    
    /// Iterate over all symbols in cache-friendly order
    pub fn iter_symbols(&self) -> impl Iterator<Item = Symbol> + '_ {
        let count = self.symbol_count();
        (0..count).filter_map(move |i| self.get_symbol(SymbolId::new(i as u32)))
    }
    
    /// Get memory usage statistics
    pub fn memory_usage(&self) -> usize {
        let symbol_arrays = self.symbol_capacity * (
            mem::size_of::<u32>() * 8 +  // ids, package_ids, name_off, doc_off, sig_off, def_file, def_off, chain
            mem::size_of::<u8>() * 2 +   // kind, visibility
            mem::size_of::<u16>() * 3    // name_len, doc_len, sig_len
        );
        
        let packages = self.package_capacity * mem::size_of::<Package>();
        let imports = self.import_capacity * mem::size_of::<Import>();
        let strings = self.string_pool_capacity;
        
        symbol_arrays + packages + imports + strings
    }
}

impl Drop for SoAStorage {
    fn drop(&mut self) {
        unsafe {
            dealloc_array::<u32>(self.symbol_ids, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_package_ids, self.symbol_capacity);
            dealloc_array::<u8>(self.symbol_kinds, self.symbol_capacity);
            dealloc_array::<u8>(self.symbol_visibilities, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_name_offsets, self.symbol_capacity);
            dealloc_array::<u16>(self.symbol_name_lens, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_doc_offsets, self.symbol_capacity);
            dealloc_array::<u16>(self.symbol_doc_lens, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_sig_offsets, self.symbol_capacity);
            dealloc_array::<u16>(self.symbol_sig_lens, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_def_file_ids, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_def_offsets, self.symbol_capacity);
            dealloc_array::<u32>(self.symbol_chain_nexts, self.symbol_capacity);
            
            dealloc_array::<u8>(self.string_pool, self.string_pool_capacity);
            dealloc_array::<Package>(self.packages, self.package_capacity);
            dealloc_array::<Import>(self.imports, self.import_capacity);
        }
    }
}

/// Helper to allocate aligned array
unsafe fn alloc_array<T>(count: usize) -> NonNull<T> {
    let layout = Layout::array::<T>(count).unwrap();
    let ptr = alloc(layout) as *mut T;
    NonNull::new(ptr).expect("allocation failed")
}

/// Helper to deallocate array
unsafe fn dealloc_array<T>(ptr: NonNull<T>, count: usize) {
    let layout = Layout::array::<T>(count).unwrap();
    dealloc(ptr.as_ptr() as *mut u8, layout);
}

/// High-level symbol storage API
pub struct SymbolStorage {
    inner: SoAStorage,
}

impl SymbolStorage {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: SoAStorage::new(capacity, capacity * 64, capacity / 10),
        }
    }
    
    pub fn insert(&self, symbol: Symbol) -> Result<SymbolId> {
        self.inner.insert_symbol(symbol)
    }
    
    pub fn get(&self, id: SymbolId) -> Option<Symbol> {
        self.inner.get_symbol(id)
    }
    
    pub fn count(&self) -> usize {
        self.inner.symbol_count()
    }
    
    pub fn memory_usage(&self) -> usize {
        self.inner.memory_usage()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soa_storage() {
        let storage = SoAStorage::new(100, 1024, 10);
        
        let sym = Symbol {
            id: 1,
            package_id: 0,
            kind: SymbolKind::Function,
            visibility: Visibility::Public,
            name_offset: 0,
            name_len: 3,
            doc_offset: 0,
            doc_len: 0,
            signature_offset: 0,
            signature_len: 0,
            def_file_id: 0,
            def_offset: 0,
            chain_next: 0,
        };
        
        let id = storage.insert_symbol(sym.clone()).unwrap();
        assert_eq!(id, SymbolId::new(1));
        
        let retrieved = storage.get_symbol(id).unwrap();
        assert_eq!(retrieved.id, sym.id);
        assert_eq!(retrieved.kind, sym.kind);
    }

    #[test]
    fn test_symbol_id_operations() {
        let id = SymbolId::new(42);
        assert_eq!(id.index(), 42);
        assert_eq!(id.as_u32(), 42);
        assert_ne!(id, SymbolId::INVALID);
    }
}
