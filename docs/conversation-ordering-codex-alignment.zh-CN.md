# 会话顺序对齐 Codex 改造方案

这份文档定义 `cloudagent` 会话事件投影链路对齐 Codex 的长期目标、边界和执行清单。

目标不是继续在 CLI 层补“迟到 reasoning 回插”之类的显示修复，而是把顺序保证上提到 `agent-app-server` 的投影层，让 UI 只消费稳定的 turn/item 语义。

## 结论

- 当前问题的根因，不在单条通知流本身，而在于上游协议允许 `reasoning`、`assistant`、`tool` 并行产出。
- Codex 值得对齐的核心，不是“单 SSE 流”这个传输形式，而是“上游先归并成稳定 item 模型，再让 UI 渲染”。
- `cloudagent` 的长期方案，应把 `ConversationNotificationProjector` 从“事件格式转换器”升级成“turn/item reducer + stable projection”。

## 当前链路

当前链路如下：

- `agent-core` 产出 `EventMsg`
- `agent-app-server` 的 [`conversation_notifications.rs`](/D:/learn/gifti/cloudagent/crates/agent-app-server/src/projection/conversation_notifications.rs) 将 `EventMsg` 立即投影为 `AppServerNotification`
- CLI 收到什么就立刻处理什么
- 当 `reasoning` / `assistant` / `tool` 并行到达时，UI 只能被动修复错序

当前 projector 已维护：

- `active_items_by_item_id`
- `active_item_id_by_call_id`

但这些状态只用于生命周期校验，不用于顺序归并或稳定视图输出。

## 目标形态

目标形态有三个原则：

1. `agent-app-server` 内部允许接收并行原始事件。
2. 对下游输出时，必须先归并成稳定的 `turn -> items` 语义模型。
3. live 更新和 history rebuild 必须共用同一份 reducer 结果，而不是各自推断顺序。

这意味着 `ConversationNotificationProjector` 未来不再是：

- 来一个事件，投一个通知

而是：

- 来一个事件，更新 turn/item 状态
- 重算该 turn 的稳定可见顺序
- 向下游发出 item 级稳定 patch 或快照

## 不采用的方案

以下方案都不作为长期终点：

- 继续在 CLI 层做回插、删重、隐藏
- 在 projector 层仅按 `kind` 做事件重排后再转发原始 started/delta/completed 流
- 强行把当前真实并行协议压成“单 active item”协议

原因：

- CLI 层补丁不能统一 live 与 rebuild 语义
- 单纯事件重排仍然输出原始事件语义，长期仍会反复遇到归属和排序问题
- 当前系统的 provider/backend 已经是并行 item 生产模型，简单压扁会损失真实语义

## 最终目标设计

`ConversationNotificationProjector` 最终应维护如下概念：

- `TurnProjectionState`
- `ProjectedItemState`
- 稳定的 item 顺序
- item 与 assistant/block 的显式归属关系

建议最小状态如下：

```rust
struct TurnProjectionState {
    turn_id: String,
    items_in_order: Vec<String>,
}

struct ProjectedItemState {
    item_id: String,
    turn_id: String,
    call_id: Option<String>,
    kind: TurnItemKind,
    title: Option<String>,
    status: ProjectedItemStatus,
    text_buffer: String,
    reasoning_buffer: String,
    tool_output_buffer: String,
    order_hint: u64,
}
```

其中长期必须保证：

- 同一个 `item_id` 始终代表同一个逻辑对象
- item 顺序由 reducer 决定，不由到达时序决定
- reasoning 是否位于 assistant 前，由模型表达，不由 UI 猜测
- rebuild 和 live 最终顺序完全一致

## 分阶段执行策略

### Phase 1：在 projector 内引入 reducer 状态骨架

目标：

- 不改变现有 CLI 行为
- 在 `conversation_notifications.rs` 内建立 turn/item 状态容器
- 先把 projector 从“纯透传器”改造成“有状态投影器”

产出：

- `TurnProjectionState`
- `ProjectedItemState`
- `ProjectedItemStatus`
- `turns_by_turn_id`
- `items_by_item_id`
- item 生命周期和 projector 内状态同步

验收：

- 现有测试全部通过
- 对外通知行为保持不变

### Phase 2：改成 item 状态驱动，而不是事件流驱动

目标：

- 收到 `ItemStarted` / `ItemDelta` / `ItemCompleted` 时先更新内部 item state
- delta 不再只做即时转发，还要进入 item buffer
- 让 projector 能产出“某个 item 当前状态”的稳定投影

产出：

- item 级更新函数
- turn 内稳定顺序函数
- reasoning / assistant / tool 的显式归属规则

验收：

- 可以从 reducer 状态重建单 turn 的稳定 item 列表
- projector 内部不再依赖“谁先到就先出”

### Phase 3：切换 app-server 到 CLI 协议主链

目标：

- 为 CLI 增加 item 状态型通知或 turn patch 通知
- 逐步减少 CLI 对原始 delta 排序的依赖

候选方向：

- `TurnItemsReplaced`
- `TurnItemUpdated`
- `TurnSnapshot`

验收：

- CLI 能从状态型通知直接渲染稳定历史
- live 与 rebuild 使用同一排序来源

### Phase 4：删除 CLI 侧顺序补丁逻辑

目标：

- 移除 trailing assistant 回插类逻辑
- 移除 turn complete 时的顺序补丁语义
- 让前端只保留渲染与交互

验收：

- 顺序保证完全来自 app-server reducer
- CLI 不再依赖“最后一条 assistant”推断 reasoning 归属

## Checklist

下面 checklist 按执行顺序排列，只有前一步完成后才进入后一步。

### A. Projector 状态化

- [x] 为 `ConversationNotificationProjector` 增加 `TurnProjectionState`
- [x] 为 `ConversationNotificationProjector` 增加 `ProjectedItemState`
- [x] 在 `ItemStarted` 时建立 item state，并登记到所属 turn
- [x] 在 `ItemDelta` 时把 delta 同步到 item buffer
- [x] 在 `ItemCompleted` 时标记 item 完成并保留最终状态
- [x] 在 `TurnCompleted/Failed/Cancelled` 时清理 turn 级状态

### B. 稳定顺序模型

- [x] 定义 turn 内 item 的稳定排序键
- [x] 定义 reasoning 与 assistant 的归属关系
- [x] 定义 tool 与 assistant/block 的归属关系
- [x] 提供“从 turn state 重建稳定 item 列表”的函数
- [x] 为复杂时序补回归测试

### C. 协议升级

- [x] 设计新的稳定 item/turn 通知类型
- [x] 为 app-server-client 补适配
- [x] 为 CLI reducer 补状态型消费路径
- [x] 删除旧的 transcript started/delta/completed 主链消费路径

### D. CLI 瘦身

- [x] 删除 reasoning 回插补丁路径
- [x] 删除基于 trailing assistant 的推断式排序
- [x] 统一 live 与 rebuild 的顺序来源
- [x] 清理只为旧事件流存在的特殊 flush 逻辑

## 本次提交范围

当前主链已落以下内容：

- 新增本文档，明确最终改造方向与 checklist
- 在 `conversation_notifications.rs` 内引入 reducer 状态骨架并补齐 active turn/item state
- `agent-app-server` 通过 `TurnSnapshot` 对外输出稳定 active turn 视图
- CLI 改为消费 `TurnSnapshot` 重建 transcript
- 删除 CLI 旧的 assistant/reasoning transcript 事件主链与顺序补丁

当前仍保留的实现边界：

- `agent-core` 内仍保留一层最小插入语义，用于 turn 内晚到 item 的基础稳定性
- 目前对 UI 的稳定输出主路径已经切到 `agent-app-server` projector + `TurnSnapshot`
- 若后续要进一步收紧责任边界，可以继续评估是否把 `agent-core` 内这层最小插入语义再下收或弱化

## 完成定义

只有当下面条件都成立时，才能认为“会话顺序已对齐 Codex 的长期方向”：

1. app-server 对外输出的主路径已经是稳定 item/turn 语义，而不是原始并行事件透传。
2. CLI 不再依赖 trailing assistant 回插和类似推断式修复。
3. live 渲染与历史重建共用同一排序来源。
4. reasoning / assistant / tool 的顺序与归属由 reducer 模型显式表达。
5. 相关回归测试覆盖迟到 completed、交错 delta、多 item 同轮并发等场景。
