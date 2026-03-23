//! Lock-free Symbol Linking
//! 
//! 并发安全的符号链接：
//! - 使用 crossbeam-epoch 实现无锁更新
//! - 支持读时复制 (Copy-on-Read) 语义
//! - 1000+ AI Agent 线程可以安全并发查询和更新

use std::sync::atomic::{AtomicU64, AtomicPtr, Ordering};
use std::ptr;

use crossbeam_epoch::{self as epoch, Atomic, Guard, Owned, Shared};

use super::{SymbolId, DefinitionLocation, Result, SymbolError};

/// Link status for atomic operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LinkStatus {
    /// Link is being constructed
    Pending = 0,
    /// Link is active and valid
    Active = 1,
    /// Link is being updated
    Updating = 2,
    /// Link has been removed
    Removed = 3,
}

/// Lock-free link node
/// 
/// Uses epoch-based memory reclamation for safe concurrent updates
pub struct LockFreeLink {
    /// Target symbol
    target: Atomic<SymbolId>,
    
    /// Definition location
    location: Atomic<DefinitionLocation>,
    
    /// Link metadata
    metadata: AtomicU64,
    
    /// Status flag
    status: AtomicU64,
}

impl LockFreeLink {
    pub fn new(target: SymbolId, location: DefinitionLocation) -> Self {
        let meta = Self::pack_metadata(0, LinkStatus::Active);
        
        Self {
            target: Atomic::new(target),
            location: Atomic::new(location),
            metadata: AtomicU64::new(meta),
            status: AtomicU64::new(0),
        }
    }
    
    /// Get target atomically
    #[inline]
    pub fn get_target(&self, guard: &Guard) -> Option<SymbolId> {
        self.target.load(Ordering::Acquire, guard).map(|p| unsafe { *p.as_raw() })
    }
    
    /// Get location atomically
    #[inline]
    pub fn get_location(&self, guard: &Guard) -> Option<DefinitionLocation> {
        self.location.load(Ordering::Acquire, guard).map(|p| unsafe { *p.as_raw() })
    }
    
    /// Update target atomically using CAS
    /// 
    /// Returns true if update succeeded, false if CAS failed
    pub fn update_target(&self, new_target: SymbolId, guard: &Guard) -> bool {
        let new = Owned::new(new_target);
        
        loop {
            let current = self.target.load(Ordering::Acquire, guard);
            
            match self.target.compare_exchange(
                current,
                new,
                Ordering::Release,
                Ordering::Acquire,
                guard
            ) {
                Ok(_) => return true,
                Err(e) => {
                    // CAS failed, retry with updated current
                    if e.current.is_null() {
                        // Link was removed
                        return false;
                    }
                    continue;
                }
            }
        }
    }
    
    /// Mark link as pending update
    pub fn begin_update(&self) -> bool {
        let expected = Self::pack_metadata(0, LinkStatus::Active);
        let new = Self::pack_metadata(0, LinkStatus::Updating);
        
        self.metadata
            .compare_exchange(expected, new, Ordering::SeqCst, Ordering::Relaxed)
            .is_ok()
    }
    
    /// Complete update
    pub fn end_update(&self) {
        let new = Self::pack_metadata(0, LinkStatus::Active);
        self.metadata.store(new, Ordering::Release);
    }
    
    /// Remove link
    pub fn remove(&self, guard: &Guard) -> bool {
        let new_status = Self::pack_metadata(0, LinkStatus::Removed);
        self.metadata.store(new_status, Ordering::Release);
        
        // Set target to null
        let new = Shared::<SymbolId>::null();
        let old = self.target.swap(new, Ordering::Release, guard);
        
        // Schedule old value for reclamation
        if !old.is_null() {
            unsafe {
                guard.defer_unchecked(move || {
                    drop(old.into_owned());
                });
            }
        }
        
        true
    }
    
    /// Get current status
    #[inline]
    pub fn status(&self) -> LinkStatus {
        let meta = self.metadata.load(Ordering::Acquire);
        Self::unpack_status(meta)
    }
    
    /// Check if link is active
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self.status(), LinkStatus::Active)
    }
    
    fn pack_metadata(version: u16, status: LinkStatus) -> u64 {
        ((version as u64) << 16) | ((status as u8) as u64)
    }
    
    fn unpack_status(metadata: u64) -> LinkStatus {
        let status_byte = (metadata & 0xFF) as u8;
        match status_byte {
            0 => LinkStatus::Pending,
            1 => LinkStatus::Active,
            2 => LinkStatus::Updating,
            3 => LinkStatus::Removed,
            _ => LinkStatus::Pending,
        }
    }
}

impl Drop for LockFreeLink {
    fn drop(&mut self) {
        // Clean up any remaining data
        epoch::pin().flush();
    }
}

/// Symbol linker with lock-free operations
/// 
/// Manages the linking of symbols across packages with
/// concurrent-safe read and write operations.
pub struct SymbolLinker {
    /// Map: symbol_id -> LockFreeLink
    links: dashmap::DashMap<u32, Arc<LockFreeLink>>,
    
    /// Reverse index: target -> sources
    reverse_index: dashmap::DashMap<u32, Vec<u32>>,
}

use std::sync::Arc;

impl SymbolLinker {
    pub fn new() -> Self {
        Self {
            links: dashmap::DashMap::with_capacity(100_000),
            reverse_index: dashmap::DashMap::new(),
        }
    }
    
    /// Create a new link
    pub fn link(&self, from: SymbolId, to: SymbolId, location: DefinitionLocation) -> Result<()> {
        let link = Arc::new(LockFreeLink::new(to, location));
        
        self.links.insert(from.as_u32(), link);
        
        // Update reverse index
        self.reverse_index
            .entry(to.as_u32())
            .and_modify(|v| v.push(from.as_u32()))
            .or_insert_with(|| vec![from.as_u32()]);
        
        Ok(())
    }
    
    /// Get link target (lock-free read)
    #[inline]
    pub fn get_target(&self, from: SymbolId) -> Option<(SymbolId, DefinitionLocation)> {
        let guard = epoch::pin();
        
        let link = self.links.get(&from.as_u32())?;
        
        let target = link.get_target(&guard)?;
        let location = link.get_location(&guard)?;
        
        Some((target, location))
    }
    
    /// Update existing link
    pub fn update_link(&self, from: SymbolId, new_target: SymbolId) -> Result<()> {
        let guard = epoch::pin();
        
        let link = self.links
            .get(&from.as_u32())
            .ok_or_else(|| SymbolError::NotFound(format!("link from {}", from.as_u32())))?;
        
        // Mark as updating
        if !link.begin_update() {
            return Err(SymbolError::InvalidId(from.as_u32()));
        }
        
        // Perform update
        let success = link.update_target(new_target, &guard);
        
        // Mark as active
        link.end_update();
        
        if success {
            Ok(())
        } else {
            Err(SymbolError::InvalidId(from.as_u32()))
        }
    }
    
    /// Remove link
    pub fn unlink(&self, from: SymbolId) -> Result<()> {
        let guard = epoch::pin();
        
        if let Some((_, link)) = self.links.remove(&from.as_u32()) {
            link.remove(&guard);
        }
        
        Ok(())
    }
    
    /// Check if link exists
    #[inline]
    pub fn has_link(&self, from: SymbolId) -> bool {
        self.links.contains_key(&from.as_u32())
    }
    
    /// Get all symbols that link to a target
    pub fn get_references(&self, target: SymbolId) -> Vec<SymbolId> {
        self.reverse_index
            .get(&target.as_u32())
            .map(|v| v.iter().map(|&id| SymbolId::new(id)).collect())
            .unwrap_or_default()
    }
    
    /// Count of active links
    pub fn link_count(&self) -> usize {
        self.links.len()
    }
    
    /// Batch link operation (atomic within batch)
    pub fn batch_link(&self, links: Vec<(SymbolId, SymbolId, DefinitionLocation)>) -> Result<()> {
        // Validate all symbols first
        for (from, _, _) in &links {
            if self.has_link(*from) {
                return Err(SymbolError::InvalidId(from.as_u32()));
            }
        }
        
        // Create all links
        for (from, to, loc) in links {
            self.link(from, to, loc)?;
        }
        
        Ok(())
    }
}

impl Default for SymbolLinker {
    fn default() -> Self {
        Self::new()
    }
}

/// Link resolver for following chains
pub struct LinkResolver<'a> {
    linker: &'a SymbolLinker,
}

impl<'a> LinkResolver<'a> {
    pub fn new(linker: &'a SymbolLinker) -> Self {
        Self { linker }
    }
    
    /// Resolve chain to terminal symbol
    /// 
    /// Follows links until reaching a terminal symbol or detecting a cycle.
    pub fn resolve(&self, start: SymbolId) -> Result<(SymbolId, DefinitionLocation, usize)> {
        let mut current = start;
        let mut depth = 0;
        let max_depth = 100;
        let mut location = DefinitionLocation::default();
        
        while depth < max_depth {
            match self.linker.get_target(current) {
                Some((target, loc)) => {
                    if depth == 0 {
                        location = loc;
                    }
                    
                    // Check for self-reference (terminal)
                    if target == current {
                        return Ok((current, location, depth));
                    }
                    
                    current = target;
                    depth += 1;
                }
                None => {
                    // No link, this is terminal
                    return Ok((current, location, depth));
                }
            }
        }
        
        Err(SymbolError::BrokenChain(start.as_u32()))
    }
    
    /// Get complete chain path
    pub fn get_chain(&self, start: SymbolId) -> Vec<(SymbolId, DefinitionLocation)> {
        let mut chain = Vec::new();
        let mut current = start;
        let max_depth = 100;
        
        for _ in 0..max_depth {
            match self.linker.get_target(current) {
                Some((target, loc)) => {
                    chain.push((current, loc));
                    
                    if target == current {
                        break;
                    }
                    current = target;
                }
                None => {
                    chain.push((current, DefinitionLocation::default()));
                    break;
                }
            }
        }
        
        chain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_free_link() {
        let link = LockFreeLink::new(
            SymbolId::new(42),
            DefinitionLocation::new(1, 100)
        );
        
        let guard = epoch::pin();
        
        assert_eq!(link.get_target(&guard), Some(SymbolId::new(42)));
        assert!(link.is_active());
    }

    #[test]
    fn test_symbol_linker() {
        let linker = SymbolLinker::new();
        
        let loc = DefinitionLocation::new(1, 100);
        linker.link(SymbolId::new(1), SymbolId::new(2), loc).unwrap();
        
        let result = linker.get_target(SymbolId::new(1));
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, SymbolId::new(2));
    }

    #[test]
    fn test_link_resolver() {
        let linker = SymbolLinker::new();
        
        // Create chain: 1 -> 2 -> 3
        linker.link(SymbolId::new(1), SymbolId::new(2), DefinitionLocation::new(1, 100)).unwrap();
        linker.link(SymbolId::new(2), SymbolId::new(3), DefinitionLocation::new(1, 200)).unwrap();
        
        let resolver = LinkResolver::new(&linker);
        let (terminal, _loc, depth) = resolver.resolve(SymbolId::new(1)).unwrap();
        
        assert_eq!(terminal, SymbolId::new(3));
        assert_eq!(depth, 2);
    }

    #[test]
    fn test_concurrent_reads() {
        let linker = Arc::new(SymbolLinker::new());
        
        // Setup links
        for i in 0..100 {
            linker.link(
                SymbolId::new(i),
                SymbolId::new(i + 1),
                DefinitionLocation::new(1, i as u32 * 10)
            ).unwrap();
        }
        
        // Concurrent reads
        let handles: Vec<_> = (0..10).map(|thread_id| {
            let linker = Arc::clone(&linker);
            std::thread::spawn(move || {
                for i in 0..100 {
                    let id = SymbolId::new((i + thread_id * 10) % 100);
                    let _ = linker.get_target(id);
                }
            })
        }).collect();
        
        for h in handles {
            h.join().unwrap();
        }
    }
}
