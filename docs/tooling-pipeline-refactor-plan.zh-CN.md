# CloudAgent 工具链路重构方案（对齐 Codex 标准化方向）

## 1. 背景

当前 `cloudagent` 已经具备一套较完整的本地工具执行骨架：

- `agent-core` 中定义了统一的 `ToolSpec / ToolCall / ToolResult / StructuredToolResult`
- turn 运行时具备 `ItemStarted / ItemDelta / ItemCompleted` 生命周期事件
- CLI 已经支持 active 运行态、历史区提交、底部状态栏展示

但是这套能力目前主要稳定覆盖的是“本地执行型工具”，例如：

- `exec_command`
- `write_stdin`
- 文件读写类工具
- workspace 搜索类工具

当接入 `web_search` 这类 hosted / model-side 工具时，系统暴露出明显的不一致：

- 工具开始态与完成态没有统一落在标准工具结果模型中
- 前端展示依赖字符串和分支特判，而不是统一的强类型 item 协议
- active 展示与历史提交的行为不完全一致
- 新工具很难“自然接入”，需要在多处补丁式修改

这说明当前问题不是 `web_search` 单点实现问题，而是整个工具链路还没有形成面向长期演进的标准化协议。

本方案目标是将 `cloudagent` 的工具链路重构为接近 Codex 的长期可维护架构，使未来新增工具、丰富展示能力、支持更多前端时，不再反复打补丁。

## 2. 重构目标

### 2.1 核心目标

建立一套统一、强类型、跨来源一致的工具运行协议，使：

- 本地 built-in 工具、MCP 工具、hosted 工具、未来动态工具都走同一条工具生命周期链路
- 前端只消费标准 item 协议，不依赖大量字符串判断和特殊 case
- active 运行态、delta 增量、completed 历史提交全部遵循统一规则
- transcript/history 是稳定摘要视图，而不是承担运行时协议职责

### 2.2 长期目标

为后续能力预留标准扩展点，包括但不限于：

- 代码修改类工具在前端显示实时 diff
- 工具执行过程中展示结构化进度
- 展示工具输出消耗的 token / 字符 / 文件数 / 搜索命中数等指标
- 支持 richer runtime item，如 review、plan、guardian、approval、collaboration
- 支持 CLI / Web / IDE 三类前端共用一套事件协议

## 3. 对齐 Codex 的核心设计原则

参考 Codex，目前值得对齐的核心原则如下：

### 3.1 所有前端展示对象都应是正式 item

前端不应依赖“`kind + title` 推导出一个临时展示对象”，而应直接接收完整的、强类型的 item。

Codex 的原则是：

- 每个可展示的运行单元都是 `ThreadItem`
- 每个 item 都能被 started / completed 正式表达
- 前端永远围绕 item 生命周期处理展示

### 3.2 所有 item 都走统一生命周期

统一规则应为：

`item/started -> 0..N 个 delta / patch / progress -> item/completed`

其中：

- `started` 用于立即展示 active 运行态
- `delta` 用于增量更新 active 内容
- `completed` 是历史提交与最终状态的权威来源

### 3.3 运行协议与历史摘要分层

运行协议层负责：

- 正在发生什么
- 当前 active item 是谁
- 收到了哪些增量
- item 当前处于什么状态

历史摘要层负责：

- 最终保留给用户的 transcript / history
- 用于恢复、重放、上下文过滤、压缩的稳定内容

这两层不能混用。

### 3.4 工具来源不能决定协议差异

工具来源可以不同，但展示协议不能分裂。

以下工具来源都应使用统一 item 协议：

- 本地 built-in 工具
- MCP 工具
- hosted 工具
- model-side tools
- future dynamic tools

来源差异应该是 item metadata，而不是另起一条渲染链。

## 4. 当前架构问题

### 4.1 TranscriptItem 同时承担运行时占位与最终历史结果

当前 started 阶段会依据 `TurnItemKind + title` 构造占位 `TranscriptItem`。

这带来的问题：

- started 阶段不是完整 item，而是推导产物
- 新工具如果需要结构化字段，started 阶段无法稳定表达
- 前端展示无法严格依赖协议

### 4.2 工具链路只对“本地工具”完全标准化

当前标准骨架主要覆盖：

- tool batch
- approval flow
- streaming output
- ToolResult -> transcript 投影

但 hosted 工具没有完整进入这套骨架。

结果是：

- `web_search` 看起来像工具，但本质仍走特殊旁路
- 新增 hosted 工具时会重复踩坑

### 4.3 TranscriptItem 存在破坏统一性的特殊变体

当前既有：

- `TranscriptItem::ToolResult`
- 又有 `TranscriptItem::WebSearch`

这说明工具结果模型没有真正封闭。

长期风险：

- 每新增一类 hosted tool，都可能再加一个新的 transcript 特例
- reducer、renderer、history rebuild、restore、filter 都会被不断撕裂

### 4.4 前端展示逻辑依赖字符串和临时特判

例如：

- `tool_name == "web_search"` 的 runtime 特判
- `matches!(item, TranscriptItem::WebSearch { .. })` 的 reducer 特判
- 为了修复跳帧问题加事件边界补丁

这类逻辑在短期可工作，但长期会使系统难以扩展和验证。

### 4.5 缺少统一的“运行时 item 协议”

当前 `TranscriptItem` 更像是历史表达，而不是完整 runtime item。

因此系统缺少一个真正居中的标准协议层，用来描述：

- started payload 长什么样
- active item 如何更新
- completed 结果如何收敛
- 不同工具类型如何带结构化展示数据

## 5. 目标架构

建议将工具链路重构为三层模型：

### 5.1 层一：Runtime Item 协议层

新增一套面向前端协议和运行时状态的强类型 item 模型，例如：

- `RuntimeItem`
- `RuntimeItemKind`
- `RuntimeItemStatus`
- `RuntimeItemProgress`
- `RuntimeItemPayload`

该层用于表达“当前发生中的可展示单元”。

建议至少覆盖以下类型：

- `assistantMessage`
- `reasoning`
- `commandExecution`
- `fileChange`
- `toolCall`
- `mcpToolCall`
- `webSearch`
- `imageView`
- `dynamicToolCall`
- `review`
- `contextCompaction`

### 5.2 层二：Turn Event 协议层

围绕 `RuntimeItem` 提供标准事件：

- `ItemStarted { item }`
- `ItemProgress { item_id, progress }`
- `ItemDelta { item_id, stream, chunk }`
- `ItemPatch { item_id, patch }`
- `ItemMetricsUpdated { item_id, metrics }`
- `ItemCompleted { item }`

说明：

- `started/completed` 都携带完整 item
- `delta/progress/metrics` 则只携带增量
- 所有前端统一消费此层

### 5.3 层三：Transcript / History 摘要层

`TranscriptItem` 不再承担运行态协议职责，只作为：

- 最终历史展示
- turn replay
- compact/filter context
- session restore

使用的稳定摘要模型。

该层可保留：

- `AgentMessage`
- `CommandExecution`
- `FileChange`
- `ToolResult`
- `Reasoning`

但应删除工具类特例项，如 `TranscriptItem::WebSearch`。

## 6. 目标数据模型

### 6.1 RuntimeItem 建议结构

建议新增类似如下结构：

```rust
pub enum RuntimeItem {
    AssistantMessage { ... },
    Reasoning { ... },
    CommandExecution { ... },
    FileChange { ... },
    ToolCall { ... },
    McpToolCall { ... },
    WebSearch { ... },
    DynamicToolCall { ... },
    ImageView { ... },
    ContextCompaction { ... },
}
```

每个 item 至少具备：

- `id`
- `call_id`
- `kind`
- `status`
- `source`
- `title`
- `summary`
- `metrics`

### 6.2 统一工具来源标识

建议补充：

```rust
pub enum ToolInvocationSource {
    BuiltIn,
    Mcp,
    Hosted,
    Dynamic,
}
```

这样前端可以知道来源，但展示协议保持统一。

### 6.3 统一结构化结果模型

`StructuredToolResult` 应继续保留，并扩展为完整工具结果封闭集。

建议加入：

- `WebSearch`
- `PatchPreview`
- `PatchApply`
- `ToolMetrics`
- future `DynamicToolCallResult`

对于 `web_search`，建议改为：

```rust
StructuredToolResult::WebSearch {
    query: String,
    action: WebSearchAction,
    result_count: Option<usize>,
    source_count: Option<usize>,
}
```

### 6.4 统一 metrics 模型

为未来 token、diff、进度等能力预留：

```rust
pub struct RuntimeItemMetrics {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub bytes_read: Option<u64>,
    pub bytes_written: Option<u64>,
    pub file_count: Option<usize>,
    pub source_count: Option<usize>,
    pub result_count: Option<usize>,
}
```

这可以支持后续展示：

- “edited 3 files”
- “searched 2 times”
- “found 6 sources”
- “output 1.2k tokens”

## 7. 事件协议重构建议

### 7.1 当前问题

当前 `EventMsg::ItemStarted` 只有：

- `item_id`
- `call_id`
- `kind`
- `title`

它不是完整 payload。

### 7.2 目标

将 `ItemStarted` 和 `ItemCompleted` 统一成完整 item 载荷。

建议改为：

```rust
ItemStarted {
    turn_id,
    item: RuntimeItem,
}

ItemCompleted {
    turn_id,
    item: RuntimeItem,
}
```

增量事件则只通过 `item_id` 关联：

```rust
ItemDelta {
    turn_id,
    item_id,
    delta_kind,
    delta,
}

ItemMetricsUpdated {
    turn_id,
    item_id,
    metrics,
}
```

### 7.3 收益

- started 阶段前端立即有完整可渲染对象
- 不再依赖 `kind + title` 推导占位 item
- 新工具接入只需定义 item payload 和 delta/metrics 规则
- runtime / restore / replay / web UI 可统一

## 8. Transcript 重构建议

### 8.1 删除工具类特例 TranscriptItem

应逐步删除：

- `TranscriptItem::WebSearch`

未来也不再允许新增类似：

- `TranscriptItem::FooHostedTool`
- `TranscriptItem::BarSearch`

### 8.2 统一用 ToolResult 承载工具历史结果

原则：

- 工具历史结果统一落为 `TranscriptItem::ToolResult`
- 更具体的分类通过 `structured: StructuredToolResult` 实现

也就是说：

- command-like 工具可以投影为 `CommandExecution`
- file-change 工具可以投影为 `FileChange`
- 其他工具统一为 `ToolResult`

这与当前已有思路是一致的，应继续强化，而不是开特例。

## 9. 前端展示层重构建议

### 9.1 分离“操作分类”和“展示语法”

延续目前你希望的方向：

- 操作分类：`Web / Search / Read / Run / Edit / External`
- 展示语法：`Exploration / Command / Edit / Tool / Notice`

但分类来源必须从结构化 item 得出，而不是字符串猜。

### 9.2 Active 区展示规则

active 区应遵循统一规则：

- 收到 `ItemStarted` 立即创建 active cell
- 收到 `ItemDelta / ItemProgress / ItemMetricsUpdated` 立即更新 active cell
- 收到 `ItemCompleted` 立即完成并提交到历史区

不应等待 assistant message 再顺带出现。

### 9.3 历史区展示规则

历史区的工具 item 应只由 completed item 提交。

即：

- started 不直接进入历史区
- completed 是历史区的权威来源
- 如果 active 丢失，则 fallback 直接插入 completed item

### 9.4 底部状态栏规则

底部状态栏不应依赖硬编码工具名。

建议由 `RuntimeItemKind + RuntimeItemMetrics + title` 推导，例如：

- command: `running command: cargo test`
- web search: `searching the web: OpenAI API pricing`
- edit: `editing files: 3 pending changes`
- read/search: `searching workspace: ToolRegistry`

### 9.5 Diff 展示能力

未来代码编辑类工具应支持结构化 diff，而不是只显示文本 summary。

建议在 runtime 协议层支持：

- `ItemPatch` 增量事件
- `StructuredToolResult::PatchPreview`
- `StructuredToolResult::PatchApply`

前端可据此实现：

- active 态显示 patch 正在生成
- completed 后历史区显示 diff 摘要
- 可扩展出更丰富的 patch viewer

## 10. 未来能力预留

### 10.1 实时 diff

适用于：

- `edit_file`
- `apply_patch`
- multi-file write tools
- future agent-generated patch tools

建议协议支持：

- patch chunk 流式增量
- changed file count
- add/delete/rename 操作分类

### 10.2 工具级 token 展示

未来可为 hosted tool 或 model-generated tool 增加：

- query token
- output token
- total token

命令类工具也可展示：

- 输出字符数
- 截断状态
- elapsed time

### 10.3 Rich progress

适用于：

- workspace search
- read large file
- batch edit
- web search multi-query
- multi-agent waiting / spawning

统一通过 `ItemProgress` 表达，而不是拼字符串。

### 10.4 Cross-frontend consistency

重构完成后，CLI、Web、IDE 前端都应只消费：

- runtime item 协议
- transcript/history 协议

不再分别发明自己的工具链解释逻辑。

## 11. 分阶段实施方案

### Phase 1：建立运行时 item 协议

目标：

- 在 `agent-core` 引入 `RuntimeItem`
- 调整 `EventMsg::ItemStarted / ItemCompleted` 载荷
- 为现有 command / file change / generic tool result 提供适配器

完成标准：

- started/completed 不再依赖 `kind + title` 推导占位 item
- CLI 可以直接消费完整 started item

### Phase 2：统一 hosted 工具接入

目标：

- 将 `web_search` 改造为标准 runtime item + 标准 tool result
- 删除 `TranscriptItem::WebSearch`
- hosted 工具与 built-in 工具进入同一工具生命周期协议

完成标准：

- `web_search` active 行为与其他工具一致
- history 提交不再需要 web_search 特判

### Phase 3：前端改为强类型渲染

目标：

- reducer 不再依赖字符串判断工具类型
- bottom pane 从 runtime item 推导展示
- history cell 从 structured result 推导分类

完成标准：

- 减少 `web_search` / `exec_command` / `edit_file` 这种分散的硬编码特判
- 分类展示统一由 item 类型系统驱动

### Phase 4：diff 与 metrics 协议

目标：

- 增加 patch / diff 增量事件
- 增加 item metrics 更新事件
- 前端支持 richer edit / search / web UI

完成标准：

- edit 类工具支持结构化 diff 展示
- hosted / tool 类 item 可以展示 source_count / result_count / token_count 等结构化指标

### Phase 5：历史重放与跨前端一致化

目标：

- replay / restore / snapshot 统一使用 runtime item + transcript item 的清晰分层
- 为未来 web 前端提供稳定协议

完成标准：

- CLI 与 future web UI 不再复制解释逻辑
- replay 不依赖大量“补偿式行为”

## 12. 代码级改造建议

### 12.1 `agent-core`

建议重点改造：

- `turn/events.rs`
- `conversation/items.rs`
- `projection/transcript.rs`
- `tool/mod.rs`
- `tool/batch.rs`
- hosted tool 相关 streaming / model observer

新增：

- `runtime_item.rs`
- `runtime_item_metrics.rs`

删除或收敛：

- `TranscriptItem::WebSearch`

### 12.2 `agent-app-server`

建议重点改造：

- `projection/conversation_notifications.rs`
- `projection/transcript_item_projection.rs`

目标：

- app server 对外发完整 runtime item started/completed
- transcript/history 继续作为持久化摘要视图

### 12.3 `cli`

建议重点改造：

- `state/reducer.rs`
- `state/bottom_pane_runtime.rs`
- `app/conversation/actions/server_actions.rs`
- `ui/history_cell/*`

目标：

- active 区、底栏、历史区都只消费强类型 item
- 删除 hosted tool 的路径特判

## 13. 验收标准

重构完成后，应满足以下标准：

### 13.1 新工具接入标准

新增一种工具时，只需要：

- 定义 runtime item payload
- 定义 structured result
- 定义 renderer mapping

而不需要在多处新增字符串分支补丁。

### 13.2 行为标准

所有工具都应满足：

- started 时立即出现在 active 区
- delta/progress 过程中实时更新
- completed 时立即提交历史区
- 不依赖 assistant message 才出现工具卡片

### 13.3 协议标准

所有前端展示对象都必须可以回答：

- 它是什么类型的 item
- 它当前状态是什么
- 它来自什么工具来源
- 它有哪些结构化指标
- 它完成后如何进入 transcript/history

### 13.4 扩展标准

后续新增以下能力时，不应再大改基础协议：

- diff viewer
- token metrics
- hosted search family tools
- dynamic tool calls
- IDE/web 前端支持

## 14. 最终建议

这次不建议继续做局部修补式优化，例如：

- 给 `web_search` 多补几条 reducer 特判
- 继续加 event boundary 临时逻辑
- 在 bottom pane 再新增几个字符串分支

这些都不能解决“协议层不统一”的根因。

正确方向是：

1. 建立强类型 runtime item 协议
2. 统一所有工具来源进入相同生命周期
3. 将 transcript/history 收敛为稳定摘要层
4. 让 UI 从类型系统推导，而不是从字符串猜

只有这样，`cloudagent` 后续才有机会像 Codex 一样，稳定承载：

- 实时工具运行展示
- 编辑 diff 可视化
- 工具级 token / metrics
- 多前端一致协议
- 长期迭代而不反复返工

