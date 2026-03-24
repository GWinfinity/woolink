//! woolink 基础用法示例
//!
//! 展示如何：
//! 1. 创建全局符号表
//! 2. 插入符号和包
//! 3. 并发查询
//! 4. 定义跳转

use std::sync::Arc;
use std::thread;

use woolink::{Symbol, SymbolId, SymbolKind, SymbolUniverse, Visibility};

fn main() {
    println!("🚀 woolink 全局符号表示例\n");

    // 创建符号宇宙
    let universe = Arc::new(SymbolUniverse::new(10_000));

    // 插入一些示例数据
    {
        let mut guard = universe.write();

        // 插入包
        let pkg = woolink::symbol::Package {
            id: 1,
            path_offset: 0,
            path_len: 15,
            name_offset: 15,
            name_len: 7,
            version_offset: 0,
            version_len: 0,
            first_symbol: 1,
            symbol_count: 3,
            import_count: 0,
        };
        guard.insert_package(pkg).unwrap();

        // 插入符号
        let symbols = [
            ("NewClient", SymbolKind::Function, 100),
            ("Client", SymbolKind::Type, 200),
            ("Options", SymbolKind::Type, 300),
        ];

        for (i, (name, kind, offset)) in symbols.iter().enumerate() {
            let sym = Symbol {
                id: (i + 1) as u32,
                package_id: 1,
                kind: *kind,
                visibility: Visibility::Public,
                name_offset: 0, // 简化处理
                name_len: name.len() as u16,
                doc_offset: 0,
                doc_len: 0,
                signature_offset: 0,
                signature_len: 0,
                def_file_id: 1,
                def_offset: *offset as u32,
                chain_next: 0,
            };

            guard.insert_symbol(sym).unwrap();
            println!("  ✓ 插入符号: {} ({:?})", name, kind);
        }

        // 设置链：NewClient -> Client
        guard
            .link_alias(SymbolId::new(1), SymbolId::new(2))
            .unwrap();
        println!("  ✓ 创建链接: NewClient -> Client");
    }

    println!("\n📊 统计信息:");
    {
        let guard = universe.read();
        let stats = guard.stats();
        println!("  符号数量: {}", stats.total_symbols);
        println!("  包数量: {}", stats.total_packages);
        println!("  内存使用: {} KB", stats.memory_usage_bytes / 1024);
    }

    // 并发查询演示
    println!("\n🔄 并发查询测试 (10 线程):");
    let mut handles = vec![];

    for thread_id in 0..10 {
        let u = Arc::clone(&universe);
        let handle = thread::spawn(move || {
            let guard = u.read();

            // 查询符号
            if let Some(sym) = guard.get_symbol(SymbolId::new(1)) {
                // O(1) 定义跳转
                match guard.jump_to_definition(SymbolId::new(1)) {
                    Ok((target, location)) => {
                        println!(
                            "  Thread {}: {} -> {} (def at offset {})",
                            thread_id, sym.id, target.id, location.offset
                        );
                    }
                    Err(e) => {
                        println!("  Thread {}: 跳转失败: {}", thread_id, e);
                    }
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    println!("\n✅ 示例完成!");
}
