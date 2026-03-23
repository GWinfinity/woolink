# woolink 🔗 - Go 模块链接分析器

**Woo 生态链组件 #4** - 依赖分析、死码检测、模块重构

## 定位

woolink 是 Woo 生态的链接分析层，负责：
- 模块依赖图构建
- 跨包死码检测
- 循环依赖检测
- 模块重构建议

## 与生态集成

```
┌─────────────────────────────────────────────────────────────┐
│                        woolink                              │
│                   (模块链接分析器)                            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  输入:                                                      │
│  ├─ woofind 的模块索引 (InvertedIndex)                      │
│  ├─ wootype 的类型使用信息 (TypeUniverse)                   │
│  └─ woofmt 的代码结构分析                                    │
│                                                             │
│  功能:                                                      │
│  ├─ 构建 Module Dependency Graph                            │
│  ├─ 检测 Unused Functions/Types (跨模块)                     │
│  ├─ 检测 Import Cycles                                      │
│  ├─ 分析 Interface 实现关系                                  │
│  └─ 提供重构建议 (如: 合并模块、拆分包)                        │
│                                                             │
│  输出:                                                      │
│  ├─ woofmt: 死代码警告                                       │
│  ├─ wootype: 类型依赖关系                                    │
│  └─ IDE: 重构建议                                           │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 功能特性

### 1. 死码检测 (Dead Code Detection)

```go
// pkg/a/a.go
func InternalHelper() {}  // 只在包内使用
func PublicAPI() {}       // 被其他包使用

// pkg/b/b.go  
import "pkg/a"
func init() {
    a.PublicAPI()  // InternalHelper 从未被使用，应标记为死码
}
```

### 2. 循环依赖检测

```
pkg/a ──imports──► pkg/b
   ▲                  │
   └────imports──────┘

woolink 会检测并报告这种循环，并提供打破建议
```

### 3. 接口实现分析

```go
// 找出所有实现了 io.Reader 但未被使用的类型
// 或者找出应该实现某个接口但没有实现的类型
```

## CLI 用法

```bash
# 分析项目
woolink analyze .

# 检测死码
woolink deadcode .

# 检测循环依赖
woolink cycles .

# 生成依赖图
woolink graph --format dot | dot -Tpng > deps.png

# 重构建议
woolink suggest --refactor .
```

## 作为库使用

```rust
use woolink::{ModuleGraph, DeadCodeAnalyzer};
use woofind::index::InvertedIndex;
use wootype::TypeUniverse;

// 构建模块图
let graph = ModuleGraph::build(&inverted_index);

// 检测死码
let analyzer = DeadCodeAnalyzer::new(&graph, &type_universe);
let dead_code = analyzer.find_unused_symbols();

// 输出报告
for item in dead_code {
    println!("未使用: {} in {}", item.name, item.package);
}
```

## 与生态协同

| 输入来源 | 用途 |
|----------|------|
| **woofind** | 获取模块结构、导入关系、符号索引 |
| **wootype** | 获取类型使用信息、接口实现关系 |
| **woofmt** | 获取代码结构、注释信息 |

| 输出目标 | 价值 |
|----------|------|
| **woofmt** | 死码警告、未使用导入检测 |
| **wootype** | 类型依赖分析、接口推荐 |
| **woof** | 统一重构建议 |

## 架构

```
woolink/
├── src/
│   ├── graph/           # 依赖图模块
│   │   ├── mod.rs       # ModuleGraph 定义
│   │   ├── builder.rs   # 从索引构建图
│   │   └── algorithms.rs # 图算法 (Tarjan, etc.)
│   ├── analyze/         # 分析器
│   │   ├── deadcode.rs  # 死码检测
│   │   ├── cycles.rs    # 循环检测
│   │   └── refactor.rs  # 重构建议
│   ├── cli/             # 命令行
│   └── lib.rs
```

## 性能目标

- 1000 个模块分析: < 1 秒
- 死码检测: < 100ms (基于 woofind 索引)
- 循环检测: < 50ms

## 状态

🚧 **开发中** - 基础架构设计完成，等待集成
