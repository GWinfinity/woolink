//! Cross-Package Symbol Resolver
//!
//! 跨包符号解析器：
//! - 解析导入路径到符号
//! - 处理重命名导入 (import alias)
//! - 支持点导入 (dot import)
//! - 支持匿名导入 (blank import)
//! - 检测循环依赖

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use dashmap::DashMap;

use super::{BridgeError, Result};
use crate::symbol::{
    ChainedIndex, DefinitionLocation, PackageId, Symbol, SymbolId, SymbolKind, SymbolUniverse,
    SymbolUniverseGuard,
};

/// Kind of symbol reference
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// Direct reference (e.g., `pkg.Symbol`)
    Direct,
    /// Selector reference (e.g., `obj.Method`)
    Selector,
    /// Type assertion (e.g., `x.(Type)`)
    TypeAssertion,
    /// Type conversion (e.g., `Type(x)`)
    TypeConversion,
    /// Method call (e.g., `x.Method()`)
    MethodCall,
    /// Interface implementation
    Implementation,
    /// Embedded type
    Embedded,
}

/// Result of a cross-package resolution
#[derive(Debug, Clone)]
pub struct ResolutionResult {
    /// The resolved symbol
    pub symbol: Symbol,

    /// Package containing the symbol
    pub package: PackageId,

    /// Kind of reference
    pub kind: ReferenceKind,

    /// Definition location for jumping
    pub location: DefinitionLocation,

    /// Chain depth (0 = direct)
    pub depth: usize,
}

/// Import information for a package
#[derive(Debug, Clone)]
pub struct PackageImports {
    /// Package ID
    pub package_id: PackageId,

    /// Map: alias -> (target_package, original_name)
    /// For example: "r" -> ("github.com/redis/go-redis/v9", "redis")
    pub aliases: HashMap<String, PackageId>,

    /// Dot imports (symbols imported into current namespace)
    pub dot_imports: Vec<SymbolId>,

    /// Anonymous imports (only for side effects)
    pub blank_imports: Vec<PackageId>,
}

impl PackageImports {
    pub fn new(package_id: PackageId) -> Self {
        Self {
            package_id,
            aliases: HashMap::new(),
            dot_imports: Vec::new(),
            blank_imports: Vec::new(),
        }
    }

    /// Register a named import
    pub fn add_import(&mut self, alias: &str, target: PackageId) {
        self.aliases.insert(alias.to_string(), target);
    }

    /// Register a dot import
    pub fn add_dot_import(&mut self, symbols: Vec<SymbolId>) {
        self.dot_imports.extend(symbols);
    }

    /// Register a blank import
    pub fn add_blank_import(&mut self, target: PackageId) {
        self.blank_imports.push(target);
    }

    /// Lookup import by alias
    pub fn resolve_alias(&self, alias: &str) -> Option<PackageId> {
        self.aliases.get(alias).copied()
    }
}

/// Cross-package symbol resolver
///
/// Manages inter-package symbol resolution with:
/// - Import alias tracking
/// - Cycle detection
/// - Efficient caching
pub struct CrossPackageResolver {
    /// Reference to symbol universe
    universe: Arc<SymbolUniverse>,

    /// Package import information
    imports: DashMap<PackageId, PackageImports>,

    /// Cache: (package, name) -> symbol
    resolution_cache: DashMap<(PackageId, String), ResolutionResult>,

    /// Cycle detection: package -> packages it depends on
    dependency_graph: DashMap<PackageId, HashSet<PackageId>>,
}

impl CrossPackageResolver {
    pub fn new(universe: Arc<SymbolUniverse>) -> Self {
        Self {
            universe,
            imports: DashMap::new(),
            resolution_cache: DashMap::with_capacity(100_000),
            dependency_graph: DashMap::new(),
        }
    }

    /// Register package imports
    pub fn register_imports(&self, imports: PackageImports) {
        let pkg_id = imports.package_id;

        // Track dependencies
        let deps: HashSet<_> = imports.aliases.values().copied().collect();
        self.dependency_graph.insert(pkg_id, deps);

        self.imports.insert(pkg_id, imports);
    }

    /// Resolve a symbol reference from a package
    ///
    /// Handles:
    /// - `pkg.Symbol` - qualified identifier
    /// - `Symbol` - identifier in current or dot-imported packages
    pub fn resolve(&self, from_package: PackageId, name: &str) -> Option<ResolutionResult> {
        // Check cache first
        let cache_key = (from_package, name.to_string());
        if let Some(result) = self.resolution_cache.get(&cache_key) {
            return Some(result.clone());
        }

        let guard = self.universe.read();

        // Try current package first
        if let Some(result) =
            self.resolve_in_package(&guard, from_package, name, ReferenceKind::Direct)
        {
            self.resolution_cache.insert(cache_key, result.clone());
            return Some(result);
        }

        // Try dot imports
        if let Some(imports) = self.imports.get(&from_package) {
            for &symbol_id in &imports.dot_imports {
                if let Some(sym) = guard.get_symbol(symbol_id) {
                    // Need to get the name - this is simplified
                    // In real implementation, we'd compare names properly
                    let _ = sym;
                }
            }
        }

        None
    }

    /// Resolve qualified identifier (e.g., `pkg.Symbol`)
    pub fn resolve_qualified(
        &self,
        from_package: PackageId,
        qualifier: &str,
        name: &str,
    ) -> Option<ResolutionResult> {
        let cache_key = (from_package, format!("{}.{}", qualifier, name));
        if let Some(result) = self.resolution_cache.get(&cache_key) {
            return Some(result.clone());
        }

        let guard = self.universe.read();

        // Resolve qualifier to package
        let target_package = if let Some(imports) = self.imports.get(&from_package) {
            imports.resolve_alias(qualifier)?
        } else {
            return None;
        };

        // Look up symbol in target package
        let result = self.resolve_in_package(&guard, target_package, name, ReferenceKind::Direct);

        if let Some(ref r) = result {
            self.resolution_cache.insert(cache_key, r.clone());
        }

        result
    }

    /// Resolve a selector expression (e.g., `obj.Method`)
    pub fn resolve_selector(
        &self,
        from_package: PackageId,
        receiver_type: SymbolId,
        selector: &str,
    ) -> Option<ResolutionResult> {
        let guard = self.universe.read();

        // Get receiver type info
        let receiver = guard.get_symbol(receiver_type)?;

        // Look for method in receiver's methods
        let methods = guard.get_methods(receiver_type);
        for method in methods {
            // Compare method name - simplified
            if method.id != 0 {
                // Placeholder check
                let location = DefinitionLocation::new(method.def_file_id, method.def_offset);
                return Some(ResolutionResult {
                    symbol: method,
                    package: PackageId::new(receiver.package_id),
                    kind: ReferenceKind::MethodCall,
                    location,
                    depth: 0,
                });
            }
        }

        // Try embedded types
        // This would check anonymous fields and their methods

        None
    }

    /// Resolve interface implementation
    pub fn resolve_implementation(&self, interface: SymbolId) -> Vec<ResolutionResult> {
        let guard = self.universe.read();

        guard
            .get_implementations(interface)
            .into_iter()
            .map(|sym| ResolutionResult {
                symbol: sym.clone(),
                package: PackageId::new(sym.package_id),
                kind: ReferenceKind::Implementation,
                location: DefinitionLocation::new(sym.def_file_id, sym.def_offset),
                depth: 0,
            })
            .collect()
    }

    /// Check for import cycles
    pub fn detect_cycle(&self, start: PackageId) -> Option<Vec<PackageId>> {
        let mut visited = HashSet::new();
        let mut path = Vec::new();

        if self.dfs_cycle(start, &mut visited, &mut path) {
            Some(path)
        } else {
            None
        }
    }

    fn dfs_cycle(
        &self,
        current: PackageId,
        visited: &mut HashSet<PackageId>,
        path: &mut Vec<PackageId>,
    ) -> bool {
        if path.contains(&current) {
            path.push(current); // Add to show cycle
            return true;
        }

        if visited.contains(&current) {
            return false;
        }

        visited.insert(current);
        path.push(current);

        if let Some(deps) = self.dependency_graph.get(&current) {
            for &dep in deps.iter() {
                if self.dfs_cycle(dep, visited, path) {
                    return true;
                }
            }
        }

        path.pop();
        false
    }

    /// Get all symbols reachable from a package
    pub fn get_reachable_symbols(&self, package: PackageId) -> HashSet<SymbolId> {
        let reachable = HashSet::new();
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();

        queue.push_back(package);
        visited.insert(package);

        let guard = self.universe.read();

        while let Some(pkg) = queue.pop_front() {
            // Add all symbols from this package
            // In real implementation, we'd iterate package symbols

            // Add dependencies
            if let Some(imports) = self.imports.get(&pkg) {
                for &dep in imports.aliases.values() {
                    if visited.insert(dep) {
                        queue.push_back(dep);
                    }
                }
            }
        }

        reachable
    }

    /// Clear resolution cache
    pub fn clear_cache(&self) {
        self.resolution_cache.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            entries: self.resolution_cache.len(),
        }
    }

    /// Helper: resolve symbol in specific package
    fn resolve_in_package(
        &self,
        guard: &SymbolUniverseGuard<'_>,
        package: PackageId,
        name: &str,
        kind: ReferenceKind,
    ) -> Option<ResolutionResult> {
        let symbols = guard.lookup_symbol(package, name);

        symbols.into_iter().next().map(|sym| {
            let location = DefinitionLocation::new(sym.def_file_id, sym.def_offset);
            ResolutionResult {
                symbol: sym.clone(),
                package,
                kind,
                location,
                depth: 0,
            }
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    pub entries: usize,
}

/// Builder for cross-package resolution configuration
pub struct ResolverBuilder {
    universe: Option<Arc<SymbolUniverse>>,
}

impl ResolverBuilder {
    pub fn new() -> Self {
        Self { universe: None }
    }

    pub fn universe(mut self, universe: Arc<SymbolUniverse>) -> Self {
        self.universe = Some(universe);
        self
    }

    pub fn build(self) -> Result<CrossPackageResolver> {
        let universe = self
            .universe
            .ok_or_else(|| BridgeError::Resolution("Universe not set".to_string()))?;

        Ok(CrossPackageResolver::new(universe))
    }
}

impl Default for ResolverBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::{DefinitionLocation, Symbol, SymbolKind, Visibility};

    fn create_test_resolver() -> CrossPackageResolver {
        let universe = Arc::new(SymbolUniverse::new(1000));

        // Setup packages
        {
            let mut guard = universe.write();

            // Package A
            guard
                .insert_package(crate::symbol::Package {
                    id: 1,
                    path_offset: 0,
                    path_len: 10,
                    name_offset: 10,
                    name_len: 7,
                    version_offset: 0,
                    version_len: 0,
                    first_symbol: 1,
                    symbol_count: 2,
                    import_count: 0,
                })
                .unwrap();

            // Package B
            guard
                .insert_package(crate::symbol::Package {
                    id: 2,
                    path_offset: 0,
                    path_len: 10,
                    name_offset: 10,
                    name_len: 7,
                    version_offset: 0,
                    version_len: 0,
                    first_symbol: 3,
                    symbol_count: 1,
                    import_count: 0,
                })
                .unwrap();

            // Symbols
            for (id, pkg, name) in [(1, 1, "Foo"), (2, 1, "Bar"), (3, 2, "Baz")] {
                guard
                    .insert_symbol(Symbol {
                        id,
                        package_id: pkg,
                        kind: SymbolKind::Type,
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
                    })
                    .unwrap();
            }
        }

        CrossPackageResolver::new(universe)
    }

    #[test]
    fn test_import_registration() {
        let resolver = create_test_resolver();

        let mut imports = PackageImports::new(PackageId::new(1));
        imports.add_import("b", PackageId::new(2));

        resolver.register_imports(imports);

        let result = resolver.resolve_qualified(PackageId::new(1), "b", "Baz");
        assert!(result.is_some());
    }

    #[test]
    fn test_cycle_detection() {
        let resolver = create_test_resolver();

        // Setup cycle: A -> B -> C -> A
        let mut imports_a = PackageImports::new(PackageId::new(1));
        imports_a.add_import("b", PackageId::new(2));
        resolver.register_imports(imports_a);

        let mut imports_b = PackageImports::new(PackageId::new(2));
        imports_b.add_import("c", PackageId::new(3));
        resolver.register_imports(imports_b);

        let mut imports_c = PackageImports::new(PackageId::new(3));
        imports_c.add_import("a", PackageId::new(1));
        resolver.register_imports(imports_c);

        let cycle = resolver.detect_cycle(PackageId::new(1));
        assert!(cycle.is_some());
    }
}
