//! Memory-Mapped Symbol Index with rkyv Zero-Copy Deserialization
//! 
//! 惰性反序列化：
//! - 索引文件直接 mmap 为 Rust 结构体，无需解析
//! - 符号数据在访问时才从磁盘加载
//! - 启动时间接近零（O(1) 文件映射）

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use memmap2::{Mmap, MmapOptions};

use super::{SymbolId, PackageId, Symbol, Package, DefinitionLocation, Result, SymbolError};

/// Archive header for version checking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ArchiveHeader {
    /// Magic number: "WLSK" (WooLink Symbol)
    pub magic: [u8; 4],
    
    /// Version: major.minor.patch (e.g., 0x0001_0000 = 1.0.0)
    pub version: u32,
    
    /// Number of symbols
    pub symbol_count: u64,
    
    /// Number of packages
    pub package_count: u64,
    
    /// String pool size in bytes
    pub string_pool_size: u64,
    
    /// Index table size
    pub index_size: u64,
    
    /// CRC32 checksum
    pub checksum: u32,
}

impl ArchiveHeader {
    pub const MAGIC: [u8; 4] = *b"WLSK";
    pub const VERSION: u32 = 0x0001_0000; // 1.0.0
    
    pub fn new(symbol_count: u64, package_count: u64, string_pool_size: u64, index_size: u64) -> Self {
        Self {
            magic: Self::MAGIC,
            version: Self::VERSION,
            symbol_count,
            package_count,
            string_pool_size,
            index_size,
            checksum: 0,
        }
    }
    
    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC && self.version == Self::VERSION
    }
}

/// Memory-mapped storage for symbols
/// 
/// Provides zero-copy access to archived symbol data:
/// - File is mmap'd directly into process address space
/// - Symbol structs are accessed without deserialization
/// - Only accessed pages are loaded from disk by OS
pub struct MmapIndex {
    /// Memory map of the index file
    mmap: Arc<Mmap>,
    
    /// Archive header
    header: ArchiveHeader,
    
    /// Symbol array offset in mmap
    symbol_offset: usize,
    
    /// Package array offset
    package_offset: usize,
    
    /// String pool offset
    string_offset: usize,
    
    /// Hash index offset
    index_offset: usize,
}

impl MmapIndex {
    /// Open and memory-map an index file
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)
            .map_err(SymbolError::MmapError)?;
        
        // Memory map the entire file
        let mmap = unsafe {
            MmapOptions::new()
                .map(&file)
                .map_err(SymbolError::MmapError)?
        };
        
        if mmap.len() < std::mem::size_of::<ArchiveHeader>() {
            return Err(SymbolError::MmapError(
                std::io::Error::new(std::io::ErrorKind::InvalidData, "File too small")
            ));
        }
        
        // Read header from mmap
        let header = unsafe {
            std::ptr::read_unaligned(mmap.as_ptr() as *const ArchiveHeader)
        };
        
        if !header.is_valid() {
            return Err(SymbolError::MmapError(
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid archive header")
            ));
        }
        
        let header_size = std::mem::size_of::<ArchiveHeader>();
        let symbol_offset = header_size;
        let symbol_data_size = header.symbol_count as usize * std::mem::size_of::<ArchivedSymbol>();
        
        let package_offset = symbol_offset + symbol_data_size;
        let package_data_size = header.package_count as usize * std::mem::size_of::<ArchivedPackage>();
        
        let string_offset = package_offset + package_data_size;
        
        let index_offset = string_offset + header.string_pool_size as usize;
        
        Ok(Self {
            mmap: Arc::new(mmap),
            header,
            symbol_offset,
            package_offset,
            string_offset,
            index_offset,
        })
    }
    
    /// Get symbol by ID - zero-copy access
    /// 
    /// The symbol data is directly accessed from the mmap'd memory
    /// without any copying or deserialization.
    #[inline]
    pub fn get_symbol(&self, id: SymbolId) -> Option<ArchivedSymbolRef<'_>> {
        let idx = id.index();
        if idx >= self.header.symbol_count as usize {
            return None;
        }
        
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.symbol_offset) as *const ArchivedSymbol;
            let sym = ptr.add(idx);
            Some(ArchivedSymbolRef {
                inner: &*sym,
                string_pool: self.string_pool(),
            })
        }
    }
    
    /// Get package by ID - zero-copy access
    #[inline]
    pub fn get_package(&self, id: PackageId) -> Option<ArchivedPackageRef<'_>> {
        let idx = id.index();
        if idx >= self.header.package_count as usize {
            return None;
        }
        
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.package_offset) as *const ArchivedPackage;
            let pkg = ptr.add(idx);
            Some(ArchivedPackageRef {
                inner: &*pkg,
                string_pool: self.string_pool(),
            })
        }
    }
    
    /// Iterate over all symbols
    pub fn iter_symbols(&self) -> impl Iterator<Item = ArchivedSymbolRef<'_>> {
        let count = self.header.symbol_count as usize;
        let mmap = Arc::clone(&self.mmap);
        let symbol_offset = self.symbol_offset;
        let string_pool_start = self.string_offset;
        let string_pool_size = self.header.string_pool_size as usize;
        
        (0..count).filter_map(move |i| {
            unsafe {
                let ptr = mmap.as_ptr().add(symbol_offset) as *const ArchivedSymbol;
                let sym = ptr.add(i);
                Some(ArchivedSymbolRef {
                    inner: &*sym,
                    string_pool: std::slice::from_raw_parts(
                        mmap.as_ptr().add(string_pool_start),
                        string_pool_size
                    ),
                })
            }
        })
    }
    
    /// Get symbol count
    #[inline]
    pub fn symbol_count(&self) -> usize {
        self.header.symbol_count as usize
    }
    
    /// Get package count
    #[inline]
    pub fn package_count(&self) -> usize {
        self.header.package_count as usize
    }
    
    /// Access string pool
    #[inline]
    fn string_pool(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.mmap.as_ptr().add(self.string_offset),
                self.header.string_pool_size as usize
            )
        }
    }
    
    /// Create a memory-mapped view of the symbol table
    /// 
    /// This allows the OS to handle paging and caching automatically.
    /// Frequently accessed symbols will stay in RAM, rarely accessed ones
    /// will be paged out.
    pub fn as_view(&self) -> SymbolTableView<'_> {
        SymbolTableView {
            index: self,
        }
    }
    
    /// Prefetch symbols into cache (for hot path optimization)
    /// 
    /// Uses POSIX madvise to hint the kernel to load pages into memory.
    pub fn prefetch_range(&self, start: SymbolId, count: usize) {
        let start_idx = start.index();
        let symbol_size = std::mem::size_of::<ArchivedSymbol>();
        let offset = self.symbol_offset + start_idx * symbol_size;
        let size = count * symbol_size;
        
        if offset + size <= self.mmap.len() {
            unsafe {
                #[cfg(target_os = "linux")]
                libc::posix_madvise(
                    self.mmap.as_ptr().add(offset) as *mut _,
                    size,
                    libc::MADV_WILL_NEED
                );
            }
        }
    }
}

/// Archived symbol format for disk storage
/// 
/// This is the on-disk format. All offsets are relative to the
/// start of their respective sections.
#[repr(C, packed)]
pub struct ArchivedSymbol {
    pub id: u32,
    pub package_id: u32,
    pub kind: u8,
    pub visibility: u8,
    pub name_offset: u32,
    pub name_len: u16,
    pub doc_offset: u32,
    pub doc_len: u16,
    pub signature_offset: u32,
    pub signature_len: u16,
    pub def_file_id: u32,
    pub def_offset: u32,
    pub chain_next: u32,
}

/// Reference to an archived symbol with string pool access
pub struct ArchivedSymbolRef<'a> {
    inner: &'a ArchivedSymbol,
    string_pool: &'a [u8],
}

impl<'a> ArchivedSymbolRef<'a> {
    #[inline]
    pub fn id(&self) -> u32 {
        self.inner.id
    }
    
    #[inline]
    pub fn package_id(&self) -> u32 {
        self.inner.package_id
    }
    
    pub fn name(&self) -> &str {
        let start = self.inner.name_offset as usize;
        let len = self.inner.name_len as usize;
        
        if start + len <= self.string_pool.len() {
            unsafe {
                std::str::from_utf8_unchecked(&self.string_pool[start..start + len])
            }
        } else {
            ""
        }
    }
    
    pub fn doc(&self) -> Option<&str> {
        if self.inner.doc_offset == 0 {
            return None;
        }
        
        let start = self.inner.doc_offset as usize;
        let len = self.inner.doc_len as usize;
        
        if start + len <= self.string_pool.len() {
            Some(unsafe {
                std::str::from_utf8_unchecked(&self.string_pool[start..start + len])
            })
        } else {
            None
        }
    }
    
    pub fn signature(&self) -> Option<&str> {
        if self.inner.signature_offset == 0 {
            return None;
        }
        
        let start = self.inner.signature_offset as usize;
        let len = self.inner.signature_len as usize;
        
        if start + len <= self.string_pool.len() {
            Some(unsafe {
                std::str::from_utf8_unchecked(&self.string_pool[start..start + len])
            })
        } else {
            None
        }
    }
    
    pub fn definition(&self) -> DefinitionLocation {
        DefinitionLocation::new(self.inner.def_file_id, self.inner.def_offset)
    }
}

impl<'a> std::fmt::Debug for ArchivedSymbolRef<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchivedSymbol")
            .field("id", &self.id())
            .field("name", &self.name())
            .field("package_id", &self.package_id())
            .finish()
    }
}

/// Archived package format
#[repr(C, packed)]
pub struct ArchivedPackage {
    pub id: u32,
    pub path_offset: u32,
    pub path_len: u16,
    pub name_offset: u32,
    pub name_len: u16,
    pub version_offset: u32,
    pub version_len: u16,
    pub first_symbol: u32,
    pub symbol_count: u16,
    pub import_count: u16,
}

/// Reference to archived package
pub struct ArchivedPackageRef<'a> {
    inner: &'a ArchivedPackage,
    string_pool: &'a [u8],
}

impl<'a> ArchivedPackageRef<'a> {
    #[inline]
    pub fn id(&self) -> u32 {
        self.inner.id
    }
    
    pub fn path(&self) -> &str {
        let start = self.inner.path_offset as usize;
        let len = self.inner.path_len as usize;
        
        if start + len <= self.string_pool.len() {
            unsafe {
                std::str::from_utf8_unchecked(&self.string_pool[start..start + len])
            }
        } else {
            ""
        }
    }
    
    pub fn name(&self) -> &str {
        let start = self.inner.name_offset as usize;
        let len = self.inner.name_len as usize;
        
        if start + len <= self.string_pool.len() {
            unsafe {
                std::str::from_utf8_unchecked(&self.string_pool[start..start + len])
            }
        } else {
            ""
        }
    }
}

/// Read-only view of the symbol table
pub struct SymbolTableView<'a> {
    index: &'a MmapIndex,
}

impl<'a> SymbolTableView<'a> {
    pub fn get_symbol(&self, id: SymbolId) -> Option<ArchivedSymbolRef<'a>> {
        self.index.get_symbol(id)
    }
    
    pub fn get_package(&self, id: PackageId) -> Option<ArchivedPackageRef<'a>> {
        self.index.get_package(id)
    }
    
    pub fn iter(&self) -> impl Iterator<Item = ArchivedSymbolRef<'a>> {
        self.index.iter_symbols()
    }
    
    pub fn count(&self) -> usize {
        self.index.symbol_count()
    }
}

/// Memory-mapped storage builder
pub struct MemoryMappedStorage {
    path: std::path::PathBuf,
}

impl MemoryMappedStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
    
    /// Open existing index
    pub fn open(&self) -> Result<MmapIndex> {
        MmapIndex::open(&self.path)
    }
    
    /// Check if index file exists
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_archive() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        
        // Write header
        let header = ArchiveHeader::new(2, 1, 100, 0);
        file.write_all(unsafe {
            std::slice::from_raw_parts(
                &header as *const _ as *const u8,
                std::mem::size_of::<ArchiveHeader>()
            )
        }).unwrap();
        
        file
    }

    #[test]
    fn test_archive_header() {
        let header = ArchiveHeader::new(100, 10, 1024, 512);
        assert!(header.is_valid());
        assert_eq!(header.symbol_count, 100);
        assert_eq!(header.package_count, 10);
    }

    #[test]
    fn test_invalid_header() {
        let mut header = ArchiveHeader::new(0, 0, 0, 0);
        header.magic = *b"XXXX";
        assert!(!header.is_valid());
    }
}
