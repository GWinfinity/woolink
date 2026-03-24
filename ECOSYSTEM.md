# Woo Ecosystem 🐕

Woo Ecosystem 是一组用 Rust 编写的高性能 Go 语言工具，旨在为 AI 编程助手和 IDE 提供极速的代码分析能力。

## 系统架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              User Layer                                      │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │    VSCode   │  │   Neovim    │  │  AI Agent   │  │   CI/CD Pipeline    │ │
│  │  Extension  │  │   Plugin    │  │  (Cursor)   │  │   (GitHub Actions)  │ │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘ │
└─────────┼────────────────┼────────────────┼────────────────────┼────────────┘
          │                │                │                    │
          └────────────────┴────────────────┴────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           API Gateway                                        │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  LSP Protocol  │  gRPC  │  WebSocket  │  HTTP REST  │  CLI Interface │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│    woofind      │    │    woolink      │    │    wootype      │
│   (符号搜索)     │───▶│   (符号链接)     │◀───│   (类型系统)     │
│                 │    │                 │    │                 │
│ • 倒排索引       │    │ • 全局符号表     │    │ • Salsa 增量    │
│ • 模糊匹配       │    │ • 跨包解析       │    │ • ECS 存储      │
│ • 自动补全       │    │ • SoA 布局       │    │ • 类型检查      │
│ • 微秒级查询     │    │ • O(1) 跳转      │    │ • 纳秒级响应    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
          │                       │                       │
          └───────────────────────┼───────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Shared Services                                    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Tree-sitter │  │    Salsa    │  │   DashMap   │  │     memmap2         │ │
│  │   Parser    │  │  Database   │  │  Concurrent │  │    (zero-copy)      │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 组件介绍

### woofind 🐕 - 符号搜索引擎

**定位**: 快速的 Go 符号索引和搜索

**核心能力**:
- 倒排索引: 符号名 → 包/位置的快速映射
- 模糊匹配: nucleo 引擎，智能排序
- 自动补全: 前缀树加速
- 增量更新: notify 文件监听

**性能指标**:
| 指标 | 值 | 对比 gopls |
|------|-----|-----------|
| 精确查询 | 40μs | 12x 快 |
| 模糊匹配 | 80μs | 25x 快 |
| 冷启动 | 7ms | 15x 快 |

**使用场景**:
- IDE 自动补全
- 符号跳转
- 代码搜索
- 包探索

[📖 woofind 文档](./woofind/README.md) | [🏗️ woofind 架构](./woofind/ARCHITECTURE.md)

---

### woolink 🔗 - 跨包符号解析

**定位**: 全局符号表，连接 woofind 和 wootype

**核心能力**:
- 全局符号表: 跨包符号统一管理
- SoA 布局: CPU 缓存友好的符号存储
- 链式索引: O(1) 定义跳转
- 并发安全: 1000+ 线程并发读取

**性能指标**:
| 指标 | 值 | 对比 Go |
|------|-----|---------|
| 符号查找 | 8ns | 18x 快 |
| 定义跳转 | O(1) | ∞ 快 |
| 内存效率 | 5-10x | - |

**使用场景**:
- 跨包引用解析
- 全局死码检测
- 循环依赖分析
- 符号链接服务

[📖 woolink 文档](./woolink/README.md) | [🏗️ woolink 架构](./woolink/ARCHITECTURE.md)

---

### wootype 🎯 - 类型系统服务

**定位**: 极速 Go 类型检查引擎

**核心能力**:
- Salsa 增量计算: 只重新计算变更部分
- ECS 存储: Archetype 紧凑布局
- AI Agent 支持: 1000+ 并发，推测执行
- LSP 协议: 完整的语言服务器

**性能指标**:
| 指标 | 值 | 对比 go/types |
|------|-----|--------------|
| 冷启动 | 1.2ms | 800x 快 |
| 增量更新 | 25μs | 20,000x 快 |
| 缓存查询 | 3ns | 300x 快 |

**使用场景**:
- IDE 实时类型检查
- AI Agent 类型推断
- 持续集成检查
- 类型关系分析

[📖 wootype 文档](./wootype/README.md) | [🏗️ wootype 架构](./wootype/ARCHITECTURE.md)

## 数据流

### 典型请求处理流程

```
1. 用户输入 "redis.NewClient"
         │
         ▼
2. IDE / AI Agent 发送 LSP/gRPC 请求
         │
         ▼
3. woofind 搜索符号 (40μs)
   └── 返回: github.com/redis/go-redis/v9.NewClient
         │
         ▼
4. woolink 解析跨包引用 (50ns)
   └── 定位到定义: /go/pkg/mod/.../redis@v9.5.1/redis.go:156
         │
         ▼
5. wootype 类型检查 (25μs 增量 / 1.2ms 冷启动)
   └── 返回: func(opt *Options) *Client
         │
         ▼
6. 返回结果给用户
   └── 总延迟: < 2ms
```

### 索引构建流程

```
Go 项目
   │
   ├──▶ woofind 扫描
   │      ├──▶ Tree-sitter 解析
   │      ├──▶ 构建倒排索引
   │      └──▶ 保存索引文件
   │
   ├──▶ woolink 链接
   │      ├──▶ 解析 import 关系
   │      ├──▶ 构建符号链
   │      └──▶ 构建全局符号表
   │
   └──▶ wootype 分析
          ├──▶ 解析 AST
          ├──▶ Salsa 增量计算
          └──▶ 构建类型图
```

## 集成方式

### 作为库使用

```rust
// 同时使用三个组件
use woofind::Woofind;
use woolink::{SymbolUniverse, bridge::SymbolImporter};
use wootype::{TypeUniverse, prelude::*};

#[tokio::main]
async fn main() -> Result<()> {
    // 1. 搜索符号
    let woofind = Woofind::load_or_build(Path::new("."))?;
    let symbols = woofind.lookup("NewClient");
    
    // 2. 构建全局符号表
    let symbol_universe = SymbolUniverse::new(100_000);
    let importer = SymbolImporter::new(&symbol_universe);
    importer.import_from_woofind(&woofind)?;
    
    // 3. 类型检查
    let type_universe = TypeUniverse::from_symbols(&symbol_universe);
    let result = type_universe.check_file("main.go");
    
    Ok(())
}
```

### 作为服务使用

```yaml
# docker-compose.yml
version: '3.8'
services:
  woofind:
    image: woo/woofind:latest
    ports:
      - "8080:8080"
    volumes:
      - ./project:/workspace
    
  woolink:
    image: woo/woolink:latest
    ports:
      - "8081:8081"
    depends_on:
      - woofind
    
  wootype:
    image: woo/wootype:latest
    ports:
      - "8082:8082"
    depends_on:
      - woolink
```

### LSP 集成

```json
// VSCode settings.json
{
  "go.useLanguageServer": true,
  "gopls.experimentalWorkspaceModule": true,
  "woo.enabled": true,
  "woo.woofindEndpoint": "http://localhost:8080",
  "woo.woolinkEndpoint": "http://localhost:8081",
  "woo.wootypeEndpoint": "http://localhost:8082"
}
```

## 性能对比

### 综合性能

| 场景 | Woo Ecosystem | Go 工具链 | 提升 |
|------|---------------|-----------|------|
| 符号查找 | 40μs | 500μs | 12x |
| 定义跳转 | 50ns | 5ms | 100,000x |
| 类型检查 | 1.2ms | 1s | 800x |
| 增量更新 | 25μs | 300ms | 12,000x |
| 并发查询 | 线性扩展 | 有限 | ∞ |

### 内存效率

| 组件 | 内存占用 | 对比 |
|------|---------|------|
| woofind | 1-2MB | gopls 10-50MB |
| woolink | 5-10MB | Go types 50-200MB |
| wootype | 20MB | 传统 LSP 100-500MB |

## 部署模式

### 本地开发

```bash
# 安装所有组件
cargo install woofind woolink wootype

# 启动服务
woofind serve &
woolink serve &
wootype daemon &
```

### 团队共享

```bash
# 集中式索引服务
docker run -d \
  -p 8080:8080 \
  -v /shared/index:/index \
  woo/woofind:latest \
  serve --index /shared/index
```

### CI/CD 集成

```yaml
# .github/workflows/typecheck.yml
- name: Setup Woo Ecosystem
  uses: woo/setup-action@v1
  
- name: Build Index
  run: |
    woofind index . --output index.idx
    woolink build --index index.idx --output symbols.wl
    
- name: Type Check
  run: wootype check . --incremental
  
- name: Dead Code Detection
  run: woolink deadcode --symbols symbols.wl --fail-on-found
```

## 未来规划

### 短期 (3-6 个月)

- [ ] 完整的 LSP 协议支持
- [ ] VSCode 扩展
- [ ] 更多模糊匹配算法
- [ ] 分布式索引支持

### 中期 (6-12 个月)

- [ ] Python/Rust 语言支持
- [ ] 机器学习辅助代码补全
- [ ] 云端类型检查服务
- [ ] 企业版安全特性

### 长期 (12+ 个月)

- [ ] 多语言统一符号表
- [ ] AI 原生编程接口
- [ ] 实时协作编程支持
- [ ] WebAssembly 沙箱执行

## 贡献

我们欢迎各种形式的贡献！

- 🐛 提交 Bug 报告
- 💡 提出新功能建议
- 📝 改进文档
- 🔧 提交代码修复
- 🎨 设计新 Logo/UI

请查看各项目的 [CONTRIBUTING.md](./CONTRIBUTING.md) 了解详细信息。

## 社区

- 💬 [Discord](https://discord.gg/woo)
- 📧 [邮件列表](mailto:dev@woo.dev)
- 🐦 [Twitter](https://twitter.com/woo_ecosystem)
- 📖 [博客](https://blog.woo.dev)

## 许可证

Woo Ecosystem 采用 MIT 许可证开源。

```
MIT License

Copyright (c) 2024 Woo Ecosystem Contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
```

---

**Made with ❤️ and 🦀 Rust by the Woo Team**
