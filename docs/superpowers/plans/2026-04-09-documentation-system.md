# Documentation System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a progressive documentation system with bidirectional Markdown links for the hollow project, primarily serving AI assistants.

**Architecture:** A `docs/` folder tree organized by domain (product, architecture, modules, decisions) with a single `INDEX.md` entry point. Product docs are extracted from `WHITEPAPER.md`; architecture docs combine whitepaper §5 and `CLAUDE.md`; module/FFI docs start as skeletons. Every doc has a "相关文档" backlinks section. `CLAUDE.md` is slimmed down and points to the doc system.

**Tech Stack:** Markdown with standard relative-path links.

**Spec:** `docs/superpowers/specs/2026-04-09-documentation-system-design.md`

---

## Task 1: Create folder structure and INDEX.md

**Files:**
- Create: `docs/INDEX.md`
- Create: `docs/product/` (directory)
- Create: `docs/architecture/` (directory)
- Create: `docs/modules/` (directory)
- Create: `docs/decisions/` (directory)

- [ ] **Step 1: Create directories**

```bash
mkdir -p docs/product docs/architecture docs/modules docs/decisions
```

- [ ] **Step 2: Create INDEX.md**

Create `docs/INDEX.md` with this exact content:

```markdown
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
```

- [ ] **Step 3: Commit**

```bash
git add docs/INDEX.md
git commit -m "docs: create documentation system folder structure and INDEX.md"
```

---

## Task 2: Create product/vision.md

**Files:**
- Create: `docs/product/vision.md`
- Source: `WHITEPAPER.md` §一（问题背景）+ §二（核心主张）+ §六（产品设计原则）+ §十（长期意义）

- [ ] **Step 1: Create vision.md**

Create `docs/product/vision.md` with this content:

```markdown
# 愿景与哲学

> hollow 是一个以 AI 驱动的、以语义为中心的个人文件摄取、理解、归档与检索系统。用户通过"意义"而非"路径"管理文件。

---

## 问题背景

传统文件系统要求用户通过路径和目录管理文件，但人类记忆是语义性的。用户记得的是：

- 文件大致是什么内容
- 和哪个项目、客户、事件有关
- 大概什么时间得到的
- 来自谁、来自哪里
- 它长什么样、与其他文件有什么关联

这是**语义记忆**、**情境记忆**、**关系记忆**与**任务记忆**的混合体，而非路径记忆。路径式文件系统要求用户把自己的记忆方式翻译成计算机的路径方式，这种转换带来持续摩擦。

现代文件管理的核心痛点不是"存不下"，而是"找不到"：目录失控、文件命名失效、人工分类成本过高、搜索能力与真实需求不匹配。

## hollow 是什么

hollow 的核心工作方式：

1. 用户把文件扔进一个统一入口
2. hollow 自动吸入、理解、标注、归档、索引
3. 用户通过自然语言、结构化条件或视觉线索快速找回文件

hollow 不取代底层文件系统，而是在其之上建立一个**语义层**（认知层）。文件从静态路径对象变为可被解释、可被召回、可被组织进语义网络中的数字资产。

## 设计哲学

1. **文件系统应该适应人** — 人的真实工作方式应成为系统设计起点
2. **搜索是主交互方式** — 检索不是功能补充，而是系统的出口核心
3. **自动化以信任为前提** — 提供可解释性、可回滚性、可纠正性和渐进自动化
4. **文件不是孤立对象** — 理解文件之间的关系，支持任务集合视图
5. **本地优先** — 敏感文件不应被默认外传，本地优先是基础信任条件
6. **系统必须允许被教会** — 用户纠正一次，系统就应在未来更接近用户习惯
7. **结果必须可解释** — "为什么找到它"比"找到它"本身更重要

## 长期意义

- 从"储存信息"到"理解信息"
- 从"人工秩序"到"机器协助秩序"
- 从"路径世界"到"语义世界"

hollow 的愿景不是整理文件，而是重写人与文件之间的关系。

---

## 相关文档

- [核心能力](core-capabilities.md) — hollow 的具体产品能力定义
- [交互范式](interaction-paradigm.md) — 基于愿景的交互设计
- [路线图](roadmap.md) — 愿景的分阶段实现路径
- [架构总览](../architecture/overview.md) — 愿景的技术实现架构
```

- [ ] **Step 2: Commit**

```bash
git add docs/product/vision.md
git commit -m "docs: add product vision and philosophy"
```

---

## Task 3: Create product/core-capabilities.md

**Files:**
- Create: `docs/product/core-capabilities.md`
- Source: `WHITEPAPER.md` §三（产品定义：核心能力）

- [ ] **Step 1: Create core-capabilities.md**

Create `docs/product/core-capabilities.md` with this content:

```markdown
# 核心能力

> hollow 的四大产品能力：统一入口、智能摄取、自动归档、自然语言检索。

---

## 统一入口

hollow 提供一个明确、低摩擦、几乎零学习成本的入口：

- **入口文件夹**：用户指定一个目录（如 `Hollow Inbox`）作为摄取区，系统监听此目录自动开始处理
- **桌面入口 UI**：浮窗/托盘组件，支持拖拽投递、多文件批量投递、投递时附加语义说明
- **未来扩展**：浏览器下载接管、邮件附件、聊天文件、手机扫描件、云盘同步、剪贴板/截图

理念不变：**一切先进入 hollow 的理解层。**

## 智能摄取

文件进入 hollow 后，系统依次完成：

### 文件基础识别

文件名、类型、大小、时间戳、来源路径、来源应用、哈希指纹、重复检测。

### 内容提取

按文件类型差异化解析：

| 类型 | 提取内容 |
|------|---------|
| PDF | 正文、版面识别、页级摘要 |
| Word | 正文、标题、注释、元数据 |
| Excel | 工作表名、表头、关键单元格、结构摘要 |
| PPT | 标题、演讲页概念、图文描述 |
| 图片/截图 | OCR、视觉内容识别、场景特征、二维码 |
| 压缩包 | 目录结构、内部文件快速分析 |
| 音视频 | 转录、说话人切分、时间轴摘要 |
| 代码/文本 | 语言识别、项目语义、依赖特征 |

### 语义理解

生成文件语义画像：

- **类别**：合同 / 发票 / 报告 / 简历 / 研究论文 / 会议纪要 / 截图
- **主题标签**：报税 / 日本签证 / 项目名 / 客户名
- **时间标签**：2025Q4 / 报税季 / 上周会议
- **实体标签**：公司名 / 人名 / 地点 / 机构
- **操作状态**：待处理 / 已归档 / 需确认 / 疑似重复
- **敏感等级**：普通 / 隐私 / 财务 / 法务 / 高敏感

### 摘要与命名

将 `IMG_4288.PNG` 转换为 `2026-03-18_日本签证补充材料截图.png`，提升系统外部可理解性。

## 自动归档

采用[渐进自动化](../product/roadmap.md)策略，分三个阶段：

1. **只索引不移动** — 文件保留原地，系统只做理解与建议
2. **建议归档由用户确认** — 系统给出建议目录与命名，用户一键通过
3. **高置信度自动归档** — 仅对重复率高、模式稳定、用户已授权的类别自动执行

归档决策来源：用户自定义规则 + 历史行为学习 + AI 语义推断。

**可逆性**是核心保障：原路径记录、变更历史、一键回滚、最近操作时间线、恢复到摄取前状态。

## 自然语言检索

### 检索方式

自然语言描述、条件筛选、关键词组合、时间/类型混合查询、视觉/场景特征描述。

### 三层检索能力

1. **元数据检索** — 时间、类型、大小、来源、标签、项目、目录
2. **全文检索** — PDF 文本、OCR 结果、文档正文、表格文字
3. **语义检索** — 基于 embedding 与语义重排，按"意思"召回

### 查询理解

将用户自然语言转化为机器可执行的检索计划。例如"找去年报税用过的银行流水"→ 时间范围（去年报税季）+ 文件类型（银行流水、PDF、扫描件）+ 标签（财务/报税）+ 检索方式（全文 + 语义）。

### 结果可解释性

结果卡片需展示：为什么匹配、命中方式（正文/OCR/语义）、相关文件、是否重复、是否有更新版本。目标：用户 2 秒内判断"是不是它"。

---

## 相关文档

- [愿景与哲学](vision.md) — 核心能力背后的产品哲学
- [交互范式](interaction-paradigm.md) — 核心能力的交互设计
- [混合智能](../architecture/hybrid-intelligence.md) — 语义理解的技术实现策略
- [数据模型](../architecture/data-model.md) — 支撑核心能力的数据结构
```

- [ ] **Step 2: Commit**

```bash
git add docs/product/core-capabilities.md
git commit -m "docs: add core capabilities (intake, ingestion, archiving, retrieval)"
```

---

## Task 4: Create product/interaction-paradigm.md

**Files:**
- Create: `docs/product/interaction-paradigm.md`
- Source: `WHITEPAPER.md` §四（交互范式）+ §七（典型使用场景）

- [ ] **Step 1: Create interaction-paradigm.md**

Create `docs/product/interaction-paradigm.md` with this content:

```markdown
# 交互范式

> hollow 的三个交互迁移：从"放置"到"投递"，从"浏览目录"到"描述目标"，从"文件对象"到"任务集合"。

---

## 从"放置"转向"投递"

传统文件管理强调"把文件放到哪里"。hollow 将用户行为改造为"把文件投递进系统"。投递减少了用户对最终目录结构的即时决策负担 — 用户不需要在获得文件的那一刻思考长期存放位置。

## 从"浏览目录"转向"描述目标"

传统方式：回忆目录 → 点开 → 返回 → 搜索关键词 → 试错。

hollow 方式：用人类语言描述目标 → 系统理解意图并召回候选 → 用户快速确认。

这是从"路径浏览"到"意图检索"的交互迁移。

## 从"文件对象"转向"任务集合"

用户常常不是在找单个文件，而是某件事相关的全部材料。例如"日本签证"语义集合包含：护照扫描件、行程单、机票、酒店预订单、财产证明、申请表、付款截图。

hollow 逐步支持任务集合视图，让用户看到的是"语义簇"而非散乱文件列表。

## 典型使用场景

| 场景 | 描述 |
|------|------|
| 下载目录失控 | 每天下载数十文件，hollow 自动识别类型与主题，形成可搜索归档流 |
| 项目型知识工作 | 产品经理、研究员、律师等围绕项目积累异构材料，按语义关系与时间线聚合 |
| 高频截图用户 | 通过 OCR 与视觉理解，让截图成为可检索资产 |
| 个人财务与法务 | 合同、发票、报税材料，通过稳定命名和敏感标记降低查找成本 |
| 事件型整理 | 旅行、签证、搬家等跨数周的材料收集，聚合为清晰的行动包 |

---

## 相关文档

- [愿景与哲学](vision.md) — 交互范式背后的设计哲学
- [核心能力](core-capabilities.md) — 支撑交互范式的产品能力
- [路线图](roadmap.md) — 交互范式的分阶段实现
```

- [ ] **Step 2: Commit**

```bash
git add docs/product/interaction-paradigm.md
git commit -m "docs: add interaction paradigm and usage scenarios"
```

---

## Task 5: Create product/roadmap.md

**Files:**
- Create: `docs/product/roadmap.md`
- Source: `WHITEPAPER.md` §九（阶段性路线图）

- [ ] **Step 1: Create roadmap.md**

Create `docs/product/roadmap.md` with this content:

```markdown
# 路线图

> hollow 的四阶段演进路径：从可用的语义入口，到可信的自动归档，到任务管理，到个人知识操作系统。

---

## 阶段一：可用的语义入口

**目标**：先做出一个真正有价值的入口层产品。

核心能力：
- 监听统一入口文件夹
- 支持拖拽摄取
- 解析常见文档与图片
- 自动摘要与标签
- 建立全文与语义索引
- 提供自然语言搜索
- 推荐归档位置，但不强制自动移动

**成功标志**：用户即使不改变现有文件系统习惯，也愿意先把文件交给 hollow。

## 阶段二：可信的自动归档

**目标**：在积累足够用户纠正与使用历史后，引入自动归档。

新增能力：
- 命名建议与目录建议
- 高置信度自动移动
- 用户规则编辑器
- 操作回滚中心
- 重复与版本识别

**成功标志**：用户开始把 hollow 视为自己文件世界的主要入口，而非仅是搜索工具。

## 阶段三：从文件管理到任务管理

**目标**：从"找文件"提升为"组织一件事所需的所有材料"。

新增能力：
- 事件聚合与项目空间
- 文件间关系图谱
- 时间线视图
- 主动建议与提醒
- 跨来源材料整合

**成功标志**：用户不再只问"文件在哪"，而是开始问"和这件事相关的东西都有哪些"。

## 阶段四：个人知识与资产操作系统

**目标**：成为个人数字资产的统一语义层。

扩展范围：云盘文档、邮件附件、笔记系统、扫描件、会议录音、网页剪藏、设计素材、合同票据、项目资料库。

**成功标志**：hollow 成为个人信息世界的语义操作系统。

---

## 相关文档

- [愿景与哲学](vision.md) — 路线图服务的产品愿景
- [核心能力](core-capabilities.md) — 阶段一需要实现的具体能力
- [风险与边界](risks-and-boundaries.md) — 各阶段面临的风险与约束
```

- [ ] **Step 2: Commit**

```bash
git add docs/product/roadmap.md
git commit -m "docs: add four-phase product roadmap"
```

---

## Task 6: Create product/risks-and-boundaries.md

**Files:**
- Create: `docs/product/risks-and-boundaries.md`
- Source: `WHITEPAPER.md` §八（风险、边界与挑战）

- [ ] **Step 1: Create risks-and-boundaries.md**

Create `docs/product/risks-and-boundaries.md` with this content:

```markdown
# 风险与边界

> hollow 面临的产品风险、设计约束与应对策略。

---

## 错误归类风险

自动归档一旦出错，用户会迅速丧失信任（以为文件丢失、无法复原、不敢再交给系统重要文件）。

**应对**：高置信度阈值、先建议后自动、可解释与可撤销、持续学习。

## 隐私与安全风险

文件系统天然包含高度敏感数据。hollow 必须允许用户定义：

- 哪些目录被扫描 / 被忽略
- 哪些文件类型不上传
- 是否仅使用本地模型
- 是否对高敏感文件禁用云端增强

## 用户目录逻辑高度个性化

不同用户有不同的组织偏好（按项目、年份、客户、文件类型、用途）。hollow 不能预设唯一正确的目录哲学，需要允许多种策略共存，并随使用逐渐适配。

## 模糊查询与错误召回

自然语言检索带来便利也带来不确定性。必须在模糊性与可用性之间平衡：既要理解"那个东西"，也要避免过度自信。

## 多模态成本与性能

OCR、视觉理解、嵌入计算、重排推理都消耗大量资源。需要分层处理、异步索引、缓存策略和优先级队列。详见[混合智能](../architecture/hybrid-intelligence.md)。

## 产品教育问题

用户长期习惯"桌面 + 下载目录 + 文件夹"模式。hollow 若想改变习惯，必须做到：入口极简、收益立竿见影、结果可见可控。

---

## 相关文档

- [愿景与哲学](vision.md) — 风险应对背后的设计哲学
- [路线图](roadmap.md) — 渐进自动化策略如何降低错误归类风险
- [混合智能](../architecture/hybrid-intelligence.md) — 多模态成本的技术应对
```

- [ ] **Step 2: Commit**

```bash
git add docs/product/risks-and-boundaries.md
git commit -m "docs: add risks, boundaries, and mitigation strategies"
```

---

## Task 7: Create architecture/overview.md

**Files:**
- Create: `docs/architecture/overview.md`
- Source: `WHITEPAPER.md` §5.1-5.2 + `CLAUDE.md` Architecture section

- [ ] **Step 1: Create overview.md**

Create `docs/architecture/overview.md` with this content:

```markdown
# 架构总览

> hollow 采用本地优先的五层架构：入口层、摄取层、理解层、存储与索引层、交互层。大部分处理在本地完成，仅 AI 任务交给云端。

---

## 五层架构

### 第一层：入口层（Intake）

接收文件与用户输入：入口文件夹、拖拽 UI、下载接管、外部应用接入。

**技术实现**：Swift/SwiftUI — 拖拽 UI、FSEvents 文件夹监听、系统集成。运行位置：本地。

### 第二层：摄取层（Ingestion）

解析与结构化：文件监听、类型识别、内容提取、OCR/转录、哈希去重、元数据标准化。

**技术实现**：hollow-core（Rust）— 文件解析、正文提取、哈希去重；Swift — macOS Vision 框架基础 OCR。运行位置：本地。

### 第三层：理解层（Understanding）

语义建模：文档摘要、分类与标签、命名建议、敏感性判断、关系推断、归档策略建议。

**技术实现**：hollow-server — 调用 LLM API 做摘要/分类/标签/命名/敏感判断/关系推断；hollow-core — 本地规则引擎。运行位置：AI 部分云端，规则部分本地。

### 第四层：存储与索引层（Storage & Index）

持久化与检索基础设施：文件路径映射、元数据库、全文索引、向量索引、操作日志、用户规则库。

**技术实现**：hollow-core — SQLite 元数据库、全文索引、向量索引（ANN search）、操作日志。运行位置：本地。

### 第五层：交互层（Interaction）

对外提供能力：搜索框、自然语言问答、文件卡片、时间线、关系视图、规则设置、撤销与审计界面。

**技术实现**：Swift/SwiftUI — 搜索界面、文件卡片、时间线、设置；hollow-server — 查询理解、结果重排。运行位置：UI 本地，AI 辅助搜索云端。

## 技术选型

| 组件 | 技术 | 说明 |
|------|------|------|
| 平台 | macOS 26.2+ | 首发平台 |
| 客户端 | Swift 6, SwiftUI (Xcode 26) | 原生 macOS 应用 |
| 本地核心 | Rust (`hollow-core/`) | 通过 FFI（C-compatible 静态库）链接进 Swift 应用 |
| 云端服务 | Rust (`hollow-server/`) | 轻量 API 代理，不存储用户数据 |
| 测试 | Swift Testing + XCTest; `cargo test` | 单元测试用 Swift Testing，UI 测试用 XCTest |
| Bundle ID | `com.syncpulse.hollow` | |

## 本地优先原则

- **隐私安全**：文件内容和语义数据默认存储在本地，仅在需要 AI 处理时才向云端发送必要内容
- **离线可用**：断网时用户仍可浏览、搜索已索引的全部文件
- **低云端负载**：服务端仅做 API 转发，不存储用户文件和索引数据
- **低延迟**：搜索在本地完成，无需网络往返

## 代码结构

- `hollow/` — Swift 应用源码（入口：`hollowApp.swift`，根视图：`ContentView.swift`）
- `hollowTests/` — 单元测试（Swift Testing 框架）
- `hollowUITests/` — UI 测试（XCTest）
- `hollow-core/` — Rust 核心库，通过 FFI 链接进 Swift 应用
- `hollow-server/` — Rust 云端服务，独立于 hollow-core

---

## 相关文档

- [愿景与哲学](../product/vision.md) — 架构服务的产品愿景
- [数据模型](data-model.md) — 文件对象的语义数据模型
- [混合智能](hybrid-intelligence.md) — 规则与 AI 的分工策略
- [FFI 桥接](ffi-bridge.md) — Rust ↔ Swift FFI 详细设计
- [hollow-core](../modules/hollow-core.md) — Rust 核心库模块设计
- [hollow-server](../modules/hollow-server.md) — 云端 API 代理模块设计
```

- [ ] **Step 2: Commit**

```bash
git add docs/architecture/overview.md
git commit -m "docs: add architecture overview (five layers, tech stack, local-first)"
```

---

## Task 8: Create architecture/data-model.md

**Files:**
- Create: `docs/architecture/data-model.md`
- Source: `WHITEPAPER.md` §5.3

- [ ] **Step 1: Create data-model.md**

Create `docs/architecture/data-model.md` with this content:

```markdown
# 数据模型

> hollow 的文件对象不只是"路径 + 文件名 + 日期"，而是包含八个维度的语义模型。

---

## 文件对象模型

一个文件对象至少包含以下维度：

| 维度 | 字段 |
|------|------|
| 基本标识 | ID、哈希、当前路径、原路径 |
| 文件属性 | 类型、扩展名、大小、创建时间、修改时间 |
| 文本内容 | 正文、OCR 结果、转录文本、结构化字段 |
| 语义信息 | 标签、摘要、分类、实体（人名/公司/地点） |
| 关系信息 | 所属主题、关联文件、版本关系、重复关系 |
| 行为信息 | 摄取时间、移动记录、用户纠正记录 |
| 检索信息 | embedding 向量、关键词倒排索引 |
| 安全信息 | 敏感等级、上传策略、访问控制 |

长期来看，这个数据模型比底层目录结构更重要，因为它决定了 hollow 能否成为一个真正"理解文件"的系统。

## 索引结构

三种索引并存，对应[三层检索能力](../product/core-capabilities.md)：

- **元数据索引** — SQLite 结构化查询（时间、类型、标签、来源等）
- **全文索引** — 倒排索引，覆盖正文、OCR 结果、转录文本
- **向量索引** — ANN（近似最近邻）搜索，基于 embedding 的语义检索

## 操作日志

所有变更操作（移动、重命名、归类、标签修改）都记录在操作日志中，支持：

- 变更历史查询
- 一键回滚
- 恢复到摄取前状态
- 用户纠正记录（用于学习用户偏好）

---

## 相关文档

- [架构总览](overview.md) — 数据模型在存储与索引层的位置
- [核心能力](../product/core-capabilities.md) — 数据模型支撑的产品能力
- [hollow-core](../modules/hollow-core.md) — 数据模型的实现模块
```

- [ ] **Step 2: Commit**

```bash
git add docs/architecture/data-model.md
git commit -m "docs: add data model (file object, indexes, operation log)"
```

---

## Task 9: Create architecture/hybrid-intelligence.md

**Files:**
- Create: `docs/architecture/hybrid-intelligence.md`
- Source: `WHITEPAPER.md` §5.4

- [ ] **Step 1: Create hybrid-intelligence.md**

Create `docs/architecture/hybrid-intelligence.md` with this content:

```markdown
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
```

- [ ] **Step 2: Commit**

```bash
git add docs/architecture/hybrid-intelligence.md
git commit -m "docs: add hybrid intelligence strategy (rules + models + vectors + LLM)"
```

---

## Task 10: Create skeleton docs (ffi-bridge, hollow-core, hollow-server)

**Files:**
- Create: `docs/architecture/ffi-bridge.md`
- Create: `docs/modules/hollow-core.md`
- Create: `docs/modules/hollow-server.md`

- [ ] **Step 1: Create ffi-bridge.md**

Create `docs/architecture/ffi-bridge.md` with this content:

```markdown
# FFI 桥接

> Rust (hollow-core) 与 Swift 应用之间的 FFI（Foreign Function Interface）设计。通过 C-compatible 静态库链接。

<!-- TODO: 待开发时填充。需要定义：导出函数签名、内存管理约定、错误传递机制、异步调用模式。 -->

---

## 相关文档

- [架构总览](overview.md) — FFI 在整体架构中的位置
- [hollow-core](../modules/hollow-core.md) — FFI 的 Rust 侧实现
```

- [ ] **Step 2: Create hollow-core.md**

Create `docs/modules/hollow-core.md` with this content:

```markdown
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
```

- [ ] **Step 3: Create hollow-server.md**

Create `docs/modules/hollow-server.md` with this content:

```markdown
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
```

- [ ] **Step 4: Commit**

```bash
git add docs/architecture/ffi-bridge.md docs/modules/hollow-core.md docs/modules/hollow-server.md
git commit -m "docs: add skeleton docs for FFI bridge, hollow-core, and hollow-server"
```

---

## Task 11: Slim down CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update CLAUDE.md**

Make these edits to `CLAUDE.md`:

1. **Add a Documentation section** right after the "Project Overview" section (before "Build & Run"):

~~~markdown
## Documentation

For detailed project documentation (product vision, architecture, data model, module design), see [docs/INDEX.md](docs/INDEX.md). Start there and follow links to the specific document you need.
~~~

2. **Delete the entire "## Architecture" section** (from `## Architecture` through the end of the "Cloud responsibilities" bullet list). This content has been moved to `docs/architecture/overview.md` and `docs/modules/*.md`.

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: slim down CLAUDE.md, move architecture details to docs system"
```

---

## Task 12: Verify all links and final commit

- [ ] **Step 1: Verify all markdown links resolve**

Run this script to check that every markdown link target exists:

```bash
cd docs && grep -roh '\[[^]]*\]([^)]*\.md)' . | grep -oh '([^)]*\.md)' | tr -d '()' | while read link; do
  # Resolve relative paths from the file that contains the link
  if [ ! -f "$link" ]; then
    echo "BROKEN: $link"
  fi
done
```

If any links are broken, fix them before proceeding.

- [ ] **Step 2: Verify INDEX.md lists all docs**

```bash
# Count docs that exist
find docs -name '*.md' ! -name 'INDEX.md' ! -path '*/superpowers/*' | sort

# Count entries in INDEX.md
grep -c '\.md)' docs/INDEX.md
```

The two counts should match (11 docs, 11 entries).

- [ ] **Step 3: Verify CLAUDE.md links to docs/INDEX.md**

```bash
grep 'docs/INDEX.md' CLAUDE.md
```

Expected: should find the documentation link.
