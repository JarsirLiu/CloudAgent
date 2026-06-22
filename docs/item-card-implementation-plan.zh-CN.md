# Item 卡片分型实施方案

## 1. 文档目的

这份文档描述 CloudAgent CLI 的 `HistoryCell` / item 卡片如何向更接近 Codex 的“分型卡片”演进。

这里的“分型”指的是：

- 不同语义的 item 使用不同的视觉结构
- 卡片标题、正文、详情、状态的职责明确分离
- 用户消息保持现有展示风格，不额外引入标题
- 不做完整 diff viewer
- 先做“可扫读的结构化摘要”，再考虑更深入的展开能力

这份文档是实施级方案，目标是直接指导改代码，而不是继续讨论抽象架构。

## 2. 当前状态

### 2.1 已有能力

当前仓库已经具备：

- `HistoryCell` 作为 CLI 历史区的统一渲染抽象
- 用户消息、推理、命令、工具、编辑、通知等基础卡片
- active 区和 history 区的分离
- `tool_ui.rs` 中对 `CommandExecution`、`ReadFile`、`SearchWorkspace`、`EditFile` 等结构化结果的分支渲染
- `runtime_item` / `patch_buffer` / `metrics` 等运行时字段

### 2.2 当前问题

目前的卡片展示还有几个明显问题：

- 不同 item 的视觉语义还不够分明
- `EditFile` 仍然更像“文件列表卡”，不像“变更卡”
- `ToolResult`、`SearchWorkspace`、`CommandExecution` 的展示有时共享太多相似路径
- 一些工具结果仍然依赖名称或字符串启发式
- 用户消息如果已经足够直接，就不应该再强行加标题

### 2.3 本文档的非目标

这份方案不做下面这些事：

- 不做完整历史区 diff viewer
- 不重构整个 app 的消息协议
- 不要求把所有历史卡片都改成全新框架
- 不要求 CLI、Web、IDE 三端同时完成一致化

## 3. 设计目标

### 3.1 核心目标

1. 让 item 卡片“按语义分型”
2. 让用户一眼看出这是：
   - 消息
   - 推理
   - 执行
   - 变更
   - 搜索
   - 计划
   - 通知
3. 让 `EditFile` 卡片拥有真正的 patch 语义
4. 让 completed 历史卡片能展示结构化摘要，而不是只有路径列表或一句泛化 summary
5. 保持用户消息现状，不额外加标题

### 3.2 视觉原则

- 标题短
- 正文简
- 详情层次清晰
- 颜色克制
- 结构重于装饰
- 卡片之间用固定语义区分，而不是靠更多背景色变化

### 3.3 用户消息原则

用户消息保持当前样式，不引入标题行。

也就是说：

- 仍然只显示内容本身
- 不要像工具卡一样补一个标题
- 保持“用户直接说话”的感觉

## 4. 目标卡片类型

### 4.1 Message

适用对象：

- `UserMessage`
- `AgentMessage`

规则：

- 用户消息保持现有样式
- 助手消息保持现有消息气质，但标题可以更简
- 不强行堆 detail

### 4.2 Reasoning

适用对象：

- `Reasoning`

规则：

- 用单独的 reasoning 卡
- 保持淡化、克制、可折叠
- 正文展示思考摘要
- 详情承载完整 reasoning 文本

### 4.3 Action

适用对象：

- `CommandExecution`
- 一般 `ToolResult`
- 某些非搜索、非变更的工具输出

规则：

- 以“执行了什么”为主
- 标题短
- 状态明确
- 失败 / 拒绝要明显

### 4.4 Patch

适用对象：

- `FileChange`
- `StructuredToolResult::EditFile`

规则：

- 这是最重要的新分型
- 不能只显示“edited N files + path list”
- 需要有“变更摘要”语义
- 详情可以包含：
  - changed paths
  - patch summary
  - metrics
  - 失败原因

### 4.5 Search

适用对象：

- `StructuredToolResult::SearchWorkspace`
- `StructuredToolResult::ToolSearch`
- `StructuredToolResult::WebSearch`

规则：

- 以“搜索到了什么”作为主语义
- 不要变成泛化工具输出
- query、sources、results、latency 等信息放详情层

### 4.6 Plan

适用对象：

- `ProposedPlan`
- `PlanUpdate`

规则：

- 使用清晰的步骤列表
- completed step 可以弱化或划掉
- 不要和普通工具结果混在一起

### 4.7 Notice

适用对象：

- 错误
- 警告
- 元信息
- 轻量提示

规则：

- 视觉上更轻
- 只在需要时出现
- 不抢内容主体

## 5. 视觉草图

### 5.1 用户消息

```text
› 你
  我想看看这块历史区要怎么做得更像 Codex
```

### 5.2 Reasoning

```text
≈ Reasoning                                        in progress
  先把卡片按语义分型，再决定每一型的标题、摘要和详情。
  ╰─ 需要统一的是“展示语义”，不是“所有东西都长一样”。
```

### 5.3 Patch

```text
• Edit file                                       completed
  edited 2 files
  ╰─ src/app/core/active_turn.rs
  ╰─ src/ui/history_cell/tool_ui.rs
  ╰─ +1 more file
  ╰─ patch summary:
     @@ -41,7 +41,8 @@
     - old line
     + new line
  ╰─ 124 lines changed, 18 insertions, 8 deletions
```

### 5.4 Search

```text
• Web search                                      completed
  searched 3 sources
  ╰─ query: Lark Application Secret revoke
  ╰─ sources: 3
  ╰─ results: 12
  ╰─ latency: 120ms
```

### 5.5 Command

```text
• Run command                                     failed
  cargo test
  ╰─ exit 101
  ╰─ stderr: unresolved import
```

### 5.6 Plan

```text
▣ Proposed plan
  1. unify runtime item protocol
  2. remove name-based fallback
  3. add patch summary card
```

### 5.7 Notice

```text
◆ Notice
  gateway started in degraded mode
```

## 6. 样式规则

### 6.1 文本层级

每张卡建议遵守固定层级：

1. 标题行
2. 一句话摘要
3. 结构化详情
4. 展开补充

### 6.2 标题风格

- 标题要短
- 标题尽量用动词或名词短语
- 避免标题里塞太多字段
- 同类型卡片标题风格统一

### 6.3 正文风格

- 正文是卡片核心摘要
- 需要可扫读
- 不要把所有字段都放进正文

### 6.4 详情风格

- 详情统一使用缩进
- 详情可以是多行
- 详情优先承载结构化信息
- 详情不要和正文混成一行

### 6.5 颜色建议

- `Message`：中性
- `Reasoning`：蓝灰或紫灰
- `Action`：青色
- `Patch`：青绿或蓝绿
- `Search`：蓝色
- `Plan`：偏黄灰
- `Notice`：黄 / 红 / 灰

### 6.6 线条和符号建议

- `›`：用户消息
- `≈`：推理
- `•`：执行 / 工具
- `◦`：变更 / patch
- `▣`：计划
- `◆`：通知

这些符号可以延续现有风格，只是重新分配语义。

## 7. 代码改造策略

### 7.1 总体策略

不建议一上来重写整个 `HistoryCell` 系统。

更稳妥的做法是：

1. 先保留现有卡片框架
2. 增加更明确的卡片分型 helper
3. 先把 `Patch` 卡片做出来
4. 再拆分 `Search`、`Plan`、`Notice`
5. 最后收敛 `ToolResult` 的兜底逻辑

### 7.2 推荐目录结构

建议把 `cli/src/ui/history_cell/` 内部进一步按语义拆开：

- `messages.rs`
- `reasoning.rs`
- `action.rs`
- `patch.rs`
- `search.rs`
- `plan.rs`
- `notices.rs`
- `display.rs`

如果暂时不想大迁移，也可以先在现有文件里按 helper 方式拆分，再逐步拆模块。

## 8. 需要改的文件

### 8.1 `cli/src/ui/history_cell/mod.rs`

职责调整：

- 继续作为总入口
- 适度拆出新的卡片 helper
- 保留当前导出结构

建议：

- 增加 `patch` / `search` / `plan` 相关 helper 的导出
- 保留 `HistoryCell` 主框架

### 8.2 `cli/src/ui/history_cell/display.rs`

职责调整：

- 从“工具卡统一渲染”变成“分型调度层”

建议修改：

- 给不同类型卡片分派不同渲染路径
- 用户消息保留现状
- `Patch`、`Search`、`Plan` 使用更独立的布局分支
- 减少 `render_tool_like(...)` 的滥用

### 8.3 `cli/src/ui/history_cell/tool_ui.rs`

职责调整：

- 变成执行型卡片的专用渲染文件

建议修改：

- `render_command_execution(...)`
- `render_tool_result(...)`
- `render_file_change(...)`
- 把 `EditFile` 从“文件列表卡”升级为“Patch 卡”

### 8.4 `cli/src/ui/history_cell/render.rs`

职责调整：

- 保持总入口
- 只做 `TranscriptItem -> HistoryCell` 映射

建议修改：

- 对不同 `TranscriptItem` 类型调用更明确的 helper
- 不要把所有结构化结果都压进同一条渲染路径

### 8.5 `cli/src/ui/theme/history.rs`

职责调整：

- 为分型卡片补充更明确的 style 语义

建议修改：

- 增加 patch/search/plan 相关 style
- 保持整体克制
- 颜色只做轻量区分

### 8.6 `cli/src/app/tests.rs`

职责调整：

- 补充分型卡片回归测试

建议新增测试：

- 用户消息不显示标题
- `EditFile` completed 走 patch 型展示
- `Search` 卡片展示 query / sources / results
- `CommandExecution` 仍保持执行型样式

### 8.7 `cli/src/ui/history_cell/render_entry_tests.rs`

职责调整：

- 专门承载卡片文本渲染 snapshot

建议新增测试：

- patch 卡 snapshot
- search 卡 snapshot
- plan 卡 snapshot
- notice 卡 snapshot

## 9. 需要关注的后端字段

### 9.1 `patch_buffer`

这是 patch 卡片最重要的数据来源之一。

建议：

- active 阶段继续保留
- completed 历史卡片可以从 runtime snapshot 或 completed summary 中拿到结构化摘要
- 不要求先做完整 diff viewer

### 9.2 `metrics`

`metrics` 是补充信息，不是主内容。

建议：

- patch / command / search 卡片可以显示轻量 metrics footer
- 不要让 metrics 抢正文
- 如果实现复杂，可以晚于 patch 卡片再做

### 9.3 `structured`

`StructuredToolResult` 应该尽可能决定卡片类型，而不是 `tool_name` 字符串。

建议优先级：

1. `structured`
2. `tool_identity`
3. `tool_name`

## 10. 实施顺序建议

### Slice 1：先把 Patch 卡做出来

目标：

- `EditFile` 不再只是 path list
- completed 历史区开始展示 patch 摘要
- 不引入完整 diff viewer

涉及文件：

- `cli/src/ui/history_cell/tool_ui.rs`
- `cli/src/ui/history_cell/display.rs`
- `cli/src/ui/history_cell/render.rs`
- `cli/src/ui/history_cell/render_entry_tests.rs`
- `cli/src/app/tests.rs`

### Slice 2：拆分 Search 和 Command

目标：

- 搜索卡和执行卡视觉上分家
- 不再把所有 `ToolResult` 统一成同一种工具卡

涉及文件：

- `cli/src/ui/history_cell/tool_ui.rs`
- `cli/src/ui/history_cell/display.rs`
- `cli/src/ui/history_cell/render_entry_tests.rs`

### Slice 3：抽出 Plan 卡

目标：

- 计划类内容独立成卡
- 步骤列表可扫读

涉及文件：

- `cli/src/ui/history_cell/mod.rs`
- `cli/src/ui/history_cell/display.rs`
- `cli/src/ui/history_cell/render_entry_tests.rs`

### Slice 4：整理 Notice 和兜底卡

目标：

- 错误、警告、信息提示风格统一
- 兜底逻辑变得轻量

涉及文件：

- `cli/src/ui/history_cell/display.rs`
- `cli/src/ui/theme/history.rs`

## 11. 验收标准

### 11.1 视觉标准

- 用户消息没有标题
- Reasoning 有独立层级
- Patch 卡能明确看出“改了什么”
- Search 卡能明确看出“搜了什么”
- Command 卡能明确看出“执行了什么”
- Plan 卡能明确看出“有哪些步骤”

### 11.2 行为标准

- completed 历史卡不会丢 patch 摘要
- active 卡和 completed 卡视觉方向一致
- 不引入完整 diff viewer 也能满足基本历史表达

### 11.3 代码标准

- 不把所有逻辑重新塞回一个巨大的 match
- 能分 helper 就分 helper
- 能按语义拆文件就拆文件
- 新增测试覆盖主要卡片类型

## 12. 最后建议

如果你要先落一刀，优先顺序我建议是：

1. 先做 `Patch` 卡
2. 再拆 `Search` 和 `Command`
3. 然后做 `Plan`
4. 最后收敛 `Notice`

用户消息保持现状，不要动。

这会让你的 CLI 历史区更接近 Codex，但不会一开始就把架构改得过重。
