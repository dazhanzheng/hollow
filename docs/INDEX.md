# hollow 文档索引

> 本索引是 hollow 项目文档系统的唯一总入口。AI 助手应从这里开始导航到所需文档。

---

## 产品

- [愿景与哲学](product/vision.md) — hollow 的核心主张、问题背景与设计哲学
- [核心能力](product/core-capabilities.md) — 统一入口、智能摄取、自动归档、自然语言检索
- [交互范式](product/interaction-paradigm.md) — 从路径浏览到意图检索的交互迁移
- [路线图](product/roadmap.md) — 四阶段演进路径与各阶段成功标志
- [风险与边界](product/risks-and-boundaries.md) — 产品风险、设计约束与应对策略

## 架构

- [架构总览](architecture/overview.md) — 五层架构、技术选型、本地优先原则
- [数据模型](architecture/data-model.md) — 文件对象语义模型、索引结构设计
- [混合智能](architecture/hybrid-intelligence.md) — 规则 + 统计 + 向量 + LLM 的分工策略
- [FFI 桥接](architecture/ffi-bridge.md) — Rust ↔ Swift FFI 设计（待填充）

## 模块

- [hollow-core](modules/hollow-core.md) — Rust 核心库：文件解析、索引、本地检索（待填充）
- [hollow-server](modules/hollow-server.md) — 云端 API 代理：LLM/Embedding 转发（待填充）

## 决策记录

（暂无，开发过程中按需添加）
