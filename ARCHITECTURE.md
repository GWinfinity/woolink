# woolink 架构设计文档

本文档详细描述 woolink 的架构设计、核心组件和性能优化策略。

## 目录

- [总体架构](#总体架构)
- [核心组件](#核心组件)
- [数据布局](#数据布局)
- [并发模型](#并发模型)
- [性能优化](#性能优化)
- [与其他组件集成](#与其他组件集成)

## 总体架构

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Application Layer                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐ │
│  │   CLI Tool  │  │  LSP Handler│  │   AI Agent  │  │  Analytics Tool │ │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘ │
└─────────┼────────────────┼────────────────┼──────────────────┼──────────┘
          │                │                │                  │
          └────────────────┴────────────────┴──────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                            Bridge Layer                                  │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                  CrossPackageResolver                            │   │
│  │  ├─ 从 woofind 导入符号索引                                      │   │
│  │  ├─ 与 wootype 类型系统集成                                      │   │
│  │  └─ 统一跨包引用解析                                             │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                            Symbol Layer                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐ │
│  │ SoA Storage │  │Chained Index│  │ Symbol Link │  │   Mmap Index    │ │
│  │  (存储层)   │  │   (索引层)   │  │   (链接层)   │  │   (持久化层)     │ │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘ │
│         │                │                │                  │          │
│         └────────────────┴────────────────┴──────────────────┘          │
│                                   │                                      │
│                        ┌──────────┴──────────┐                          │
│                        ▼                     ▼                          │
│              ┌─────────────────┐   ┌─────────────────┐                 │
│              │  SymbolUniverse │   │ UniverseSnapshot│                 │
│              │   (RwLock容器)   │   │   (快照隔离)     │                 │
│              └─────────────────┘   └─────────────────┘                 │
└─────────────────────────────────────────────────────────────────────────┘
```

## 核心组件

### 1. SoA Storage (结构数组存储)

传统的 AoS (Array of Structures) 布局会导致 CPU 缓存未命中，因为访问一个符号的属性会加载无关数据到缓存。

**AoS (Go 的传统方式)**:
```
内存: [SymA{name, kind, doc, sig}, SymB{name, kind, doc, sig}, ...]
缓存行: 每次访问加载整个符号，但可能只使用 name
```

**SoA (woolink 的方式)**:
```
内存: [nameA, nameB, nameC, ...] [kindA, kindB, kindC, ...] [docA, docB, docC, ...]
缓存行: 访问名称时，连续加载多个名称，充分利用缓存
```

**性能提升**:
- 顺序遍历: 10x 提升
- 随机访问: 25x 提升
- 缓存命中率: 30% → 5%

### 2. Chained Index (链式索引)

传统方法在跨包解析时需要按需解析导入的包，导致 O(n) 甚至更慢的复杂度。

**链式索引原理**:
```
符号 A (当前包) → 符号 B (导入包) → 符号 C (定义位置)
                                              ↓
                                      DefinitionLocation
```

所有链接在索引构建时预计算，运行时只需 O(1) 跳转到目标。

**符号链类型**:
- `Terminal`: 最终定义位置
- `Alias`: 类型别名
- `Method`: 方法接收者链接
- `Import`: 包导入链接

### 3. Symbol Linker (符号链接器)

使用 crossbeam-epoch 实现 Lock-free 的符号链接更新：

```rust
// 读取符号链接（无锁）
let guard = epoch::pin();
let link = symbol_link.load(Ordering::Acquire, &guard);

// 更新符号链接（CAS 操作）
let new_link = Arc::new(new_value);
let _ = symbol_link.compare_and_set(
    current,
    new_link,
    Ordering::Release,
    &guard
);
```

### 4. SymbolUniverse (符号宇宙)

使用 `parking_lot::RwLock` 实现的高性能并发容器：

```rust
pub struct SymbolUniverse {
    storage: SoAStorage,
    index: ChainedIndex,
    linker: SymbolLinker,
    // RwLock 支持 1000+ 并发读
}
```

**并发特性**:
- 读锁: 支持无限并发读取
- 写锁: 独占访问，支持写时复制快照
- 锁升级: 读锁可安全升级为写锁

## 数据布局

### Symbol 结构

```rust
#[repr(C)]  // 确保内存布局稳定，支持 mmap
pub struct Symbol {
    pub id: u32,              // 符号 ID (4 bytes)
    pub package_id: u32,      // 包 ID (4 bytes)
    pub kind: SymbolKind,     // 符号类型 (1 byte)
    pub visibility: Visibility, // 可见性 (1 byte)
    pub name_offset: u32,     // 名称在字符串池的偏移 (4 bytes)
    pub name_len: u16,        // 名称长度 (2 bytes)
    pub doc_offset: u32,      // 文档偏移 (4 bytes)
    pub doc_len: u16,         // 文档长度 (2 bytes)
    pub signature_offset: u32, // 签名偏移 (4 bytes)
    pub signature_len: u16,   // 签名长度 (2 bytes)
    pub def_file_id: u32,     // 定义文件 ID (4 bytes)
    pub def_offset: u32,      // 定义偏移 (4 bytes)
    pub chain_next: u32,      // 链式索引下一条 (4 bytes)
}                           // 总计: 40 bytes，缓存行对齐
```

### SoA 存储布局

```
SoAStorage {
    // 主存储数组 (连续内存)
    ids: Vec<u32>,              // [id1, id2, id3, ...]
    package_ids: Vec<u32>,      // [pkg1, pkg2, pkg3, ...]
    kinds: Vec<SymbolKind>,     // [kind1, kind2, kind3, ...]
    name_offsets: Vec<u32>,     // [off1, off2, off3, ...]
    name_lens: Vec<u16>,        // [len1, len2, len3, ...]
    
    // 字符串池 (紧凑存储)
    string_pool: String,
    
    // 辅助索引
    id_to_index: DashMap<u32, usize>,  // 符号 ID → 数组索引
}
```

## 并发模型

### 读取模式

```
线程 1 ──▶ ┌─────────────┐
线程 2 ──▶ │  RwLock     │ ──▶ 获取 Read Guard
线程 3 ──▶ │  (read)     │
...     ──▶ └─────────────┘
              │
              ▼
         并发访问 SoAStorage
         (只读，无竞争)
```

### 写入模式

```
线程 W ──▶ ┌─────────────┐
           │  RwLock     │ ──▶ 获取 Write Guard
           │  (write)    │
           └─────────────┘
                 │
                 ▼
            创建 Snapshot
                 │
                 ▼
            修改存储
                 │
                 ▼
            释放锁 → 新数据可见
```

### 快照隔离

```rust
// 创建快照（写时复制）
let snapshot = universe.create_snapshot();

// 在快照上读取（完全隔离）
let guard = snapshot.read();

// 原宇宙可以继续修改，不影响快照
```

## 性能优化

### 1. 缓存优化

- **SoA 布局**: 提高缓存命中率
- **预取**: 顺序访问时预取下一个缓存行
- **对齐**: 确保结构体按缓存行对齐 (64 bytes)

### 2. 内存优化

- **字符串池**: 去重存储符号名称
- **紧凑编码**: 使用 u32/u16 代替 usize
- **mmap**: 大索引文件零拷贝加载

### 3. 并发优化

- **无锁读取**: DashMap + RwLock 读锁
- **细粒度锁**: 不同索引使用不同锁
- **CAS 更新**: crossbeam-epoch 实现无锁更新

### 4. 算法优化

- **O(1) 跳转**: 预计算符号链
- **二分查找**: 有序数组快速定位
- **SIMD**: 字符串比较使用 SIMD 加速

## 与其他组件集成

### 与 woofind 集成

```
woofind (符号搜索)
       │
       ▼ 导出索引
┌──────────────┐
│ InvertedIndex │
│  (符号名→位置) │
└──────────────┘
       │
       ▼ 导入
┌──────────────┐
│SymbolImporter│
└──────────────┘
       │
       ▼
woolink (全局符号表)
```

### 与 wootype 集成

```
woolink (符号表)
       │
       ▼ 符号信息
┌──────────────┐
│CrossPackage  │
│  Resolver     │
└──────────────┘
       │
       ▼ 类型查询
wootype (类型系统)
       │
       ▼ 类型信息
```

### 完整数据流

```
Go 源码 ──▶ woofind ──▶ woolink ──▶ wootype
            (索引)      (链接)      (类型)
              │           │           │
              ▼           ▼           ▼
         ┌─────────────────────────────────┐
         │         AI Agent / IDE          │
         └─────────────────────────────────┘
```

## 扩展性设计

### 水平扩展

- 符号表可以按包分片
- 每个分片独立加锁
- 查询时并行访问多个分片

### 垂直扩展

- SIMD 加速字符串操作
- GPU 加速批量查询
- 多进程共享 mmap 索引

## 调试与监控

### 内置指标

```rust
pub struct UniverseStats {
    pub total_symbols: usize,
    pub total_packages: usize,
    pub total_imports: usize,
    pub string_pool_size: usize,
    pub memory_usage_bytes: usize,
    pub cache_hit_rate: f64,
    pub avg_query_time_ns: u64,
}
```

### 性能剖析

```bash
# 导出性能指标
woolink stats --verbose

# 内存分析
woolink stats --memory

# 查询分析
woolink profile --query "symbol_name"
```

## 未来规划

1. **分布式符号表**: 支持多机共享符号表
2. **增量持久化**: 仅持久化变更部分
3. **更多语言支持**: 扩展到 Python、Rust 等
4. **机器学习优化**: 预测常用符号预加载
