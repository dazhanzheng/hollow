# hollow-server

> Rust 云端服务，轻量 API 代理。仅负责本机无法高效完成的 AI 任务，不存储用户文件和索引数据。独立于 hollow-core。

<!-- TODO: 待开发时填充。需要定义：API 端点、请求/响应格式、认证机制、部署配置。 -->

---

## 职责概述

- LLM API 代理：摘要、分类、标签、命名建议、敏感性检测、关系推断
- Embedding API 代理：向量生成（结果返回客户端本地存储）
- 查询理解：自然语言 → 检索计划
- 结果重排：语义重排候选结果
- 重计算 AI：高精度 OCR、音视频转录
- 未来：用户账户、多设备同步

---

## 相关文档

- [架构总览](../architecture/overview.md) — hollow-server 在架构中的位置
- [混合智能](../architecture/hybrid-intelligence.md) — hollow-server 负责的云端侧任务
