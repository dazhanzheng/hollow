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
