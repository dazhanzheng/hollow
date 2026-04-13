# Spotlight 风格全局搜索浮层 — 设计 Spec

> **日期**：2026-04-13
> **阶段**：Phase 1 / post-Batch-3 UX 增强
> **关联路线图节点**：阶段一"可用的语义入口" → 自然语言搜索入口的体感升级

---

## 背景

Batch 3 已完成 FTS5 + 本地 Embedding + Reciprocal Rank Fusion 混合检索，检索能力本身是完整的，但唯一的用户入口是一个独立的 `Window("search")` 大搜索窗，需要用户先打开 menubar 面板 → 点 "Open Main Window" 或通过菜单触发。这条路径对"随手一搜"的场景太重。

我们要做一个**类 Spotlight 的全局浮层搜索入口**：按 ⌥Space 任意位置弹出，边输入边出结果，选中回车打开，关闭自动收起。不改现有的主搜索窗和 menubar 面板，两条路径并存。

## 目标

- 用户在任意 App 里按 ⌥Space 即可搜 hollow 索引过的所有文件
- 搜索体感对标 Spotlight / Alfred：极简浮层、边输入边出、键盘优先
- 搜索质量用的是完整 `hybrid_search`（FTS5 + embedding RRF 融合），不降级
- 快捷键可在 Settings 里自定义或禁用
- 零 Rust 侧改动（复用现有 FFI）

## 非目标

- 不做右侧预览面板（Spotlight 完整版）— 后续迭代
- 不做多 action / action panel（Raycast 风格）— 后续迭代
- 不做 Quick Look 集成 — 后续迭代
- 不改现有 `SearchView`（主搜索窗）的行为 — 两条路径故意并存
- 不做多语言命令解析、NL intent 分类等 — 等 LLM 管线上线后再谈

## 架构

新增一个**独立于 `MenuBarExtra` 的搜索浮层子系统**，挂在 App 级别。

```
hollowApp (App)
├── MenuBarExtra(...)            ← 保持不变
├── Window("main", ...)          ← 保持不变
├── Window("search", ...)        ← 保持不变（主搜索窗）
└── SpotlightOverlay (new)
    ├── SpotlightCoordinator    ← @Observable 单例，管状态机 + panel 生命周期
    ├── SpotlightPanel          ← NSPanel 子类（无边框 HUD 浮层）
    │   └── NSHostingView<SpotlightView>
    │       └── SpotlightView   ← SwiftUI 内容，输入框 + 结果列表
    │           └── SpotlightResultRow
    └── KeyboardShortcuts.Name.spotlightSearch ← 快捷键持久化
```

### 关键决定：为什么 `NSPanel` 而不是 SwiftUI `Window`

SwiftUI 的 `Window` / `WindowGroup` scene 没法做成：
- 无边框 (`.borderless`)
- 非激活式弹出（不抢当前 App 焦点栈上的其他窗口，`.nonactivatingPanel`）
- HUD vibrancy 背景（`.hudWindow` style mask）
- 覆盖全屏 level (`NSWindow.Level.floating` 或更高)

这些都是 Spotlight/Alfred 类浮层的硬性要求。做法是自己子类化 `NSPanel`，内部仍然用 `NSHostingView` 嵌入 SwiftUI 视图，享受声明式 UI 的好处。

### 关键决定：依赖 `sindresorhus/KeyboardShortcuts`

通过 SPM 引入 [sindresorhus/KeyboardShortcuts](https://github.com/sindresorhus/KeyboardShortcuts)。理由：
- 纯 Swift，SwiftUI-native `KeyboardShortcuts.Recorder` view，Settings 自定义 UI 零成本
- 全局 hotkey 注册、持久化（UserDefaults）、冲突检测内置
- 自己写 Carbon `RegisterEventHotKey` 约 100+ 行且没有 Recorder UI，成本更高

用户已在 brainstorming 中认可此依赖。

## 数据流与状态机

```
⌥Space 按下
  ↓
KeyboardShortcuts callback → SpotlightCoordinator.toggle()
  ↓
  ├─ [isVisible == true]  → SpotlightPanel.orderOut() + clearState()
  └─ [isVisible == false] → positionPanel() → makeKeyAndOrderFront()
                             ↓
                             SpotlightView 挂载 → TextField 自动聚焦
                             ↓
                             [query == ""]  → 显示 ingestion.recentFiles 前 8 条
                             [query != ""]  → 250ms debounce → hybridSearch → 显示结果
                             ↓
                             ↑/↓ 调整 selectedIndex
                             ↓
                             ↵  → openFile(results[selectedIndex])  → hide()
                             ⌘↵ → revealInFinder(results[selectedIndex]) → hide()
                             ESC → hide()
                             点击外部 → hide()
                             App resignActive → hide()
```

### 状态

`SpotlightCoordinator`（`@Observable`，App 级单例）持有：

| 字段 | 类型 | 说明 |
|---|---|---|
| `isVisible` | `Bool` | 面板是否可见 |
| `query` | `String` | 当前输入框内容 |
| `results` | `[SearchResult]` | 当前结果列表（初始为空 / recentFiles 映射 / hybridSearch 返回值） |
| `selectedIndex` | `Int` | 键盘选中的行索引，范围 `[0, results.count)` |
| `searchTask` | `Task<Void, Never>?` | 当前进行中的 debounced 搜索任务，query 变化时 cancel 旧的 |

`query` / `results` / `selectedIndex` 在 `hide()` 时全部清空——下次打开是干净状态（和 Spotlight 一致）。

### Debounce 实现

```swift
func onQueryChange(_ newQuery: String) {
    searchTask?.cancel()
    query = newQuery
    if newQuery.isEmpty {
        results = recentFilesAsSearchResults()
        selectedIndex = 0
        return
    }
    searchTask = Task { @MainActor in
        try? await Task.sleep(for: .milliseconds(250))
        if Task.isCancelled { return }
        let hits = await HollowBridge.shared.hybridSearch(query: newQuery, limit: 8)
        if Task.isCancelled { return }
        results = hits
        selectedIndex = 0
    }
}
```

250ms 是典型的"输入暂停"阈值。`hybrid_search` 在当前规模（<10k 文件）下的预期延迟 <50ms，FTS5 + embedding 暴力 cosine 都在 Rust 侧，debounce 主要是避免连打时打出 N 次搜索。

### 关闭触发

`SpotlightPanel` 是 `NSPanel` 子类：

- `canBecomeKey = true`，以便 TextField 获得焦点
- 监听 `NSWindow.didResignKeyNotification` → `coordinator.hide()`（点击外部）
- SwiftUI view 层 `.onExitCommand` → `coordinator.hide()`（ESC）
- App 级 `NSApplication.didResignActiveNotification` → `coordinator.hide()`（切到别的 App）
- `coordinator.toggle()` 重复按 hotkey 时自己处理 show/hide 分支

## 视觉规格

```
┌────────────────────────────────────────────────────────────────┐
│  🔍  Search hollow...                                          │  60pt 高
├────────────────────────────────────────────────────────────────┤
│  📄  annual-report-2025.pdf                                2d  │
│      "...projected revenue of $4.2M in Q3..."                  │  52pt 高
├────────────────────────────────────────────────────────────────┤
│  🖼  design-mockup-v3.png                                  5h  │
│      "figma export homepage hero section"                       │
└────────────────────────────────────────────────────────────────┘
     680pt wide
```

### 面板整体

- 宽：680pt（固定）
- 高：`60 + min(results.count, 8) * 53`（分隔线 1pt 算进去）
- 输入条和结果区都有时，总高度上限约 60 + 8×53 ≈ 484pt
- 圆角 10pt，无标题栏
- `NSWindow.StyleMask = [.borderless, .nonactivatingPanel, .hudWindow]`
- `backgroundColor = .clear`，内部用 `NSVisualEffectView` (`.hudWindow` material) 承载毛玻璃
- `level = .floating`
- `collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]`（跨 Space + 全屏 App 下也能浮出）

### 定位

水平居中于主显示器 `visibleFrame`；垂直 `origin.y = visibleFrame.origin.y + visibleFrame.height * 0.35`。多显示器场景下始终用 `NSScreen.main`（即当前 key window 所在的那个屏）。

### 输入条（60pt 高）

- 左内边距 24pt，`Image(systemName: "magnifyingglass")` 18pt secondary
- `TextField("Search hollow...", text: $query)` `.plain` 风格，字号 22pt，无边框
- `.focused($isFieldFocused)`，面板显示时 `.onAppear { isFieldFocused = true }`

### 结果行（52pt 高，每行之间 1pt `Divider`）

- 左内边距 20pt，行内 HStack spacing 12pt
- 左侧：48×48pt 图标，来自 `NSWorkspace.shared.icon(forFile: path)` 桥到 SwiftUI `Image`
- 中部 VStack：
  - 主行：文件名，`.system(size: 15, weight: .semibold)`
  - 副行：FTS5 snippet（命中词用 `AttributedString` 标记 bold + accent color），`.system(size: 12)` secondary，`lineLimit(1)` + `truncationMode(.middle)`
- 右侧：相对时间字符串（`"2d" / "5h" / "1w"`），`.system(size: 11)` tertiary
- 选中态：整行背景 `Color.accentColor`，主行 / 副行 / 右侧时间文字均变 `.white`，图标保持原色（自然的 Finder/Spotlight 行为）
- 鼠标 hover：移动 `selectedIndex` 到 hover 的行（跟 Spotlight 一致——hover 和键盘导航共用选中态，不区分两种高亮）

### 键盘导航

| 按键 | 行为 |
|---|---|
| ↓ | `selectedIndex = min(selectedIndex + 1, results.count - 1)` |
| ↑ | `selectedIndex = max(selectedIndex - 1, 0)`（不 wrap） |
| Home | `selectedIndex = 0` |
| End | `selectedIndex = results.count - 1` |
| ↵ | `openFile(results[selectedIndex])` + hide |
| ⌘↵ | `revealInFinder(results[selectedIndex])` + hide |
| ESC | hide |
| 其他字符 | 交给 TextField |

### 空状态

- `query == "" && recentFiles.isEmpty`：只显示输入条，不显示结果区（面板高度 60pt）
- `query == "" && !recentFiles.isEmpty`：显示 recentFiles 前 8 条作为结果
- `query != "" && results.isEmpty`：显示一行 52pt 高的 "No matches" 灰字 placeholder
- `query.count < 3` 且是纯拉丁字符：FTS5 trigram 的最小词限制保持（和主搜索窗行为一致），显示 "Type at least 3 characters" 提示

## Settings 集成

在 [hollow/SettingsView.swift](hollow/SettingsView.swift) 的 **General** tab 新增一节 "Global Search"：

```
Global Search
  Hotkey:  [⌥Space      ] (Recorder)
           Press to record · click ⓧ to clear
  □ Enable global search hotkey
```

- `KeyboardShortcuts.Recorder(for: .spotlightSearch)` 直接绑定
- 默认值：`.option + .space`
- 用户清空 recorder 即等于禁用（KeyboardShortcuts 库原生支持）
- 冲突检测由库处理——被系统占用时 recorder 会拒绝录入并显示警告

## 文件清单

**新增**：
- [hollow/Spotlight/SpotlightCoordinator.swift](hollow/Spotlight/SpotlightCoordinator.swift) — `@Observable` 单例，状态机 + panel 生命周期
- [hollow/Spotlight/SpotlightPanel.swift](hollow/Spotlight/SpotlightPanel.swift) — `NSPanel` 子类 + `canBecomeKey` + `didResignKey` 监听
- [hollow/Spotlight/SpotlightView.swift](hollow/Spotlight/SpotlightView.swift) — SwiftUI 主视图，输入框 + 结果列表 + 键盘绑定
- [hollow/Spotlight/SpotlightResultRow.swift](hollow/Spotlight/SpotlightResultRow.swift) — 单行展示 + 选中态
- [hollow/Spotlight/KeyboardShortcutsNames.swift](hollow/Spotlight/KeyboardShortcutsNames.swift) — `extension KeyboardShortcuts.Name { static let spotlightSearch = ... }`

**修改**：
- [hollow/hollowApp.swift](hollow/hollowApp.swift) — `init()` 里挂载 `SpotlightCoordinator` 单例 + 注册 hotkey handler
- [hollow/SettingsView.swift](hollow/SettingsView.swift) — General tab 加 Recorder
- `hollow.xcodeproj/project.pbxproj` — 新增 SPM 依赖 `KeyboardShortcuts`

**不改**：
- Rust `hollow-core` 零改动（复用现有 `hybrid_search` FFI）
- [hollow/MenuBarView.swift](hollow/MenuBarView.swift) 零改动
- [hollow/SearchView.swift](hollow/SearchView.swift)（主搜索窗）零改动

## 错误处理

- **hotkey 注册失败**（极罕见，被更高优先级的系统服务抢走）：KeyboardShortcuts 库 fallback 为无 hotkey 状态，Settings 里显示警告
- **hybridSearch 抛错**：已经是 Rust → Swift 桥的错误，复用现有错误路径，面板里显示 "Search failed" 灰字行（不抛异常、不崩）
- **ingestion 未启动**（首次运行 / 数据库为空）：空 query 时 recentFiles 为空，面板只有输入条，输入搜索正常走 hybridSearch 返回空
- **多显示器**：定位算法用 `NSScreen.main`，失败则用 `NSScreen.screens.first`
- **面板已显示时再次按 hotkey**：toggle → hide，不会出现"套娃"

## 测试策略

### Swift 单元测试（新增）

- `SpotlightCoordinatorTests`
  - `toggle()` 在 isVisible false → true → false 的状态机正确性
  - `hide()` 后 query / results / selectedIndex 都被清空
  - `onQueryChange("")` 直接填充 recentFiles，不进 hybridSearch
  - `onQueryChange("foo")` 在 debounce 窗口内 cancel 前一个 task
  - `selectedIndex` 在 ↑↓ 边界不越界、不 wrap
- `SpotlightResultRowTests`
  - snippet 高亮的 `AttributedString` 生成正确
  - 相对时间字符串格式（"2d" / "5h" / "1w" / "just now"）

### 手动验证清单

- [ ] 第一次启动后在 Safari / Xcode / Finder 三种 App 下按 ⌥Space 都能弹出
- [ ] ESC / 点击外部 / ⌥Space 再按 / 切 App 都能正确关闭
- [ ] ↑↓ 键盘导航不越界
- [ ] ↵ 打开 PDF / ⌘↵ 在 Finder 定位
- [ ] 中英文切换输入法（ABC → 拼音）时不出现字符丢失
- [ ] Alfred / Raycast 同时运行时自定义快捷键避开冲突
- [ ] 外接显示器 + 主屏切换时面板定位正确
- [ ] Stage Manager 和全屏 App 下都能浮出
- [ ] 连打字符时 debounce 生效（查看 Rust log 应看到去重后的搜索次数）
- [ ] 空 query 下显示最近文件
- [ ] `query.count < 3` 时的 trigram 提示
- [ ] 搜索结果为空时的 "No matches" 显示
- [ ] 执行动作后面板自动关闭 + 下次打开 query 为空
- [ ] Settings Recorder 录制新快捷键后立即生效
- [ ] Settings Recorder 清空后 ⌥Space 不再触发

### 不测的

- Rust `hybrid_search` 的正确性（已在 Rust 158 个测试覆盖）
- `KeyboardShortcuts` 库本身的 hotkey 持久化（信任上游）

## 风险与边界

| 风险 | 缓解 |
|---|---|
| ⌥Space 与 Alfred/Raycast 默认冲突 | Settings 允许自定义，录制器有冲突检测 |
| NSPanel 非激活式在 macOS 15+ 的 Stage Manager 下表现不稳 | 手动测试覆盖，遇到具体坑再用 `collectionBehavior` 调整 |
| 连打输入时 hybridSearch 可能排队 | 250ms debounce + `Task.cancel` 双重兜底 |
| 多显示器定位抖动 | `NSScreen.main` 在每次 show 时重新计算，不缓存 |
| Settings 里用户把 hotkey 清空但忘了怎么恢复 | 提供 "Reset to default" 按钮（未来迭代） |

## 成功标志

- 用户反馈"搜索体感像 Spotlight"
- 用户的主搜索窗使用频率下降，hotkey 使用频率上升
- 首次输入到结果渲染的 P50 延迟 <300ms（250ms debounce + ~50ms 搜索）

## 相关文档

- [产品路线图](../../product/roadmap.md) — 阶段一"可用的语义入口"
- [Batch 3 实施计划](../plans/2026-04-12-batch3-semantic-search.md) — 复用的 hybrid_search 来源
- [工程实施进度](../../engineering-status.md) — 本 spec 完成后需新增 Batch 4 / UX 增强条目
