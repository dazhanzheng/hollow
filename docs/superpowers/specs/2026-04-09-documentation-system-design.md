# hollow 渐进式文档系统设计

## 概述

为 hollow 项目创建一个以 AI 助手为主要受众的渐进式文档系统。文档间使用标准 Markdown 链接实现双向导航，结构按领域分层，内容随开发渐进填充。

## 设计决策

- **主要受众**：AI 助手（Claude Code 等），开发者偶尔查阅
- **链接格式**：标准 Markdown 相对路径链接 `[文本](路径.md)`，不使用 Obsidian `[[wikilinks]]`
- **与 CLAUDE.md 的关系**：CLAUDE.md 瘦身为构建命令 + 简要指引，架构详细内容迁移到文档系统，CLAUDE.md 中加入 `docs/INDEX.md` 的链接入口
- **结构方案**：按领域分子文件夹 + 全局 INDEX.md 入口（方案二）

## 文件夹结构

```
docs/
├── INDEX.md                        # 总入口，全局导航
├── product/                        # 产品层（源自白皮书拆分）
│   ├── vision.md                   # 核心主张、哲学
│   ├── core-capabilities.md        # 统一入口、智能摄取、自动归档、检索
│   ├── interaction-paradigm.md     # 交互范式
│   ├── roadmap.md                  # 四阶段路线图
│   └── risks-and-boundaries.md    # 风险、边界、挑战
├── architecture/                   # 技术架构层
│   ├── overview.md                 # 五层架构总览、技术选型
│   ├── data-model.md               # 文件对象数据模型
│   ├── hybrid-intelligence.md      # 混合智能策略
│   └── ffi-bridge.md               # Rust ↔ Swift FFI 设计（待填充）
├── modules/                        # 模块设计（随开发渐进填充）
│   ├── hollow-core.md              # Rust 核心库
│   └── hollow-server.md            # 云端 API 代理
└── decisions/                      # 架构决策记录（按需添加）
```

## 文档模板

每个文档统一遵循以下结构：

```markdown
# 标题

> 一句话摘要（让 AI 快速判断是否与当前任务相关）

---

## 正文章节...

---

## 相关文档
- [文档名](相对路径.md) — 关系说明
```

- 一句话摘要是必须的，放在标题下方，用 blockquote 格式
- 正文按主题自由组织章节
- 底部"相关文档"区域列出反向链接和相关文档

## 双向链接约定

### 正向链接

文档正文中提到另一个文档覆盖的概念时，使用标准 Markdown 链接指向目标文档：

```markdown
系统采用[混合智能](../architecture/hybrid-intelligence.md)策略，不把所有事情都交给 LLM。
```

### 反向链接

被链接的目标文档，在底部"相关文档"区域中列出指回来源文档的链接：

```markdown
## 相关文档
- [架构总览](overview.md) — 在五层架构中引用了本文档
- [核心能力](../product/core-capabilities.md) — 语义理解部分依赖混合智能策略
```

维护规则：每次在文档 A 中新增指向文档 B 的正向链接时，同步在文档 B 的"相关文档"中加上指回 A 的条目。

## INDEX.md 结构

作为唯一总入口，按领域分区，每条一行简介：

```markdown
# hollow 文档索引

## 产品
- [愿景与哲学](product/vision.md) — 核心主张与设计哲学
- [核心能力](product/core-capabilities.md) — 统一入口、智能摄取、自动归档、检索
- [交互范式](product/interaction-paradigm.md) — 投递、描述目标、任务集合
- [路线图](product/roadmap.md) — 四阶段演进路径
- [风险与边界](product/risks-and-boundaries.md) — 风险、挑战、设计约束

## 架构
- [架构总览](architecture/overview.md) — 五层架构、技术选型、本地优先
- [数据模型](architecture/data-model.md) — 文件对象模型、索引结构
- [混合智能](architecture/hybrid-intelligence.md) — 规则 + 统计 + 向量 + LLM 分工
- [FFI 桥接](architecture/ffi-bridge.md) — Rust ↔ Swift FFI 设计（待填充）

## 模块
- [hollow-core](modules/hollow-core.md) — Rust 核心库（待填充）
- [hollow-server](modules/hollow-server.md) — 云端 API 代理（待填充）

## 决策记录
（暂无，开发过程中按需添加）
```

## CLAUDE.md 变更

### 保留内容
- 项目一句话简介
- Build & Run 命令（Swift + Rust）
- Tech Stack 列表
- Bundle ID

### 迁移内容
- Architecture 章节的详细描述 → `architecture/overview.md` + `modules/*.md`

### 新增内容
- 一个"Documentation"段落，指向 `docs/INDEX.md`，说明完整项目文档在文档系统中

## 初始内容来源

产品层文档内容从 `WHITEPAPER.md` 中提取并结构化：

| 白皮书章节 | 目标文档 |
|-----------|---------|
| 一（问题背景）+ 二（核心主张）| `product/vision.md` |
| 三（产品定义：核心能力）| `product/core-capabilities.md` |
| 四（交互范式）| `product/interaction-paradigm.md` |
| 九（路线图）| `product/roadmap.md` |
| 八（风险边界）| `product/risks-and-boundaries.md` |

架构层文档内容从白皮书第五章 + CLAUDE.md 的 Architecture 章节提取：

| 来源 | 目标文档 |
|------|---------|
| 白皮书 §5.1-5.2 + CLAUDE.md Architecture | `architecture/overview.md` |
| 白皮书 §5.3 | `architecture/data-model.md` |
| 白皮书 §5.4 | `architecture/hybrid-intelligence.md` |

## 渐进填充策略

- 产品层文档在本次创建时即填充完整内容（从白皮书提取）
- 架构层文档填充已有内容，开发过程中持续补充
- 模块文档和 FFI 文档创建骨架，标记 `<!-- TODO: 待开发时填充 -->`
- 决策记录文件夹留空，开发中做重要技术决策时添加
