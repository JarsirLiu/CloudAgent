# CloudAgent 工具链路重构实施方案（细节版）

## 1. 文档目的

本文档是 [tooling-pipeline-refactor-plan.zh-CN.md](D:\learn\gifti\cloudagent\docs\tooling-pipeline-refactor-plan.zh-CN.md) 的实施级细化版本。

目标不是重复架构口号，而是明确：

- 每个 crate 的职责调整
- 每个文件需要新增、删除、替换的内容
- 关键函数的改造策略
- 需要迁移或删除的兼容逻辑
- 每一阶段应补哪些测试

本文档默认采用以下原则：

- 不继续沿用 `TranscriptItem::WebSearch` 这类工具特例
- 不在旧链路上继续叠加 `if web_search { ... }` 式补丁
- 先建立标准运行时 item 协议，再把 `web_search` 作为第一批 hosted 工具迁入
- 改动以“高内聚、低耦合、可删除旧路径”为目标

## 2. 当前实际受影响模块总览

### 2.1 `agent-core`

核心受影响文件：

- [crates/agent-core/src/model/mod.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\model\mod.rs)
- [crates/agent-core/src/turn/events.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\turn\events.rs)
- [crates/agent-core/src/conversation/items.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\conversation\items.rs)
- [crates/agent-core/src/turn/execution/streaming.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\turn\execution\streaming.rs)
- [crates/agent-core/src/turn/execution/response.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\turn\execution\response.rs)
- [crates/agent-core/src/projection/transcript.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\transcript.rs)
- [crates/agent-core/src/projection/core_transcript.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\core_transcript.rs)
- [crates/agent-core/src/projection/turn_output.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\turn_output.rs)
- [crates/agent-core/src/lib.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\lib.rs)

### 2.2 `agent-app-server`

核心受影响文件：

- [crates/agent-app-server/src/projection/conversation_notifications.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\conversation_notifications.rs)
- [crates/agent-app-server/src/projection/transcript_item_projection.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\transcript_item_projection.rs)
- [crates/agent-app-server/src/projection/turn_projection_state.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\turn_projection_state.rs)

### 2.3 `agent-protocol`

协议同步文件：

- [crates/agent-protocol/src/messages.rs](D:\learn\gifti\cloudagent\crates\agent-protocol\src\messages.rs)
- [crates/agent-protocol/src/wire.rs](D:\learn\gifti\cloudagent\crates\agent-protocol\src\wire.rs)
- 必要时补充 `view_state.rs` / `types.rs`

### 2.4 `cli`

核心受影响文件：

- [cli/src/state/reducer.rs](D:\learn\gifti\cloudagent\cli\src\state\reducer.rs)
- [cli/src/state/bottom_pane_runtime.rs](D:\learn\gifti\cloudagent\cli\src\state\bottom_pane_runtime.rs)
- [cli/src/state/bottom_pane_controller.rs](D:\learn\gifti\cloudagent\cli\src\state\bottom_pane_controller.rs)
- [cli/src/app/conversation/actions/server_actions.rs](D:\learn\gifti\cloudagent\cli\src\app\conversation\actions\server_actions.rs)
- [cli/src/app/core/active_turn.rs](D:\learn\gifti\cloudagent\cli\src\app\core\active_turn.rs)
- [cli/src/app/runtime/controller.rs](D:\learn\gifti\cloudagent\cli\src\app\runtime\controller.rs)
- [cli/src/ui/history_cell/render.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\render.rs)
- [cli/src/ui/history_cell/tool_operation.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_operation.rs)
- [cli/src/ui/history_cell/tool_ui.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_ui.rs)
- [cli/src/tool_identity.rs](D:\learn\gifti\cloudagent\cli\src\tool_identity.rs)

### 2.5 `agent-gateway`

因为 gateway 直接消费 `TranscriptItem` 和 app-server 通知，以下文件也必须同步：

- `crates/agent-gateway/src/app_server_mapping.rs`
- `crates/agent-gateway/src/gateway_event.rs`
- `crates/agent-gateway/src/adapter/*/runtime.rs`
- `crates/agent-gateway/src/adapter/*/outbound.rs`
- `crates/agent-gateway/src/adapter/*/render.rs`

当前它们对 `TranscriptItem::WebSearch` 有显式分支，必须一起收敛。

## 3. 重构总策略

本次实施不建议一步到位全面替换所有 runtime / transcript 模型，而应分两层推进：

### 第一层：消除 `WebSearch` 特例，收敛到标准工具结果

目标：

- 删除 `TranscriptItem::WebSearch`
- 新增 `StructuredToolResult::WebSearch`
- 让 `web_search` 完成态统一落到 `TranscriptItem::ToolResult`
- 保持现有 `EventMsg::ItemStarted/ItemDelta/ItemCompleted` 大框架不变

收益：

- 先把最明显的架构裂缝缝合
- 降低后续引入真正 `RuntimeItem` 模型的复杂度
- 能较快恢复 `web_search` 与其他工具一致的历史展示链路

### 第二层：将 started/completed payload 从“推导式”升级到“完整 item 协议”

目标：

- 引入 `RuntimeItem`
- `ItemStarted` 改为携带完整 item
- CLI active/history/bottom banner 统一消费标准 runtime item

收益：

- 从根上解决新工具接入问题
- 为 diff、metrics、tokens、future web UI 预留协议

本文档中的 Phase A 对应第一层，Phase B 对应第二层。

## 4. Phase A：先消除 `WebSearch` 特例

---

## 4.1 `agent-core`：工具结果模型收口

### 文件

[crates/agent-core/src/tool/mod.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\tool\mod.rs)

### 改动目标

在 `StructuredToolResult` 中新增标准化 web search 结构。

### 需要修改的位置

当前 `StructuredToolResult` 定义起点在该文件约 `240+` 行。

### 新增内容

新增变体：

```rust
WebSearch {
    query: String,
    action: Option<WebSearchAction>,
    result_count: Option<usize>,
    source_count: Option<usize>,
}
```

### 依赖调整

- 当前 `WebSearchAction` 定义在 [crates/agent-core/src/model/mod.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\model\mod.rs:45)
- 需要确保 `tool/mod.rs` 能访问 `WebSearchAction`
- 可通过 `use crate::WebSearchAction;` 或提升导出实现

### 额外建议

如果后续准备支持 metrics，建议同时新增：

```rust
ToolRuntimeMetrics {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    elapsed_ms: Option<u64>,
    source_count: Option<usize>,
    result_count: Option<usize>,
}
```

但 Phase A 可以先不落地。

---

## 4.2 `agent-core`：删除 `TranscriptItem::WebSearch`

### 文件

[crates/agent-core/src/conversation/items.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\conversation\items.rs)

### 当前问题

`TranscriptItem` 中存在：

```rust
WebSearch {
    id: String,
    query: String,
    action: Option<WebSearchAction>,
}
```

这破坏了工具结果的封闭性。

### 需要修改的函数

#### 1. `enum TranscriptItem`

- 删除 `WebSearch` 变体

#### 2. `TranscriptItem::id()`

- 删除 `| Self::WebSearch { id, .. } => id`

#### 3. 文件顶部 imports

- 删除 `use crate::WebSearchAction;`

### 注意事项

此改动会波及所有 `match TranscriptItem` 的地方，必须同步改完所有引用点后再跑编译。

---

## 4.3 `agent-core`：将 web search 完成态改为标准 ToolResult

### 文件

[crates/agent-core/src/turn/execution/streaming.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\turn\execution\streaming.rs)

### 需要修改的函数

#### 1. `TurnStreamObserver::on_web_search_started`

当前逻辑：

- 发 `ItemStarted`
- `kind = TurnItemKind::ToolResult`
- `title = Some("web_search")`
- 然后发一个 `ToolOutputDelta` 写 query

### Phase A 处理方式

此函数先保留生命周期时序，但需做两点整理：

- `item_id` 统一改成和其他工具一样，建议从 `id` 映射成 `tool:{call_id}`，避免 hosted 和本地工具 item id 形态不一致
- started title 仍然使用 `"web_search"`，但不再期待 completed 是特殊 TranscriptItem

#### 2. `TurnStreamObserver::on_web_search_completed`

当前逻辑：

```rust
EventMsg::ItemCompleted {
    item: TranscriptItem::WebSearch { ... }
}
```

### 改造目标

改为：

```rust
EventMsg::ItemCompleted {
    item: TranscriptItem::ToolResult {
        id: item_id,
        tool_name: "web_search".to_string(),
        content: ...,
        summary: ...,
        structured: Some(StructuredToolResult::WebSearch { ... }),
    }
}
```

### `content/summary` 建议

保持与 CLI 现有独立 Web UI 一致，建议：

- `summary`: `"searched the web"` 或按 action 生成
- `content`: 存放 detail 文本，例如 query / open page / find in page 的格式化结果

更推荐新增一个共享 helper：

- `render_web_search_summary(action: Option<&WebSearchAction>) -> String`
- `render_web_search_content(query: &str, action: Option<&WebSearchAction>) -> String`

建议落点：

- `crates/agent-core/src/projection/transcript.rs` 中现有 web search detail helper 可迁为公共 helper
- 或新增 `crates/agent-core/src/web_search_presentation.rs`

### 额外注意

如果 started item id 改成 `tool:{call_id}`，completed 也要一致。

---

## 4.4 `agent-core`：录制 rollout 时不再持久化 WebSearch 特例

### 文件

[crates/agent-core/src/turn/execution/response.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\turn\execution\response.rs)

### 需要修改的函数

#### `record_model_response`

当前代码会对 `response.web_searches` 额外生成：

```rust
RolloutItem::from(EventMsg::ItemCompleted {
    item: TranscriptItem::WebSearch { ... }
})
```

### 改造目标

改为标准 `TranscriptItem::ToolResult` 的 `ItemCompleted` rollout。

### 注意事项

如果 streaming 阶段已经发过同样的 `ItemCompleted` 并且 rollout 已记录，应避免重复持久化。

建议检查：

- 当前 rollout 记录职责是“运行时 event 持久化”还是“response 收尾补记”
- 如果已有 streaming event 被持久化，则此处应删除 web search 补写逻辑
- 如果 streaming event 未持久化，则此处改成持久化标准 ToolResult item

建议优先做法：

- 保持现有机制，但把补写项改成标准 ToolResult
- 后续 Phase B 再清理 response 收尾补写的冗余

---

## 4.5 `agent-core`：统一 transcript 投影

### 文件

[crates/agent-core/src/projection/transcript.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\transcript.rs)

### 需要修改的函数

#### 1. `transcript_item_from_item_start`

当前：

- `TurnItemKind::ToolCall | TurnItemKind::ToolResult => TranscriptItem::ToolResult`

Phase A 保留这个大体策略，不在这里做协议层重构。

#### 2. `transcript_item_from_tool_response`

当前只对：

- `CommandExecution`
- `EditFile`

做专门投影，其余进入 `ToolResult`

### 改造目标

保持 `WebSearch` 也走标准 `ToolResult`

这意味着此函数不一定要新增新分支，只要 `StructuredToolResult::WebSearch` 不被特殊转成别的 `TranscriptItem` 即可。

#### 3. `assign_response_item_id`

- 删除 `TranscriptItem::WebSearch` 分支

#### 4. `transcript_item_is_empty`

当前有：

```rust
TranscriptItem::WebSearch { query, action, .. } => ...
```

### 改造目标

删除该分支，`ToolResult` 统一走：

- `summary.trim().is_empty()`

### 建议新增 helper

在本文件或共享模块新增：

- `pub fn web_search_detail(query, action) -> String`
- `pub fn web_search_summary(query, action) -> String`

用于 core / app-server / cli 共用，避免多处复制 detail 文案。

---

## 4.6 `agent-core`：turn output 适配标准 ToolResult

### 文件

[crates/agent-core/src/projection/turn_output.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\turn_output.rs)

### 当前问题

该文件目前对 `TranscriptItem::WebSearch` 有专门分支。

### 改造目标

- 删除 `TranscriptItem::WebSearch` 分支
- 对 `TranscriptItem::ToolResult { structured: Some(StructuredToolResult::WebSearch { .. }) }` 提供标准输出摘要

### 建议

此文件的 summary 文案应和 CLI 历史区 summary 保持一套来源，避免不同层文案漂移。

---

## 4.7 `agent-core`：`core_transcript` 暂不做结构性重写

### 文件

[crates/agent-core/src/projection/core_transcript.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\core_transcript.rs)

### Phase A 策略

这里先不做协议模型大改。

只需保证：

- `ItemCompleted` 带的是标准 ToolResult
- replay/reconstruction 不再依赖 `TranscriptItem::WebSearch`

## 5. Phase A：`agent-app-server` 改造

---

## 5.1 `transcript_item_projection.rs`

### 文件

[crates/agent-app-server/src/projection/transcript_item_projection.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\transcript_item_projection.rs)

### 当前问题

该文件专门处理了 `TranscriptItem::WebSearch`：

- `projected_item_from_transcript_item`
- `turn_item_kind_for_transcript_item`
- `projected_transcript_item_is_empty`
- `web_search_detail`

### 需要修改的函数

#### 1. `projected_item_from_transcript_item`

- 删除 `TranscriptItem::WebSearch` 分支

#### 2. `projected_item_to_transcript_item`

原则上不用新增 web search 特判，仍然让 `TurnItemKind::ToolResult` 落为 `TranscriptItem::ToolResult`

#### 3. `turn_item_kind_for_transcript_item`

- 删除 `TranscriptItem::WebSearch => TurnItemKind::ToolResult`

#### 4. `projected_transcript_item_is_empty`

- 删除 `TranscriptItem::WebSearch` 分支

#### 5. `web_search_detail`

- 删除本地私有实现
- 改为调用共享 helper

### 结果

app-server 不再知道“web search 是一个 transcript 特例”，只知道它是 `ToolResult + StructuredToolResult::WebSearch`

---

## 5.2 `conversation_notifications.rs`

### 文件

[crates/agent-app-server/src/projection/conversation_notifications.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\conversation_notifications.rs)

### Phase A 目标

不重写整个 started/completed 协议，但要让它对 web search 不再依赖 transcript 特例。

### 需要关注的函数与路径

#### 1. `project_turn_event`

当前 started 路径：

- `EventMsg::ItemStarted` -> `observe_item_started(...)`
- 再从 `ProjectedItemState` 或 fallback 推出 started item

Phase A 不改 started 协议形状。

#### 2. `project_core_transcript_event`

当前 completed 路径：

- `CoreTranscriptEvent::ItemCompleted` -> `AppServerNotification::ItemCompleted { item }`

只要 completed item 已经是标准 ToolResult，这里无需 web search 特判。

### 要做的事

- 检查是否还有任何 `TranscriptItem::WebSearch` 相关分支
- 若有，全部删除

### Phase A 不做的事

- 不在此阶段把 `ItemStarted` 改为完整 runtime item 协议

## 6. Phase A：`agent-protocol` 改造

### 文件

- [crates/agent-protocol/src/messages.rs](D:\learn\gifti\cloudagent\crates\agent-protocol\src\messages.rs)
- [crates/agent-protocol/src/wire.rs](D:\learn\gifti\cloudagent\crates\agent-protocol\src\wire.rs)

### 改造目标

因为 `TranscriptItem` 序列化结构变了，协议层要同步：

- 删除 `WebSearch` transcript variant 的协议支持
- 确保 `ToolResult + StructuredToolResult::WebSearch` 可正常序列化/反序列化

### 需要检查

#### `messages.rs`

- `AppServerNotification::ItemStarted`
- `AppServerNotification::ItemCompleted`
- 是否依赖 `TranscriptItem::WebSearch` 的 pattern match

#### `wire.rs`

- 分类函数是否依赖具体 transcript variant
- 测试快照需同步更新

## 7. Phase A：`cli` 改造

---

## 7.1 `state/reducer.rs`

### 文件

[cli/src/state/reducer.rs](D:\learn\gifti\cloudagent\cli\src\state\reducer.rs)

### 当前问题

该文件有最典型的 web search 特判：

- `ItemCompleted` 时 `if matches!(item, TranscriptItem::WebSearch { .. })`
- `turn_item_kind`
- `turn_item_title`

### 需要修改的函数

#### 1. `reduce_app_server_message` 中 `ItemCompleted` 分支

当前：

- web search completed 会 `ClearLastToolName`

### 改造目标

不要再依赖 transcript 特例判断 completed 类型。

改为：

- 如果 `item` 是 `TranscriptItem::ToolResult { tool_name, structured, .. }`
- 且 `tool_name == "web_search"` 或 `structured == StructuredToolResult::WebSearch`
- 则清理 active tool/banner

### 更推荐

不要只看 `tool_name`，引入统一 helper：

```rust
fn is_web_search_result(item: &TranscriptItem) -> bool
```

Phase A 可先用 helper，Phase B 再升级成强类型 runtime item 判断。

#### 2. `turn_item_kind`

- 删除 `TranscriptItem::WebSearch` 分支

#### 3. `turn_item_title`

- 删除 `TranscriptItem::WebSearch => Some("web_search")`
- 对标准 `TranscriptItem::ToolResult` 保持现状

### 新增 helper 建议

在该文件新增：

- `is_web_search_tool_result(item: &TranscriptItem) -> bool`
- `tool_result_name(item: &TranscriptItem) -> Option<&str>`

---

## 7.2 `state/bottom_pane_runtime.rs`

### 文件

[cli/src/state/bottom_pane_runtime.rs](D:\learn\gifti\cloudagent\cli\src\state\bottom_pane_runtime.rs)

### 当前问题

该文件有专门的：

- `active_web_search`
- `WebSearchRuntimeState`
- `TurnItemKind::ToolResult if title.is_some_and(is_web_search_tool_name)`

### Phase A 目标

先不删独立 Web runtime banner 设计，但必须让它基于标准 ToolResult，而不是 `TranscriptItem::WebSearch`

### 需要修改的函数

#### 1. `on_active_item_started`

保持 web search 可以进入独立 runtime state，但 started title 来源应为标准 ToolResult started item。

#### 2. `on_tool_output_delta`

继续让 web search query 通过 delta 更新 active banner。

#### 3. 清理完成逻辑

完成时不再依赖 transcript 特例，而应由 reducer 标准触发收尾。

### Phase B 方向

将 `active_web_search` 与 `active_tool_title` 统一收敛为：

- `active_runtime_item`

但 Phase A 可以先不做。

---

## 7.3 `app/conversation/actions/server_actions.rs`

### 文件

[cli/src/app/conversation/actions/server_actions.rs](D:\learn\gifti\cloudagent\cli\src\app\conversation\actions\server_actions.rs)

### 当前问题

completed 分支只对 `CommandExecution` 做显式状态收尾。

### 改造目标

引入统一工具完成处理 helper，例如：

```rust
fn handle_tool_like_item_completed(app: &mut TuiApp, item_id: &str, item: &TranscriptItem)
```

内部负责：

- command finished
- web search finished
- future hosted tool finished

避免 reducer 和 action 层分别散落 web search 特判。

---

## 7.4 `app/core/active_turn.rs`

### 文件

[cli/src/app/core/active_turn.rs](D:\learn\gifti\cloudagent\cli\src\app\core\active_turn.rs)

### 当前问题

该文件的 `turn_item_kind(item)` 仍接受 `TranscriptItem::WebSearch`

### 改造目标

- 删除 `TranscriptItem::WebSearch` 分支
- 保持 ToolResult 统一作为工具类 active item

### 需要检查

- `ActiveItemView::new(...)`
- active cell 完成后如何提交 history
- running turn restore 是否还引用 web search transcript 特例

---

## 7.5 `ui/history_cell/render.rs`

### 文件

[cli/src/ui/history_cell/render.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\render.rs)

### 当前问题

该文件直接 match：

```rust
TranscriptItem::WebSearch { ... } => ...
```

### 改造目标

改为：

- 对 `TranscriptItem::ToolResult` 分支内部判断
- 若 `structured == StructuredToolResult::WebSearch { .. }`
- 渲染独立 `Web search` UI

### 建议新增 helper

```rust
fn render_web_search_tool_result(
    tool_name: &str,
    structured: Option<&StructuredToolResult>,
    content: &str,
) -> Option<HistoryCell>
```

### 删除内容

- `render_web_search_detail(query, action)` 私有逻辑

改为使用共享 helper 或从 structured 直接渲染。

---

## 7.6 `ui/history_cell/tool_operation.rs`

### 文件

[cli/src/ui/history_cell/tool_operation.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_operation.rs)

### 改造目标

把 web search 的分类依据从：

- `tool_name == "web_search"`

逐步升级为：

- `StructuredToolResult::WebSearch`

Phase A 可以做“双判定但只保留一个 helper 入口”，例如：

```rust
pub(crate) fn classify_tool_operation(
    tool_name: &str,
    structured: Option<&StructuredToolResult>,
) -> ToolOperation
```

内部优先看 structured，再回退到 name。

这样 Phase B 再删 name fallback 会比较顺。

---

## 7.7 `ui/history_cell/tool_ui.rs`

### 文件

[cli/src/ui/history_cell/tool_ui.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_ui.rs)

### 当前问题

这里对 web search 展示依赖：

- `tool_name`
- `WEB_SEARCH_TOOL_NAME`

### 改造目标

统一成：

- `StructuredToolResult::WebSearch` 决定 UI
- `tool_name` 仅作为 fallback

### 建议新增 helper

```rust
fn is_web_search_structured_result(
    structured: Option<&StructuredToolResult>
) -> bool
```

---

## 7.8 `app/runtime/controller.rs`

### 文件

[cli/src/app/runtime/controller.rs](D:\learn\gifti\cloudagent\cli\src\app\runtime\controller.rs)

### 当前问题

这里有最典型的补丁式逻辑：

- `should_stop_after_event_boundary`
- `is_web_search_started_item`

并且 completed 直接 match `TranscriptItem::WebSearch`

### Phase A 策略

先把它从 transcript 特例切换到标准 ToolResult 判断：

#### 需要修改的函数

1. `should_stop_after_event_boundary`
2. `is_web_search_started_item`

### 改造后判断规则

- started：`ItemStarted.item` 是 `ToolResult` 且 `tool_name == web_search`
- completed：`ItemCompleted.item` 是 `ToolResult` 且 `structured == WebSearch` 或 `tool_name == web_search`

### Phase A 说明

这仍是临时 boundary 方案，但至少可以摆脱 `TranscriptItem::WebSearch`。

### Phase B 目标

随着真正 runtime item started payload 落地，这一段 boundary hack 应被尽量缩小甚至删除。

## 8. Phase A：`agent-gateway` 改造

### 原因

gateway 当前也显式识别 `TranscriptItem::WebSearch`。

如果不一起改，会出现：

- CLI 编译过了
- gateway 渲染或 runtime 分类挂掉

### 改造目标

统一把以下判断改成：

- `TranscriptItem::ToolResult + StructuredToolResult::WebSearch`

### 重点文件

- `adapter/weixin/runtime.rs`
- `adapter/weixin/outbound.rs`
- `adapter/wecom/outbound.rs`
- `adapter/feishu/render.rs`

### 建议

在 `agent-gateway` 新增共享 helper：

```rust
fn is_web_search_item(item: &TranscriptItem) -> bool
```

避免多个 adapter 再次分散写 web search 判断。

## 9. Phase A 测试改造清单

### 9.1 `agent-core`

受影响测试：

- `turn/execution/chat_tests.rs`
- `projection/transcript_tests.rs`
- `projection/turn_output_tests.rs`

需要修改：

- 断言 `ItemCompleted` 不再是 `TranscriptItem::WebSearch`
- 改为 `TranscriptItem::ToolResult { structured: Some(WebSearch { .. }) }`

### 9.2 `agent-app-server`

受影响测试：

- `projection/conversation_notifications_tests.rs`

需要新增：

- hosted web search completed 生成标准 `ItemCompleted(ToolResult)`

### 9.3 `cli`

受影响测试：

- `cli/src/app/tests.rs`
- `cli/src/state/reducer_tests.rs`
- `cli/src/app/runtime/controller_tests.rs`
- `cli/src/ui/history_cell/render_entry_tests.rs`
- `cli/src/state/bottom_pane_controller_tests.rs`

需要修改：

- 所有引用 `TranscriptItem::WebSearch` 的断言

### 9.4 `agent-gateway`

受影响测试：

- weixin / wecom / feishu renderer tests

需要修改：

- event type / render path 不再匹配 `TranscriptItem::WebSearch`

## 10. Phase B：真正引入 Runtime Item 协议

Phase A 只是把 web search 收回标准工具结果，不是最终形态。

下一步 Phase B 才是根治：

- started 不再用 `kind + title`
- active UI 不再依赖 TranscriptItem 占位推导
- 所有前端统一消费强类型 runtime item

---

## 10.1 `agent-core` 新增 `runtime_item.rs`

### 新文件

建议新增：

- `crates/agent-core/src/runtime_item.rs`
- `crates/agent-core/src/runtime_metrics.rs`

### 主要内容

```rust
pub enum RuntimeItem {
    AssistantMessage { ... },
    Reasoning { ... },
    CommandExecution { ... },
    FileChange { ... },
    ToolCall { ... },
    ToolResult { ... },
    WebSearch { ... },
}
```

### 注意

Phase B 中，`RuntimeItem::WebSearch` 是“运行协议 item”，而不是 transcript item。

这点和 `TranscriptItem::WebSearch` 是完全不同的概念。

## 10.2 `turn/events.rs`

### 需要修改

将：

```rust
ItemStarted {
    item_id,
    call_id,
    kind,
    title,
}
```

改为：

```rust
ItemStarted {
    turn_id,
    item: RuntimeItem,
}
```

将：

```rust
ItemCompleted {
    turn_id,
    item_id,
    call_id,
    item: TranscriptItem,
}
```

拆分或改造为：

```rust
ItemCompleted {
    turn_id,
    item: RuntimeItem,
    transcript_item: Option<TranscriptItem>,
}
```

或者：

- 保持 runtime completed
- transcript/history 通过单独投影生成

### 推荐方案

建议保留一个 event，只是 payload 带两层：

```rust
ItemCompleted {
    turn_id,
    runtime_item: RuntimeItem,
    transcript_item: Option<TranscriptItem>,
}
```

好处：

- 前端拿 runtime item
- history/replay 拿 transcript item
- 避免 started/completed 使用不同模型

## 10.3 `agent-app-server`

### `conversation_notifications.rs`

需要重构：

- `EventMsg::ItemStarted` 到 `AppServerNotification::ItemStarted`
- `EventMsg::ItemCompleted` 到 `AppServerNotification::ItemCompleted`

从“基于 projected transcript state 推 started item”改成：

- 直接发 runtime item

### `transcript_item_projection.rs`

此文件职责要缩窄：

- 专注 transcript/history rebuild
- 不再承担 runtime started payload 推导

## 10.4 `agent-protocol`

### `messages.rs`

新增/替换：

- `RuntimeItem`
- `RuntimeItemMetrics`
- `RuntimeItemProgress`

将 `AppServerNotification::ItemStarted/Completed` 的 item 类型由 `TranscriptItem` 升级为 `RuntimeItem` 或包装结构。

## 10.5 `cli`

### `state/reducer.rs`

从：

- `turn_item_kind(item: &TranscriptItem)`
- `turn_item_title(item: &TranscriptItem)`

过渡为：

- `runtime_item_kind(item: &RuntimeItem)`
- `runtime_item_title(item: &RuntimeItem)`

### `active_turn.rs`

active cell 不再由 transcript placeholder 推导，而是直接由 runtime item started 创建。

### `history_cell/render.rs`

历史区继续吃 `TranscriptItem`，但 active 区吃 `RuntimeItem`。

这就是运行协议和历史摘要真正分层的关键点。

## 11. 实施顺序建议

### Step 1

完成 Phase A：

- 删除 `TranscriptItem::WebSearch`
- 引入 `StructuredToolResult::WebSearch`
- 迁移 CLI / gateway / app-server 相关特判

### Step 2

在一个独立 PR/提交中引入：

- `RuntimeItem`
- `RuntimeMetrics`
- started/active 协议重构

### Step 3

把 CLI active/bottom banner 迁移到 runtime item

### Step 4

再扩展：

- diff patch event
- metrics update event
- token display

## 12. 验收标准（实施级）

### Phase A 完成标准

- 全仓库不再存在 `TranscriptItem::WebSearch`
- `web_search` completed 统一为 `ToolResult + StructuredToolResult::WebSearch`
- CLI 历史区、active 区、bottom banner 行为与其他工具一致
- `controller.rs` 中不再显式匹配 `TranscriptItem::WebSearch`
- gateway 各 adapter 编译通过且测试通过

### Phase B 完成标准

- `ItemStarted` 不再依赖 `kind + title`
- active UI 不再从 transcript 占位推导
- `started -> delta -> completed` 的所有前端渲染都只依赖 runtime item 协议
- 新增 hosted 工具无需再横切多层补字符串逻辑

## 13. 最后说明

这份细化文档的重点是：

- 先做可收敛的结构性修复
- 再做真正的协议升级

不要反过来做：

- 先在 CLI 里继续修渲染
- 再在 reducer 里继续打补丁
- 最后发现 core 模型还是裂开的

正确顺序必须是：

1. 先收口 core 工具结果模型
2. 再收口 app-server / protocol
3. 再清理 CLI 和 gateway 的展示特判
4. 最后升级到完整 runtime item 协议

