# CloudAgent 工具链路重构实施方案（细节版）

## 当前状态（2026-06-21）

这份文档暂时不建议删除。

当前 Phase A 的完成情况：

- `TranscriptItem::WebSearch` 已删除。
- `StructuredToolResult::WebSearch` 已接入完成态工具结果。
- `agent-core` / `agent-app-server` / `agent-protocol` / `agent-gateway` 已完成第一轮迁移，能围绕标准 `ToolResult` 继续工作。
- CLI 的历史区与 active 区已经能围绕标准 `ToolResult` 表达 web search 结果，不再依赖旧 transcript 变体。
- CLI 底部运行态已删除 `WebSearchRuntimeState` 特例，web search 与其他工具共用 `ToolRuntimeState`。
- CLI 运行时事件边界已收敛为基于 `TranscriptItem` 种类的通用判断，不再对 web search 做独立 render boundary 特判。
- `cli/src/app/core/active_turn.rs` 与 `crates/agent-gateway/src/adapter/weixin/runtime.rs` 的 web search 完成态专门分支已清理，web search 现在遵循标准 `ToolResult` 生命周期。

当前 Phase A 剩余状态：

- 已完成，无额外收尾项。

当前 Phase B 的完成情况：

- 核心链路已完成，仍有少量后续演进项未做。

Phase B 仍未完成的核心项：

- `RuntimeItem` / `RuntimeItemMetrics` 已引入，`EventMsg::ItemStarted / ItemCompleted` 以及 app-server / protocol started/completed 通知已切到 `RuntimeItem`。
- `RuntimeItemProgress`、`EventMsg::ItemProgress / ItemMetricsUpdated` 以及 app-server / protocol / gateway 对应通知已接通，CLI active / bottom banner 已能消费标准 progress / metrics 更新。
- `ToolSource` 已扩展为 `BuiltIn / Hosted / Mcp / Dynamic`，`web_search` 已作为第一批 hosted 工具接入统一 runtime item 元数据。
- `JsonPatch` 已作为标准运行时 delta 打通到 CLI active 展示链路，文件编辑类工具不再依赖额外的文件变更输出特例。
- 运行中 turn 的 restore 已切到 runtime-item-first，app-server 投影状态也已持久化 `tool_identity / structured / progress / metrics / patch`，并且 `ConversationViewChanged` 的 active-turn 去重现在会比较 runtime snapshot，避免 progress / metrics 更新被静默吞掉。
- `web_search` 开始态的兼容 `ToolOutput` delta 桥接已删除，现在遵循 `ItemStarted / ItemProgress / ItemCompleted`。
- CLI 的工具分类与部分聚合逻辑仍依赖 `tool_name` / `StructuredToolResult` / summary 字符串启发式，并非完全由统一 runtime metadata 驱动。
- `RuntimeItemMetrics` 已在 CLI active / bottom banner 接入工具级 token 展示，但历史区仍没有 completed 后的 metrics 持久展示。
- patch 目前已经能随 runtime snapshot 一起 restore 到 active 区，但历史区仍以 completed 摘要为主，还没有结构化 diff viewer。
- gateway 已不再丢弃 `ItemProgress / ItemMetricsUpdated`，但各 adapter 仍以安全降级为主，尚未实现和 CLI 同等丰富的运行态 UI。

结论：

- 这份文档仍然保留价值，但它下面的大量 Phase A / 早期 Phase B 内容已经转为历史背景。
- 以后继续实施时，应以 `0.1` 和 `0.2` 的“当前剩余项 + 下一步切片”为准，而不是继续参考后文那些已经完成的旧迁移步骤。

### 0.1 本轮审计后的剩余问题清单（2026-06-21）

下面这批问题，是当前代码中仍然还没走到最终形态的关键点。它们已经从“web search 能不能接入标准链路”转成“链路打通后，如何做长期可维护演进”。

#### A. 历史区仍未具备结构化 diff / metrics 展示

- [crates/agent-app-server/src/projection/transcript_item_projection.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\transcript_item_projection.rs)
- [crates/agent-app-server/src/projection/turn_projection_state.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\turn_projection_state.rs)
- [cli/src/ui/history_cell/render.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\render.rs)
- [cli/src/ui/history_cell/tool_ui.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_ui.rs)
- [cli/src/ui/history_cell/display.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\display.rs)
- [cli/src/ui/history_cell/render_entry_tests.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\render_entry_tests.rs)

现状：

- active 区的 patch / metrics 已经能恢复，completed 后却仍然只落成摘要文本。
- `HistoryCell::edit(...)` 仍以“edited N files + path list”为主，缺少结构化 diff / metrics 的 completed 呈现。
- 这意味着后端虽然已经保留了一部分 richer runtime metadata，但历史区没有把这些数据吃进去。

#### B. CLI 分类仍有名称和字符串推断兜底

- [cli/src/ui/history_cell/tool_operation.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_operation.rs)
- [cli/src/ui/history_cell/tool_ui.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_ui.rs)
- [crates/agent-core/src/projection/turn_output.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\turn_output.rs)
- [cli/src/ui/history_cell/tool_operation_tests.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_operation_tests.rs)

现状：

- `tool_identity` / `structured` 已经开始成为主判定来源，但仍保留 `tool_name` / summary 字符串 fallback。
- `turn_output.rs` 里的普通工具聚合仍会读取 summary 文本做启发式归类。
- 这层 fallback 现在主要是为了兼容未结构化的旧工具结果，而不是 web search 专项特判，但长期看仍会让新工具接入继续依赖经验规则。

#### C. gateway 已接线，但还没有富展示

- [crates/agent-gateway/src/adapter/weixin/runtime.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\weixin\runtime.rs)
- [crates/agent-gateway/src/adapter/weixin/outbound.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\weixin\outbound.rs)
- [crates/agent-gateway/src/adapter/wecom/runtime.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\wecom\runtime.rs)
- [crates/agent-gateway/src/adapter/wecom/outbound.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\wecom\outbound.rs)
- [crates/agent-gateway/src/adapter/feishu/runtime.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\feishu\runtime.rs)
- [crates/agent-gateway/src/adapter/feishu/render.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\feishu\render.rs)

现状：

- `ItemProgress / ItemMetricsUpdated` 已能穿过 gateway，但 weixin / wecom / feishu 目前主要用于 runtime 协调、日志和 typing，不会像 CLI 一样展示完整工具卡片。
- 这一块属于“跨前端一致性”后续演进，不再是 web search 基础接入阻塞项。

#### D. 命令工具与通用工具的 active 路径仍有分裂

- [cli/src/app/core/active_turn.rs](D:\learn\gifti\cloudagent\cli\src\app\core\active_turn.rs)
- [cli/src/state/bottom_pane_runtime.rs](D:\learn\gifti\cloudagent\cli\src\state\bottom_pane_runtime.rs)
- [cli/src/app/conversation/actions/server_actions.rs](D:\learn\gifti\cloudagent\cli\src\app\conversation\actions\server_actions.rs)

现状：

- `ActiveTurnAction::StartItem` 遇到 `TurnItemKind::CommandExecution` 仍不创建普通 live item，而是提前返回。
- 命令运行态主要走 bottom banner 的 `CommandRuntimeState`，普通工具走 `ToolRuntimeState`。
- 现在行为是对的，但底层仍是“命令一条链、其他工具另一条链”，这会让后续做统一 active history/diff/token 展示时持续遇到分支。

## 0.2 下一阶段实施顺序（基于当前实际代码）

这部分替代原来过于宽泛的 “继续做 Phase B” 表述。下面 4 个 slice 是按当前收益/风险排序后的真实下一步，不再包含已经做完的迁移动作。

### Slice 1：历史区接入 completed metrics 与 patch 摘要

目标：

- completed 后的工具卡片，不再只显示“摘要文本”，而能显示结构化 metrics / patch 摘要。
- 先做“摘要增强版历史卡片”，不在这一轮直接做完整 diff viewer。

需要修改：

- [cli/src/ui/history_cell/tool_ui.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_ui.rs)
  - 为 `StructuredToolResult::EditFile` / `CommandExecution` / `WebSearch` 增加统一的 completed metrics detail builder。
  - 对 edit 类结果，把 `changed_paths + patch 摘要 + metrics` 组合成 detail，而不是只显示 path list。
- [cli/src/ui/history_cell/render.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\render.rs)
  - 保持 `render_history_entry(...)` 入口不变，但让 `ToolResult` / `FileChange` 都能走 richer detail。
- [cli/src/ui/history_cell/display.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\display.rs)
  - 如 detail 为多行 patch 摘要，确认 `Edit` 卡片展示格式稳定，不出现截断错乱。
- [crates/agent-app-server/src/projection/transcript_item_projection.rs](D:\learn\gifti\cloudagent\crates\agent-app-server\src\projection\transcript_item_projection.rs)
  - 评估是否需要把 `patch_buffer` 的摘要版投影到 completed transcript item 的 `summary` / `content`。
  - 如果不投影全文 patch，至少要提供稳定的摘要来源，避免 CLI 再次自己猜。

建议新增/抽离：

- 在 CLI 新增一个小型 helper，例如 `completed_tool_detail.rs` 或复用 [cli/src/runtime_metrics_display.rs](D:\learn\gifti\cloudagent\cli\src\runtime_metrics_display.rs)，统一生成历史区 metrics 文案。

测试：

- [cli/src/ui/history_cell/render_entry_tests.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\render_entry_tests.rs)
  - 新增 edit completed 带 metrics / patch 摘要的渲染断言。
- [cli/src/app/tests.rs](D:\learn\gifti\cloudagent\cli\src\app\tests.rs)
  - 新增 completed 后历史区保留 richer detail 的集成测试。

### Slice 2：继续去掉 CLI / turn output 的名称启发式 fallback

目标：

- 让工具分类与 turn output 尽量从 `tool_identity + structured` 推导，而不是继续读 `tool_name` / `summary`。
- 对无法结构化的旧工具结果，保留一个集中式 fallback，而不是分散在多个文件里各猜各的。

需要修改：

- [cli/src/ui/history_cell/tool_operation.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_operation.rs)
  - 把 `classify_tool_name(...)` 限制为最后 fallback。
  - 新增一个“非结构化 fallback 只在单点生效”的 helper，例如 `classify_unstructured_tool_result(...)`。
- [cli/src/ui/history_cell/tool_ui.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_ui.rs)
  - `render_tool_result(...)` 中优先吃 `structured`，其次吃 `identity`，最后才退到 `tool_name`。
  - 移除对 `WEB_SEARCH_TOOL_NAME` 这类名称常量的直接依赖。
- [crates/agent-core/src/projection/turn_output.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\turn_output.rs)
  - 把普通工具输出聚合逻辑中的 summary 文本启发式收缩到一个 helper。
  - 对 `StructuredToolResult::ToolError` 优先走 `tool_name + identity`，不再直接从错误 summary 猜类型。

测试：

- [cli/src/ui/history_cell/tool_operation_tests.rs](D:\learn\gifti\cloudagent\cli\src\ui\history_cell\tool_operation_tests.rs)
  - 增加 built-in / hosted / mcp / dynamic 四类 identity 断言。
- [crates/agent-core/src/projection/turn_output_tests.rs](D:\learn\gifti\cloudagent\crates\agent-core\src\projection\turn_output_tests.rs)
  - 锁住 structured 优先、summary fallback 最后触发。

### Slice 3：统一命令与通用工具的 active 展示内核

目标：

- 把“命令工具单独一套 active 流程、其他工具另一套流程”收成统一的 runtime item 展示内核。
- 不要求这轮就把 UI 全改成同一视觉组件，但内部状态机要尽量统一。

- [cli/src/app/core/active_turn.rs](D:\learn\gifti\cloudagent\cli\src\app\core\active_turn.rs)
  - 改掉 `StartItem` 遇到 `CommandExecution` 直接提前返回的特殊分支。
  - 让命令也能拥有一个标准 live item，只是底部 banner 继续可选地投影最近输出。
- [cli/src/state/bottom_pane_runtime.rs](D:\learn\gifti\cloudagent\cli\src\state\bottom_pane_runtime.rs)
  - 把 `CommandRuntimeState` 与 `ToolRuntimeState` 的重复字段和逻辑继续抽象。
  - 保留命令输出 delta 特殊能力，但避免“是否命令”决定整条 active 生命周期分裂。
- [cli/src/app/conversation/actions/server_actions.rs](D:\learn\gifti\cloudagent\cli\src\app\conversation\actions\server_actions.rs)
  - completed 收尾统一经一个 helper 处理，避免 command / tool 两套完成路径继续发散。
- [cli/src/app/tests.rs](D:\learn\gifti\cloudagent\cli\src\app\tests.rs)
  - 增加 command 与 generic tool 在 start/progress/completed 生命周期上的一致性断言。

### Slice 4：gateway 的富运行态展示第一版

目标：

- 在不引入复杂前端状态机的前提下，让 gateway 适配器至少能把 progress / metrics 变成稳定的“运行态文案”，而不是仅用于 typing / ignore。

需要修改：

- [crates/agent-gateway/src/adapter/weixin/runtime.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\weixin\runtime.rs)
  - 为 `GatewayEvent::ItemProgress` / `ItemMetricsUpdated` 增加统一的“工具运行态文案”聚合，不只打日志。
- [crates/agent-gateway/src/adapter/wecom/runtime.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\wecom\runtime.rs)
  - 同上，保持和 weixin 同一套文案 helper。
- [crates/agent-gateway/src/adapter/feishu/render.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\feishu\render.rs)
  - 支持 progress / metrics 的安全文本渲染，先不做复杂卡片。
- [crates/agent-gateway/src/adapter/weixin/outbound.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\weixin\outbound.rs)
- [crates/agent-gateway/src/adapter/wecom/outbound.rs](D:\learn\gifti\cloudagent\crates\agent-gateway\src\adapter\wecom\outbound.rs)
  - 决定这些 runtime 文案是合并发送、节流发送还是只更新 typing，不要让每个 adapter 自己散落判断。

建议新增/抽离：

- 在 `agent-gateway` 新增一个共享 helper，例如 `runtime_progress_text.rs`，从 `RuntimeItemProgress` / `RuntimeItemMetrics` 统一生成跨 adapter 文案。

测试：

- `crates/agent-gateway/src/adapter/*/*_tests.rs`
  - 增加 progress / metrics 文案或安全降级行为的断言。

### 暂不在本轮推进的事项

- 完整历史 diff viewer
- completed 后可展开查看完整 patch
- Web / IDE 前端复用同一套 richer runtime 卡片

这些能力需要单独开下一轮，不建议和上面 4 个 slice 混做。

## 0.3 文档使用说明

- 本文档从这里往下的 `Phase A / Phase B` 大段内容，主要用于保留历史迁移背景和设计动机。
- 它们包含大量已经完成的改造步骤，不能再直接当作待办清单执行。
- 真正还要继续做什么，以 `0.1` 和 `0.2` 为准。

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
