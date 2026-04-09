# 混合智能

> hollow 不把所有事情都交给 LLM。正确的架构是混合式：规则保证稳定性，统计模型保证效率，向量检索保证语义召回，大模型负责高层理解与交互。

---

## 分工原则

| 任务 | 技术手段 | 运行位置 |
|------|---------|---------|
| 扩展名识别 | 规则系统 | 本地 (hollow-core) |
| 哈希去重 | 确定性算法 | 本地 (hollow-core) |
| 基础 OCR | macOS Vision 框架 | 本地 (Swift) |
| 高精度 OCR | 专门模型 | 云端 (hollow-server) |
| 全文检索 | 倒排索引 | 本地 (hollow-core) |
| 语义检索 | 向量 ANN 搜索 | 本地 (hollow-core) |
| Embedding 生成 | Embedding API | 云端 (hollow-server)，结果存本地 |
| 文档摘要/分类/标签 | LLM API | 云端 (hollow-server) |
| 命名建议 | LLM API | 云端 (hollow-server) |
| 敏感性判断 | LLM API + 规则 | 云端 + 本地 |
| 归档目录推荐 | 规则 + LLM 共同决定 | 本地 + 云端 |
| 查询理解 | LLM API | 云端 (hollow-server) |
| 结果重排 | LLM API | 云端 (hollow-server) |
| 音视频转录 | 专门模型 | 云端 (hollow-server) |

## 设计目标

在成本、速度、精度与可控性之间取得平衡：

- **能用规则的不用模型** — 扩展名识别、哈希去重等确定性任务
- **能在本地跑的不上云** — 全文索引、向量搜索、基础 OCR
- **云端只做本地做不了的** — LLM 推理、Embedding 生成、高精度 OCR、音视频转录
- **结果尽量存本地** — Embedding 向量返回本地存储，云端不保留

---

## 相关文档

- [架构总览](overview.md) — 混合智能在五层架构中的位置
- [核心能力](../product/core-capabilities.md) — 混合智能支撑的产品能力
- [风险与边界](../product/risks-and-boundaries.md) — 多模态成本与性能风险
- [hollow-core](../modules/hollow-core.md) — 本地侧实现
- [hollow-server](../modules/hollow-server.md) — 云端侧实现
