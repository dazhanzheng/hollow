# hollow-core

> Rust 核心库，通过 FFI 链接进 Swift 应用。负责文件解析、索引构建、本地检索等不需要大模型的全部任务。

<!-- TODO: 待开发时填充。需要定义：模块结构、公开接口、依赖项、数据流。 -->

---

## 职责概述

- 文件类型识别与内容提取（PDF、Word、Excel、PPT、文本、代码）
- 哈希计算与去重
- SQLite 元数据库管理
- 全文索引构建与查询
- 向量索引构建与 ANN 搜索
- 操作日志记录与回滚
- 本地规则引擎
- 通过 FFI 向 Swift 层暴露 C-compatible 接口

---

## 相关文档

- [架构总览](../architecture/overview.md) — hollow-core 在架构中的位置
- [数据模型](../architecture/data-model.md) — hollow-core 实现的数据结构
- [混合智能](../architecture/hybrid-intelligence.md) — hollow-core 负责的本地侧任务
- [FFI 桥接](../architecture/ffi-bridge.md) — 与 Swift 的接口设计
