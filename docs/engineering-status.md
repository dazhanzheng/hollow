# 工程实施进度

> 本文件追踪 hollow 的**具体工程实施状态**，与 [产品路线图](product/roadmap.md) 互补。
> 路线图定义"做什么、为什么"；本文件记录"做到哪了、下一步是什么"。
>
> 每次完成一个里程碑或开始新阶段时更新本文件。

---

## 当前所处位置

**产品阶段**：阶段一 — 可用的语义入口

**工程阶段**：Batch 2 完成 + Apple Vision OCR Pipeline 完成，**Batch 3（语义理解 + 全文检索）未开始**

---

## 已完成的里程碑

### Phase 1 · Batch 1 — 基础设施 + 元数据摄取（2026-04-09）

- [x] SQLite schema v1（files / file_metadata / file_content / operations_log）
- [x] Rust `FileStore` / `FileContentStore` / `FileMetadataStore` CRUD
- [x] UniFFI 0.31 FFI 桥接（proc-macros，`HollowCore` 对象）
- [x] Swift `HollowBridge` 单例包装
- [x] `FileWatcher`（FSEvents + kFSEventStreamCreateFlagFileEvents）
- [x] `IngestionService`（双并行 OperationQueue：metadata + content）
- [x] Quick hash（SHA-256 采样 5×4KB）+ 路径去重 + inode 去重
- [x] 基础 UI：ContentView（状态面板）、DatabaseBrowserView、SettingsView
- [x] 日志系统：HollowLogger（os.Logger 按 category）+ RustLogRelay（Rust tracing → Swift）+ LogViewerView
- [x] Debug 菜单：数据库浏览器、日志查看器、删除数据库并退出

**相关文档**：
- Spec: `docs/superpowers/specs/2026-04-09-phase1-batch1-infrastructure-design.md`
- Plan: `docs/superpowers/plans/2026-04-09-phase1-batch1-infrastructure.md`

### Phase 1 · Batch 2 — 内容解析管线（2026-04-10 ~ 2026-04-12）

#### 2a. 插件式 Extractor 架构

- [x] `FormatDetector`（infer magic bytes + heuristic text fallback + zip variant inspection）
- [x] `Extractor` trait + `ExtractionError` enum
- [x] `ExtractorRegistry`（by_mime + by_extension + by_basename 三级路由）
- [x] `ContentPipeline`（panic::catch_unwind 包裹 extract，extension_mismatch 检测）
- [x] 全局 disabled set（`set_extractor_enabled` / `is_extractor_disabled`）
- [x] `plugin_descriptors()` 供 Settings UI 展示
- [x] `extract_content` FFI：pending → extracting → indexed/unsupported/extract_failed 状态机
- [x] zstd 压缩 body_text_compressed、chardetng 编码检测

#### 2b. Rust Extractors（9 个）

| Extractor | 格式 | 技术栈 |
|---|---|---|
| PlainText | txt/md/csv/json/yaml/toml/xml + 70 种配置/字幕/日历/联系人/数据契约/i18n 扩展名 + 32 种无扩展名 basename | chardetng 编码检测 |
| SourceCode | 90+ 种编程语言 + 13 种无扩展名 build 脚本 | chardetng |
| Html | html/htm/xhtml | html2text 0.16 去标签 |
| Docx | docx | zip 8 + quick-xml 0.39 流式 `<w:t>` |
| Rtf | rtf | rtf-parser 0.4 |
| Epub | epub | zip + html2text 逐章节 |
| Svg | svg/svgz | quick-xml 抽 `<text>/<title>/<desc>` |
| Jupyter | ipynb | serde_json 解析 source cells |
| Fb2 | fb2 | quick-xml 跳 `<binary>` 段 |

#### 2c. Rust image_docs 模块（4 个 — 用于 Swift OCR 的 text+image 抽取）

| 格式 | 图像引用机制 |
|---|---|
| DOCX | `word/_rels/document.xml.rels` → rId → `word/media/*` |
| PPTX | 每 slide 独立 rels → `../media/*` 相对路径解析 |
| ODF | `content.xml` → `<draw:image xlink:href="Pictures/*">` |
| EPUB | 章节 XHTML → `<img src="...">` 字节级扫描注入 marker → html2text |

#### 2d. ZIP Variant Detection

- [x] `FormatDetector::detect_zip_variant`：当 infer 返回 `application/zip` 时，打开归档检查 mimetype / `word/document.xml` / `xl/workbook.xml` / `ppt/presentation.xml` 等标志文件，纠正为具体 MIME
- [x] 覆盖：EPUB、DOCX、XLSX、PPTX、ODT、ODS、ODP

**相关文档**：
- Spec: `docs/superpowers/specs/2026-04-10-content-extraction-pipeline-design.md`
- Plan: `docs/superpowers/plans/2026-04-10-content-extraction-pipeline-plan.md`

### Apple Vision OCR Pipeline（2026-04-12）

#### Swift extractors（7 个）

| Extractor | 格式 | 技术路径 |
|---|---|---|
| AppleVisionImage | png/jpg/heic/tiff/gif/bmp/webp | CGImageSource → VNRecognizeTextRequest |
| AppleVisionPdf | pdf | PDFPage.string 文本层优先；<50 字/页 → 200 DPI 灰度栅格化 + Vision OCR |
| AppleVisionDocx | docx | Rust `extract_with_images` → 每图 Vision OCR → `[Image: <text>]` 原位替换 |
| AppleVisionPptx | pptx | 同上 |
| AppleVisionOdf | odt/ods/odp | 同上 |
| AppleVisionEpub | epub | 同上 |
| AppleVisionIWork | pages/numbers/key | MDItemCopyAttribute(kMDItemTextContent) + Data/*.png OCR 追加末尾 |

#### FFI 扩展

- [x] `extract_content_external`：Swift 产出的 OCR 结果通过此 FFI 写回数据库，复用 Rust 侧的完整状态机（missing guard / extracting mark / zstd 压缩 / atomic status update）
- [x] `extract_with_images`：Rust 抽取文本层 + 图像字节，返回 text_template + images 数组，Swift 做 OCR 后替换 `{{HOLLOW_IMG_N}}` 占位符

#### 架构原则

- Swift 插件通过 `SwiftExtractor` 协议 + `SwiftExtractorRegistry` 注册
- Routing 优先级：Swift 命中 → Rust 不跑；Swift 禁用 → 落到 Rust text-only 路径
- Rust 旧 DocxExtractor / EpubExtractor 保留做 no-OCR fallback
- 共享 `plugin.enabled.<name>` UserDefaults key，Settings Plugins tab 统一显示
- Swift LOCAL 插件标 `LOCAL` 胶囊标签区分

### macOS 客户端增强（2026-04-12）

- [x] **菜单栏常驻图标**：`MenuBarExtra(style: .window)` 弹出面板，含 watching 状态、统计数、最近文件、打开主窗 / Inbox / Settings / 暂停/恢复 / 退出
- [x] **菜单悬浮高亮**：`MenuBarButton` + `@State isHovered` + accentColor 背景
- [x] **窗口管理修复**：`surfaceWindow(open:matches:)` 统一处理 Stage Manager + Dock minimize；主窗改 `Window` (单实例) 不再重复创建
- [x] **设置页分页**：TabView（General / Plugins / Advanced / Developer）
- [x] **开机自启动**：`SMAppService.mainApp` + 首次启动 prompt + Settings toggle + `.requiresApproval` 处理
- [x] **启动幂等性**：`@State didStartup` guard + `IngestionService.start()` 自身 `guard !isWatching`

---

## 当前状态

| 指标 | 数值 |
|---|---|
| Rust 测试 | 136 green |
| Rust extractors | 9 个（text-only） |
| Rust image_docs | 4 个（text + image bytes 联合抽取） |
| Swift extractors | 7 个（Vision OCR） |
| Settings 插件总数 | 16 个（9 Rust + 7 Swift LOCAL） |
| 覆盖文件扩展名 | 200+ 种 |
| Cargo 依赖 | rusqlite, uuid, serde/serde_json, thiserror, uniffi, sha2, time, mime_guess, tracing, infer, chardetng, zstd, encoding_rs, html2text, zip, quick-xml, rtf-parser |
| Xcode 构建 | Debug clean，无 warning |

---

## 未开始：Batch 3 — 语义理解 + 全文检索

这是阶段一的**最后一个 batch**，也是 hollow 的核心差异化——没有它，hollow 只是一个"能读各种文件的归档器"。

### 3a. 全文检索（FTS5）— 不依赖 LLM，可先行落地

- [ ] SQLite FTS5 虚拟表 + tokenizer 选型（ICU for CJK / Porter for English）
- [ ] body_text 解压 → FTS5 写入管线（在 `extract_content` / `extract_content_external` 成功后触发）
- [ ] 搜索 FFI：`search(query: String, limit: u32) -> Vec<SearchResult>`
- [ ] Swift 搜索 UI：搜索栏 + 结果列表 + 片段高亮
- [ ] 增量更新：re-extract 后同步更新 FTS5 索引

### 3b. LLM 语义管线 — 依赖 hollow-server 或直连 API

- [ ] hollow-server API proxy：转发 LLM/Embedding 请求到 Claude / OpenAI / 本地模型
- [ ] 自动摘要（`file_metadata.summary`）
- [ ] 自动标签（`file_metadata.tags`）
- [ ] 语义分类（`file_metadata.category`）
- [ ] Embedding 向量生成 + 向量存储（SQLite vec extension 或外部 Qdrant）
- [ ] 语义搜索：embedding cosine similarity + rerank
- [ ] Settings：API key 配置（用户自带，hollow 不当中间商）

### 3c. 混合检索

- [ ] FTS5 全文检索 + embedding 向量检索 + 元数据过滤 → 统一排序
- [ ] 自然语言 query 解析（LLM-assisted intent detection）

---

## 推迟 / 暂缓的工作

| 事项 | 原因 | 触发条件 |
|---|---|---|
| Cloud OCR fallback（Azure Document Intelligence） | Apple Vision 本地 OCR 已覆盖 90% 场景 | 用户反馈 Vision 质量不足时再做 |
| 额外监听文件夹（Downloads / Desktop） | 非侵入，但容易产生噪音 | Batch 3 搜索上线后用户需要更多来源时 |
| 独立 OCR 队列 | 先观察并发 OCR 是否真的是瓶颈 | 用户报告大 PPTX 卡顿时 |
| .doc / .xls / .ppt（老二进制 Office） | 需要 cfb crate + 协议解析，命中率低 | 用户有大量老格式文件时 |
| iWork 内联图像位置 | 需要逆向 IWA 格式（snappy+protobuf），每次 iWork 大版本会坏 | 强烈不推荐 |
| 中文本地化补齐 | 新增 UI 字符串只有英文 | 发布前 |
| XLSX / PPTX / ODF text-only Rust extractor | 被 Swift OCR 路径覆盖了，Rust no-OCR fallback 暂缺 | 有用户明确不想要 OCR 且要索引这些格式时 |

---

## 关键技术决策记录

| 决策 | 选择 | 理由 | 日期 |
|---|---|---|---|
| 压缩 vs 明文存储 body_text | zstd 压缩（body_text_compressed 列）| 节省空间 2-5x，FTS5 解压后再喂 | 2026-04-10 |
| 并发度 | `max(2, cores/2)` | 不让 IO/CPU 抢光用户前台，留一半核 | 2026-04-10 |
| 修改检测 | quick_hash（SHA-256 采样 5×4KB）| 毫秒级，足够区分 99.9% 的变更 | 2026-04-10 |
| Schema 迁移策略 | pre-v1.0 直接写干净 schema + 删库重建 | 用户量 = 0，不需要迁移脚本的维护负担 | 2026-04-11 |
| OCR 默认 | Apple Vision 本地，不接云 API | 隐私优先，零成本，零后端 | 2026-04-12 |
| PDF 双遍策略 | 文本层优先（≥50 字/页直接用）+ 扫描件回退 OCR | 90% PDF 是电子原生的，避免无谓 OCR | 2026-04-12 |
| iWork 抽取路径 | MDItemCopyAttribute (Spotlight importer) | Apple 自己用的路径，比逆向 IWA 稳定 10 倍 | 2026-04-12 |
| 文档内图像 OCR 位置 | 原位替换 `{{HOLLOW_IMG_N}}` → `[Image: <text>]` | 用户要求无缝按位置拼接 | 2026-04-12 |
| 用户自带 API key vs 统一后端 | 用户自带 key | 不当数据中介，规避 HIPAA/CCPA/PIPEDA 合规包袱 | 2026-04-12 |

---

## 相关文档

- [产品路线图](product/roadmap.md) — 四阶段产品愿景
- [架构总览](architecture/overview.md) — 五层架构
- [CEP 设计 Spec](superpowers/specs/2026-04-10-content-extraction-pipeline-design.md) — 内容解析管线设计
- [CEP 实施计划](superpowers/plans/2026-04-10-content-extraction-pipeline-plan.md) — 内容解析管线实施步骤
