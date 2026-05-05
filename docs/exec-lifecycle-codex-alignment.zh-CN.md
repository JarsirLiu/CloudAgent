# CloudAgent 调用级生命周期对齐 Codex 方案

## 背景

当前 `cloudagent` 的 CLI 历史区与运行态交互，已经在前端侧初步对齐到 Codex 的方向：

- control 事件已经收敛到统一入口
- CLI 已经有统一的 `ActiveExecSession`
- exploration 与 command 已经能共用一个运行态容器

但目前整个系统仍然存在一个根本限制：

- 实时通知链路只暴露 `item_id`
- 没有独立的调用级 `call_id`

这意味着 CLI 只能把 `item_id` 当作运行态路由键近似使用，无法完全复用 Codex 那种更严格的：

- begin / delta / completed 三段同 key 路由
- 多 call 容器精确匹配
- orphan completion routing
- 更稳的并行调用聚合

如果继续在现有 `item_id` 方案上补逻辑，后续一旦后端支持真正的调用级标识，仍然会面临二次重构。

因此，本方案的目标不是“继续加一点前端技巧”，而是直接建立一套可长期演进、与 Codex 同方向的调用级生命周期架构。

## 目标

目标拆成四层：

1. 协议层：前端通知支持稳定的调用级 `call_id`
2. 投影层：app-server 把 core 的调用级生命周期显式透传给 CLI
3. CLI 状态机层：统一用调用级 route key 驱动 `ActiveExecSession`
4. 展示层：history/live cell 只消费 session，不再推断调用关系

最终效果：

- `item_id` 降级为 transcript item 标识
- `call_id` 成为运行态生命周期主键
- exploration 只是 exec session 的一种分组模式
- command / exploration / tool / file change 共享同一套生命周期骨架

## 非目标

本方案不追求一次完成所有 UI 表现细节。

不是本阶段重点的内容：

- 立即复刻 Codex 的全部动画/微交互
- 一次性补齐所有 tool source 类型
- 在后端协议未就绪前，靠 CLI 猜测并行调用关系

本方案优先保证：

- 分层清晰
- 协议稳定
- 后续演进不推翻框架

## 当前现状

### 协议层

当前实时通知只有 `item_id`，没有独立 `call_id`。

相关位置：

- [crates/agent-protocol/src/messages.rs](/D:/learn/gifti/cloudagent/crates/agent-protocol/src/messages.rs:103)
- [crates/agent-app-server/src/projection/conversation_notifications.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server/src/projection/conversation_notifications.rs:30)

受影响的通知包括：

- `ItemStarted`
- `CommandExecutionOutputDelta`
- `ToolOutputDelta`
- `FileChangeOutputDelta`
- `ItemCompleted`

### app-server 投影层

当前 app-server 主要按 `item_id` 跟踪 active item 生命周期：

- item started 时注册 `item_id`
- delta 时按 `item_id` 校验
- completed 时按 `item_id` 回收

这让前端缺少一个真正的调用级路由键。

### CLI 层

CLI 侧已经做了正确的架构预铺：

- `ControlDispatch` 已经是统一入口
- `ActiveExecSession` 已经是统一运行态容器
- `ActiveExecSession` 内已经是多 call 容器

相关位置：

- [cli/src/state/reducer.rs](/D:/learn/gifti/cloudagent/cli/src/state/reducer.rs:8)
- [cli/src/state/mod.rs](/D:/learn/gifti/cloudagent/cli/src/state/mod.rs:42)
- [cli/src/app/conversation/items.rs](/D:/learn/gifti/cloudagent/cli/src/app/conversation/items.rs:1)

但它目前仍然只能使用 `item_id` 作为 route key。

## 对齐 Codex 的目标架构

### 分层原则

参考 Codex，长期正确的职责边界应为：

1. `agent-core`
   负责生成稳定的调用级 lifecycle 标识

2. `agent-app-server`
   负责把调用级 lifecycle 投影成稳定的前端通知

3. `agent-protocol`
   负责定义 begin / delta / completed 的公共字段结构

4. `cli`
   负责消费调用级通知，驱动 `ActiveExecSession`

### 关键原则

1. `item_id` 不是运行态主键
2. `call_id` 才是调用级主键
3. begin / delta / completed 三段必须共享同一个 `call_id`
4. CLI 不应自行猜测调用关系
5. exploration 不应是一套独立架构，而应是一种 grouping policy

## 推荐最终数据模型

### 协议层通知

建议为以下通知统一增加：

```rust
call_id: Option<String>
```

涉及通知：

- `ItemStarted`
- `CommandExecutionOutputDelta`
- `ToolOutputDelta`
- `FileChangeOutputDelta`
- `ItemCompleted`

推荐结构示意：

```rust
ItemStarted {
    conversation_id: String,
    turn_id: TurnId,
    item_id: String,
    call_id: Option<String>,
    kind: TurnItemKind,
    title: Option<String>,
}
```

```rust
CommandExecutionOutputDelta {
    conversation_id: String,
    turn_id: TurnId,
    item_id: String,
    call_id: Option<String>,
    delta: String,
}
```

`ToolOutputDelta`、`FileChangeOutputDelta`、`ItemCompleted` 同理。

### app-server 内部生命周期索引

建议不要继续只保存 `item_id -> turn_id`，而是引入完整生命周期对象：

```rust
struct ActiveLifecycle {
    turn_id: String,
    item_id: String,
    call_id: Option<String>,
}
```

并维护至少两层索引：

```rust
active_items_by_item_id: HashMap<String, ActiveLifecycle>
active_item_id_by_call_id: HashMap<String, String>
```

### CLI route key

CLI 不应把 `item_id` 写死成最终键。

建议统一 route key：

```rust
enum ActiveExecRouteKey {
    CallId(String),
    ItemId(String),
}
```

过渡期策略：

- 通知里有 `call_id` 时优先使用 `CallId`
- 没有 `call_id` 时回退到 `ItemId`

这样后端协议升级后，CLI 只需切换 route key 来源，不需要重写状态机。

## 逐文件改造清单

### 1. `crates/agent-protocol/src/messages.rs`

文件：

- [crates/agent-protocol/src/messages.rs](/D:/learn/gifti/cloudagent/crates/agent-protocol/src/messages.rs:103)

需要修改：

1. `AppServerNotification::ItemStarted`
2. `AppServerNotification::CommandExecutionOutputDelta`
3. `AppServerNotification::ToolOutputDelta`
4. `AppServerNotification::FileChangeOutputDelta`
5. `AppServerNotification::ItemCompleted`

改造内容：

- 增加 `call_id: Option<String>`

设计要求：

- 兼容阶段允许 `None`
- 最终目标是 begin / delta / completed 都带稳定 `Some(call_id)`

### 2. `crates/agent-protocol/src/wire.rs`

文件：

- [crates/agent-protocol/src/wire.rs](/D:/learn/gifti/cloudagent/crates/agent-protocol/src/wire.rs:289)

需要修改：

1. 所有通知 roundtrip 测试
2. 任何断言 notification JSON shape 的测试

建议新增测试：

- `item_started_roundtrips_with_call_id`
- `command_output_delta_roundtrips_with_call_id`
- `item_completed_roundtrips_with_call_id`
- `notifications_allow_missing_call_id`

目标：

- 序列化支持 `call_id = Some(...)`
- 兼容 `call_id = None`

### 3. `crates/agent-app-server/src/projection/conversation_notifications.rs`

文件：

- [crates/agent-app-server/src/projection/conversation_notifications.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server/src/projection/conversation_notifications.rs:30)

这是本次后端改造的主战场。

#### 需要新增的内部结构

```rust
#[derive(Clone, Debug)]
struct ActiveLifecycle {
    turn_id: String,
    item_id: String,
    call_id: Option<String>,
}
```

#### 需要调整的 projector 状态

从：

```rust
active_items: HashMap<String, String>
```

升级到：

```rust
active_items_by_item_id: HashMap<String, ActiveLifecycle>
active_item_id_by_call_id: HashMap<String, String>
```

#### 建议新增的辅助函数

```rust
fn register_active_item(&mut self, lifecycle: ActiveLifecycle)
fn remove_active_item(&mut self, item_id: &str) -> Option<ActiveLifecycle>
fn call_id_for_item(&self, item_id: &str) -> Option<&str>
fn validate_active_item(&self, turn_id: &str, item_id: &str) -> Option<AppServerNotification>
```

可选预留：

```rust
fn validate_active_call(&self, turn_id: &str, call_id: &str) -> Option<AppServerNotification>
```

#### `EventMsg::ItemStarted` 分支

需要改造：

1. 从 core event 中取 `call_id`
2. 注册完整 `ActiveLifecycle`
3. 通知显式带 `call_id`

目标：

- started 事件进入 projector 时，调用级标识就已经可见

#### `EventMsg::ItemDelta` 分支

建议暂时保留现有 `item_id` 校验逻辑，但：

1. 构建通知时，通过 `item_id` 查 lifecycle
2. 把 `call_id` 一并透传

适用于：

- `CommandExecutionOutputDelta`
- `ToolOutputDelta`
- `FileChangeOutputDelta`

#### `CoreTranscriptEvent::ItemCompleted` 分支

需要改造：

1. 回收完整 lifecycle，而不只是删 `item_id`
2. 同时清理 `call_id` 索引
3. 通知显式带 `call_id`

### 4. `crates/agent-app-server` 测试

相关测试位置：

- [crates/agent-app-server/src/projection/conversation_notifications.rs](/D:/learn/gifti/cloudagent/crates/agent-app-server/src/projection/conversation_notifications.rs:371)

建议新增/修改测试覆盖：

1. `started -> delta -> completed` 三段 `call_id` 一致
2. `call_id = None` 兼容路径仍然工作
3. `remove_active_item` 会同步清理 `call_id` 索引
4. turn mismatch / lifecycle mismatch 不退化

### 5. `crates/agent-core`

关键结论：

- `call_id` 不能在 app-server 临时生成
- 必须来自 core/tool execution 生命周期

需要重点搜索：

- `EventMsg::ItemStarted`
- `EventMsg::ItemDelta`
- `EventMsg::ItemCompleted`
- `ToolCall`
- `ToolResult`
- `ToolOutputDelta`
- `TranscriptItem::CommandExecution`
- `TranscriptItem::ToolResult`

重点文件建议从这里开始：

- [crates/agent-core/src/projection/transcript.rs](/D:/learn/gifti/cloudagent/crates/agent-core/src/projection/transcript.rs:318)

目标：

1. 找到同一次调用在 core 内最早可见的稳定标识
2. 让该标识贯穿：
   - started
   - delta
   - completed

如果 core 已有稳定 tool call id：

- 直接透传到 `EventMsg::*`

如果 core 还没有：

- 在 tool execution 生命周期创建处补一个稳定 id
- 不要在 transcript projection 或 app-server 层临时拼接

### 6. `cli/src/state/reducer.rs`

文件：

- [cli/src/state/reducer.rs](/D:/learn/gifti/cloudagent/cli/src/state/reducer.rs:8)

需要修改：

- `ControlDispatch` 增加 route key 信息

建议引入：

```rust
enum ControlRouteKey {
    CallId(String),
    ItemId(String),
}
```

生成规则：

- 通知 `call_id` 有值时，生成 `CallId`
- 否则回退 `ItemId`

### 7. `cli/src/state/mod.rs`

文件：

- [cli/src/state/mod.rs](/D:/learn/gifti/cloudagent/cli/src/state/mod.rs:42)

当前 CLI 已经有 `ActiveExecSession`，这是正确方向。

下一步应做：

- 把 `ActiveExecCall` 的 route key 从隐式 `item_id` 升级为 `ActiveExecRouteKey`

建议：

```rust
enum ActiveExecRouteKey {
    CallId(String),
    ItemId(String),
}
```

### 8. `cli/src/app/conversation/items.rs`

文件：

- [cli/src/app/conversation/items.rs](/D:/learn/gifti/cloudagent/cli/src/app/conversation/items.rs:1)

当前该文件已经有统一的 `ActiveExecSession` 驱动逻辑。

等后端协议与 reducer 改完后，这里应做的只是路由键切换：

- `append_delta`
- `complete_call`
- `contains_call`

从 `item_id` 改为 `route_key`

这一步应是机械替换，而不是重构状态机。

## 推荐实施顺序

为了减少返工，建议严格按下面顺序推进：

1. 协议层
   - `agent-protocol/messages.rs`
   - `agent-protocol/wire.rs`

2. app-server 投影层
   - `conversation_notifications.rs`
   - projector tests

3. core 事件来源
   - 找到并透传真实 `call_id`

4. CLI 路由键切换
   - reducer
   - `ActiveExecRouteKey`
   - session route matching

## 兼容策略

过渡期不应强制一次性完成所有层的改造。

允许的阶段性兼容策略：

1. 协议先支持 `call_id: Option<String>`
2. app-server 先透传 `None`
3. core 优先给 command execution 路径补真实 `call_id`
4. CLI 支持：
   - 有 `call_id` 时走 `CallId`
   - 否则回退 `ItemId`

这样是最终架构下的渐进落地，不是需要未来推翻的中间态。

## 不建议采用的方案

以下方案会导致未来二次返工，不建议采用：

1. 只在 `ItemCompleted.item` 内隐含 `call_id`
   - delta 路由仍然无法解决

2. app-server 用 `item_id + turn_id` 拼一个伪 `call_id`
   - 只是换名字，不是真正的调用级标识

3. CLI 自己猜测调用归属
   - 会继续把协议层问题推给前端

4. 继续把 exploration 当成一套特殊前端机制
   - 长期应只是 exec grouping policy

## 里程碑建议

### M1：协议与投影层定型

完成后应达到：

- 通知结构支持 `call_id`
- app-server 可为所有 begin / delta / completed 附带 `call_id: Option<String>`

### M2：core 输出真实 `call_id`

完成后应达到：

- command execution 全链路带稳定 `call_id`
- CLI 可开始使用 `CallId` 路由

### M3：CLI 全量切换调用级路由

完成后应达到：

- `ActiveExecSession` 主要依赖 `CallId`
- `ItemId` 仅为兼容 fallback

### M4：补齐 Codex 风格高级行为

包括但不限于：

- orphan completion routing
- 更精确的 exploring grouping
- 更复杂 tool source 合流
- 更稳的并行 call 路由

## 结论

这次后端改造看起来改动面确实不小，但它属于“明确最终接口，再分阶段落地”的那种大改，而不是会导致未来返工的中间态。

只要遵守以下三条原则，就能保证后续演进平滑：

1. `call_id` 必须成为调用级主键
2. begin / delta / completed 必须共享同一个 `call_id`
3. CLI 只消费稳定协议，不自行推断调用关系

在此基础上，前端现有已经搭好的 `ActiveExecSession` 架构可以直接承接，不需要推翻。
