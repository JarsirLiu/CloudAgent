# 生产级会话状态机重构实施方案

## 目标

本方案用于把当前会话 view 状态机升级成生产级后端状态模型。改完后，后端是会话状态唯一真相，CLI / Web / Node worker 只消费 `ConversationViewSnapshot`，不再各自推导业务状态。

最终模型对齐 Codex 的 `ThreadWatchManager`：

```text
agent-core
  负责 turn 执行、tool call、approval、compaction、model event

agent-app-server::ConversationWatchManager
  负责会话 view 状态真相
  负责 runtime facts
  负责 guard 生命周期
  负责 watch subscribers
  负责 ConversationViewChanged

projection
  只负责 transcript / item / delta notification
  不拥有 conversation view state

server_request
  只负责审批 reply channel
  不拥有 UI/view 状态

CLI / Web / Node
  消费 ConversationViewSnapshot
  映射 display mode / UI
  不恢复业务状态
```

## 当前状态

当前已完成的过渡形态：

```text
ConversationViewHandle
  -> ServerState.apply_conversation_runtime_update(...)
  -> ConversationRuntimeViewManager
  -> ConversationViewChanged
```

当前主要文件：

```text
crates/agent-protocol/src/view_state.rs
crates/agent-app-server/src/session/conversation_runtime.rs
crates/agent-app-server/src/session/view_broadcast.rs
crates/agent-app-server/src/session/listener.rs
crates/agent-app-server/src/session/service.rs
crates/agent-app-server/src/turn/service.rs
crates/agent-app-server/src/server_request/service.rs
crates/agent-app-server/src/routing/command_router.rs
```

当前已有能力：

- `ConversationViewSnapshot`
- `ConversationViewStatus`
- `ConversationActiveFlag`
- `ConversationRuntimeViewManager`
- `ConversationRuntimeUpdate`
- `ConversationViewHandle`
- approval guard: `ConversationRuntimeActiveGuard`

当前主要缺口：

- `ConversationRuntimeViewManager` 仍挂在 `ServerState` 内部。
- `ConversationViewHandle` 仍主动发送 `ConversationViewChanged`。
- `watch::Sender<ConversationViewSnapshot>` 已有，但还不是生产通知驱动核心。
- 业务 service 仍直接构造 `ConversationRuntimeUpdate`。
- `WaitingOnUserInput` 只有状态机和协议支持，缺生产 guard。
- listener 仍会写 conversation view 状态。

## 最终文件结构

完成后建议文件结构：

```text
crates/agent-app-server/src/session/
  conversation_runtime.rs
  conversation_runtime_tests.rs
  conversation_watch.rs
  conversation_watch_tests.rs
  listener.rs
  service.rs
  skills_watch.rs
  state.rs
  subscriptions.rs
  turn_registry.rs

crates/agent-app-server/src/server_request/
  coordinator.rs
  service.rs
  view.rs

crates/agent-app-server/src/turn/
  service.rs
```

需要删除：

```text
crates/agent-app-server/src/session/view_broadcast.rs
crates/agent-app-server/src/session/view_broadcast_tests.rs
```

如果迁移期暂时保留 `view_broadcast.rs`，只能作为 thin wrapper，最终必须删除。

## 最终核心类型

### ConversationWatchManager

新增文件：

```text
crates/agent-app-server/src/session/conversation_watch.rs
```

目标结构：

```rust
use crate::app::notification::send_notification;
use crate::routing::command_router::ServerState;
use crate::session::conversation_runtime::{
    ConversationRuntimeUpdate,
    ConversationRuntimeViewManager,
    ConversationRuntimeWatch,
};
use agent_core::ConversationTurn;
use agent_protocol::{
    AppServerMessage,
    AppServerNotification,
    ConversationViewSnapshot,
    PendingServerRequestView,
    RequestId,
    TurnViewStatus,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, mpsc};

#[derive(Clone)]
pub(crate) struct ConversationWatchManager {
    inner: Arc<Mutex<ConversationWatchState>>,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    server_state: Arc<Mutex<ServerState>>,
}

struct ConversationWatchState {
    runtime: ConversationRuntimeViewManager,
}

pub(crate) struct ConversationRuntimeActiveGuard {
    manager: ConversationWatchManager,
    conversation_id: String,
    guard_type: ConversationRuntimeActiveGuardType,
    released: Arc<AtomicBool>,
    handle: tokio::runtime::Handle,
}

#[derive(Clone)]
enum ConversationRuntimeActiveGuardType {
    ServerRequest { request_id: RequestId },
    UserInput,
}
```

说明：

- `ConversationWatchManager` 是生产入口。
- `ConversationRuntimeViewManager` 可以继续作为纯状态机引擎保留。
- `ServerState` 只用于订阅过滤和其他 session 元数据，不再持有 runtime view。
- `ConversationRuntimeActiveGuard` 负责等待态兜底释放。

### ConversationViewMutation

新增在 `conversation_watch.rs`：

```rust
struct ConversationViewMutation {
    conversation_id: String,
    snapshot: ConversationViewSnapshot,
    changed: bool,
}
```

变化判断不能只看 `updated_at_ms`。

推荐判断：

```rust
fn snapshots_equivalent_for_publish(
    previous: &ConversationViewSnapshot,
    current: &ConversationViewSnapshot,
) -> bool {
    previous.conversation_id == current.conversation_id
        && previous.status == current.status
        && previous.active_turn == current.active_turn
        && same_pending_requests(&previous.pending_requests, &current.pending_requests)
        && previous.message_count == current.message_count
}
```

如果 `PendingServerRequestView` 没有 `PartialEq`，不要为了测试临时乱比较字符串；可以实现稳定比较函数：

```rust
fn same_pending_requests(
    left: &[PendingServerRequestView],
    right: &[PendingServerRequestView],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(left, right)| {
            left.request_id == right.request_id
                && left.conversation_id == right.conversation_id
                && left.turn_id == right.turn_id
                && left.kind == right.kind
                && left.tool_name == right.tool_name
                && left.reason == right.reason
                && left.preview == right.preview
        })
}
```

如果愿意稳定协议类型，也可以给 `PendingServerRequestView` 加 `PartialEq, Eq`，但这属于协议类型变更，需要跑协议相关测试。

## 分阶段实施

### Phase 1：新增 conversation_watch 模块

修改：

```text
crates/agent-app-server/src/session/mod.rs
```

新增：

```rust
pub(crate) mod conversation_watch;
```

新增：

```text
crates/agent-app-server/src/session/conversation_watch.rs
crates/agent-app-server/src/session/conversation_watch_tests.rs
```

第一版 `ConversationWatchManager` 先包装现有 `ConversationRuntimeViewManager`：

```rust
impl ConversationWatchManager {
    pub(crate) fn new(
        event_tx: mpsc::UnboundedSender<AppServerMessage>,
        server_state: Arc<Mutex<ServerState>>,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ConversationWatchState {
                runtime: ConversationRuntimeViewManager::default(),
            })),
            event_tx,
            server_state,
        }
    }

    pub(crate) async fn snapshot(
        &self,
        conversation_id: &str,
    ) -> ConversationViewSnapshot {
        self.inner.lock().await.runtime.snapshot(conversation_id)
    }

    pub(crate) async fn subscribe(
        &self,
        conversation_id: &str,
    ) -> ConversationRuntimeWatch {
        self.inner.lock().await.runtime.subscribe(conversation_id)
    }
}
```

测试：

```text
conversation_watch_tests.rs
  unknown_conversation_is_not_loaded
  note_turn_started_updates_snapshot
  subscriber_receives_latest_snapshot
```

验收：

```text
cargo test -p agent-app-server session::conversation_watch
```

### Phase 2：实现 mutate_and_publish

在 `conversation_watch.rs` 新增：

```rust
impl ConversationWatchManager {
    async fn apply_update(
        &self,
        update: ConversationRuntimeUpdate,
    ) -> ConversationViewMutation {
        let conversation_id = update.conversation_id().to_string();
        let mut state = self.inner.lock().await;
        let previous = state.runtime.snapshot(&conversation_id);
        let snapshot = state.runtime.apply(update);
        let changed = !snapshots_equivalent_for_publish(&previous, &snapshot);
        ConversationViewMutation {
            conversation_id,
            snapshot,
            changed,
        }
    }

    async fn publish_if_changed(&self, mutation: ConversationViewMutation) {
        if !mutation.changed {
            return;
        }
        send_notification(
            &self.event_tx,
            &self.server_state,
            AppServerNotification::ConversationViewChanged {
                conversation_id: mutation.conversation_id,
                snapshot: mutation.snapshot,
            },
        )
        .await;
    }

    pub(crate) async fn apply(&self, update: ConversationRuntimeUpdate) {
        let mutation = self.apply_update(update).await;
        self.publish_if_changed(mutation).await;
    }

    pub(crate) async fn emit_current(&self, conversation_id: &str) {
        let snapshot = self.snapshot(conversation_id).await;
        send_notification(
            &self.event_tx,
            &self.server_state,
            AppServerNotification::ConversationViewChanged {
                conversation_id: conversation_id.to_string(),
                snapshot,
            },
        )
        .await;
    }
}
```

注意：

- `emit_current` 用于显式 replay，例如 `conversation/view` 请求或切换会话后重放。
- `apply` 只在真实状态变化时广播。
- 不能因为 `updated_at_ms` 自增就广播。

测试：

```text
duplicate_request_resolved_does_not_emit_notification
duplicate_mark_loaded_does_not_emit_notification
changed_status_emits_once
emit_current_replays_even_when_unchanged
```

### Phase 3：增加语义 API

在 `ConversationWatchManager` 上新增业务语义方法。业务 service 后续只能调用这些方法，不直接构造 `ConversationRuntimeUpdate`。

新增函数：

```rust
pub(crate) async fn note_loaded(
    &self,
    conversation_id: &str,
    message_count: usize,
) {
    self.apply(ConversationRuntimeUpdate::MarkLoaded {
        conversation_id: conversation_id.to_string(),
    })
    .await;
    self.apply(ConversationRuntimeUpdate::UpdateMessageCount {
        conversation_id: conversation_id.to_string(),
        message_count,
    })
    .await;
}

pub(crate) async fn note_turn_starting(&self, conversation_id: &str) {
    self.apply(ConversationRuntimeUpdate::TurnStarting {
        conversation_id: conversation_id.to_string(),
    })
    .await;
}

pub(crate) async fn note_turn_started(
    &self,
    conversation_id: &str,
    turn_id: String,
) {
    self.apply(ConversationRuntimeUpdate::TurnStarted {
        conversation_id: conversation_id.to_string(),
        turn_id,
    })
    .await;
}

pub(crate) async fn note_active_turn_snapshot(
    &self,
    conversation_id: &str,
    turn: Option<ConversationTurn>,
) {
    self.apply(ConversationRuntimeUpdate::UpdateActiveTurn {
        conversation_id: conversation_id.to_string(),
        turn,
    })
    .await;
}

pub(crate) async fn note_turn_finished(
    &self,
    conversation_id: &str,
    final_status: TurnViewStatus,
) {
    self.apply(ConversationRuntimeUpdate::TurnFinished {
        conversation_id: conversation_id.to_string(),
        final_status,
    })
    .await;
}

pub(crate) async fn note_interrupt_requested(&self, conversation_id: &str) {
    self.apply(ConversationRuntimeUpdate::InterruptRequested {
        conversation_id: conversation_id.to_string(),
    })
    .await;
}

pub(crate) async fn note_compaction_started(&self, conversation_id: &str) {
    self.apply(ConversationRuntimeUpdate::CompactionStarted {
        conversation_id: conversation_id.to_string(),
    })
    .await;
}

pub(crate) async fn note_compaction_finished(&self, conversation_id: &str) {
    self.apply(ConversationRuntimeUpdate::CompactionFinished {
        conversation_id: conversation_id.to_string(),
    })
    .await;
}

pub(crate) async fn note_system_error(
    &self,
    conversation_id: &str,
    message: String,
) {
    self.apply(ConversationRuntimeUpdate::SystemError {
        conversation_id: conversation_id.to_string(),
        message,
    })
    .await;
}
```

验收扫描：

```text
rg -n "ConversationRuntimeUpdate::" crates/agent-app-server/src
```

阶段目标：

```text
允许在 conversation_watch.rs、conversation_runtime.rs、tests 中出现。
业务 service 中后续不应出现。
```

### Phase 4：迁移 ServerState

修改：

```text
crates/agent-app-server/src/routing/command_router.rs
```

删除 imports：

```rust
use crate::session::conversation_runtime::{
    ConversationRuntimeUpdate,
    ConversationRuntimeViewManager,
    ConversationRuntimeWatch,
};
```

改成只在需要时 import `ConversationWatchManager`：

```rust
use crate::session::conversation_watch::ConversationWatchManager;
```

从 `ServerState` 删除字段：

```rust
runtime_view: ConversationRuntimeViewManager,
```

从 `ServerState::new` 删除：

```rust
runtime_view: ConversationRuntimeViewManager::default(),
```

删除方法：

```rust
pub(crate) fn conversation_view_snapshot(...)
pub(crate) fn subscribe_conversation_view(...)
pub(crate) fn apply_conversation_runtime_update(...)
```

新增服务依赖结构：

```rust
#[derive(Clone)]
pub(crate) struct AppSessionServices {
    pub(crate) event_tx: mpsc::UnboundedSender<AppServerMessage>,
    pub(crate) state: Arc<Mutex<ServerState>>,
    pub(crate) view: ConversationWatchManager,
}
```

修改 `handle_command` 签名：

当前：

```rust
pub(crate) async fn handle_command(
    runtime: Arc<AgentHost>,
    command: AppClientCommand,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()>
```

目标：

```rust
pub(crate) async fn handle_command(
    runtime: Arc<AgentHost>,
    command: AppClientCommand,
    services: AppSessionServices,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
) -> Result<()>
```

如果这个改动牵扯太大，可以先不改 `handle_command` 签名，只在调用处构造 manager，并逐步传入 service 层。但本阶段结束时应完成依赖收束。

验收扫描：

```text
rg -n "runtime_view|apply_conversation_runtime_update|conversation_view_snapshot|subscribe_conversation_view" crates/agent-app-server/src/routing/command_router.rs
```

期望：

```text
无命中。
```

### Phase 5：替换 view_broadcast

修改：

```text
crates/agent-app-server/src/session/view_broadcast.rs
```

迁移方式：

1. 把 `ConversationRuntimeActiveGuard` 移到 `conversation_watch.rs`。
2. 把 `ConversationViewHandle::note_server_request_pending` 改成 `ConversationWatchManager::note_server_request_pending`。
3. 删除 `ConversationViewHandle`。
4. 删除 `view_broadcast.rs` 和 `view_broadcast_tests.rs`。
5. 从 `session/mod.rs` 删除：

```rust
pub(crate) mod view_broadcast;
```

验收扫描：

```text
rg -n "view_broadcast|ConversationViewHandle|ConversationViewHandle::new" crates/agent-app-server/src
```

期望：

```text
无生产命中。
```

### Phase 6：approval guard 生产化

在 `conversation_watch.rs` 实现：

```rust
pub(crate) async fn note_server_request_pending(
    &self,
    conversation_id: &str,
    request: PendingServerRequestView,
) -> ConversationRuntimeActiveGuard {
    let request_id = request.request_id.clone();
    self.apply(ConversationRuntimeUpdate::RequestPending {
        conversation_id: conversation_id.to_string(),
        request,
    })
    .await;
    ConversationRuntimeActiveGuard::new(
        self.clone(),
        conversation_id.to_string(),
        ConversationRuntimeActiveGuardType::ServerRequest { request_id },
    )
}
```

实现 guard：

```rust
impl ConversationRuntimeActiveGuard {
    fn new(
        manager: ConversationWatchManager,
        conversation_id: String,
        guard_type: ConversationRuntimeActiveGuardType,
    ) -> Self {
        Self {
            manager,
            conversation_id,
            guard_type,
            released: Arc::new(AtomicBool::new(false)),
            handle: tokio::runtime::Handle::current(),
        }
    }

    pub(crate) async fn release(&self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        self.manager
            .note_active_guard_released(
                self.conversation_id.clone(),
                self.guard_type.clone(),
            )
            .await;
    }
}

impl Drop for ConversationRuntimeActiveGuard {
    fn drop(&mut self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        let manager = self.manager.clone();
        let conversation_id = self.conversation_id.clone();
        let guard_type = self.guard_type.clone();
        self.handle.spawn(async move {
            manager
                .note_active_guard_released(conversation_id, guard_type)
                .await;
        });
    }
}
```

实现释放：

```rust
impl ConversationWatchManager {
    async fn note_active_guard_released(
        &self,
        conversation_id: String,
        guard_type: ConversationRuntimeActiveGuardType,
    ) {
        match guard_type {
            ConversationRuntimeActiveGuardType::ServerRequest { request_id } => {
                self.apply(ConversationRuntimeUpdate::RequestResolved {
                    conversation_id,
                    request_id,
                })
                .await;
            }
            ConversationRuntimeActiveGuardType::UserInput => {
                self.apply(ConversationRuntimeUpdate::UserInputResolved {
                    conversation_id,
                })
                .await;
            }
        }
    }
}
```

注意：`UserInputResolved` 需要 Phase 7 新增。

测试：

```text
approval_guard_drop_clears_waiting_on_approval
approval_guard_release_is_idempotent
approval_guard_cancelled_future_clears_waiting_on_approval
duplicate_request_resolved_does_not_rebroadcast
```

### Phase 7：WaitingOnUserInput 状态和 guard

修改：

```text
crates/agent-app-server/src/session/conversation_runtime.rs
```

修改 `ConversationRuntimeUpdate`：

```rust
pub(crate) enum ConversationRuntimeUpdate {
    ...
    UserInputRequested {
        conversation_id: String,
    },
    UserInputResolved {
        conversation_id: String,
    },
}
```

修改 `conversation_id()` match，加入两个 variant。

修改 facts：

当前：

```rust
waiting_on_user_input: bool,
```

目标：

```rust
waiting_on_user_input_count: u32,
```

修改初始化：

```rust
waiting_on_user_input_count: 0,
```

修改状态投影：

```rust
if self.waiting_on_user_input_count > 0 {
    flags.push(ConversationActiveFlag::WaitingOnUserInput);
}
```

新增方法：

```rust
pub(crate) fn user_input_requested(&mut self, conversation_id: &str) {
    let facts = self.facts_mut(conversation_id);
    facts.loaded = true;
    facts.running = true;
    facts.waiting_on_user_input_count =
        facts.waiting_on_user_input_count.saturating_add(1);
    facts.updated_at_ms = next_updated_at_ms();
}

pub(crate) fn user_input_resolved(&mut self, conversation_id: &str) {
    let facts = self.facts_mut(conversation_id);
    let previous = facts.waiting_on_user_input_count;
    facts.waiting_on_user_input_count =
        facts.waiting_on_user_input_count.saturating_sub(1);
    if facts.waiting_on_user_input_count != previous {
        facts.updated_at_ms = next_updated_at_ms();
    }
}
```

修改原测试 helper：

删除或改造：

```rust
waiting_on_user_input(...)
resumed_from_user_input(...)
```

改成通过 update 或新方法测试。

在 `ConversationWatchManager` 新增：

```rust
pub(crate) async fn note_user_input_requested(
    &self,
    conversation_id: &str,
) -> ConversationRuntimeActiveGuard {
    self.apply(ConversationRuntimeUpdate::UserInputRequested {
        conversation_id: conversation_id.to_string(),
    })
    .await;
    ConversationRuntimeActiveGuard::new(
        self.clone(),
        conversation_id.to_string(),
        ConversationRuntimeActiveGuardType::UserInput,
    )
}
```

生产接入原则：

```text
没有真实 core 事件源时，不在 turn/service 中伪造 WaitingOnUserInput。
先提供 manager API 和测试。
等 core 有 UserInputRequested / UserInputResolved 事件后再接入。
```

测试：

```text
user_input_guard_sets_waiting_on_user_input
user_input_guard_drop_clears_waiting_on_user_input
user_input_guard_nested_requests_use_count
duplicate_user_input_resolved_does_not_rebroadcast
```

### Phase 8：迁移 turn/service.rs

修改文件：

```text
crates/agent-app-server/src/turn/service.rs
```

当前 import：

```rust
use crate::session::conversation_runtime::ConversationRuntimeUpdate;
use crate::session::view_broadcast::ConversationViewHandle;
```

目标：

```rust
use crate::session::conversation_watch::ConversationWatchManager;
```

修改函数签名。

当前：

```rust
pub(crate) async fn submit_turn(
    runtime: Arc<AgentHost>,
    event_tx: &mpsc::UnboundedSender<AppServerMessage>,
    state: &Arc<Mutex<ServerState>>,
    ...
)
```

目标：

```rust
pub(crate) async fn submit_turn(
    runtime: Arc<AgentHost>,
    services: AppSessionServices,
    conversation_id: String,
    content: Vec<InputItem>,
    permission_profile: PermissionProfile,
    approval_policy: ApprovalPolicy,
    auto_approve: bool,
    auto_approve_reason: Option<String>,
)
```

如果暂时不引入 `AppSessionServices`，最小目标是给函数增加：

```rust
view: ConversationWatchManager
```

替换点：

```rust
view.apply(ConversationRuntimeUpdate::InterruptRequested { ... })
```

改为：

```rust
view.note_interrupt_requested(&conversation_id).await;
```

```rust
view.apply(ConversationRuntimeUpdate::CompactionStarted { ... })
```

改为：

```rust
view.note_compaction_started(&conversation_id).await;
```

```rust
view.apply(ConversationRuntimeUpdate::CompactionFinished { ... })
```

改为：

```rust
view.note_compaction_finished(&conversation_id).await;
```

```rust
view.apply(ConversationRuntimeUpdate::TurnStarting { ... })
```

改为：

```rust
view.note_turn_starting(&conversation_id).await;
```

```rust
view.note_server_request_pending(...)
```

改为 manager 方法：

```rust
let request_guard = view
    .note_server_request_pending(
        &conversation_id,
        pending_request_view(&conversation_id, request_id.clone(), &request, 0),
    )
    .await;
```

```rust
view.apply(ConversationRuntimeUpdate::TurnFinished { ... })
```

改为：

```rust
view.note_turn_finished(&conversation_id, turn_view_status).await;
```

```rust
view.apply(ConversationRuntimeUpdate::SystemError { ... })
```

改为：

```rust
view.note_system_error(&conversation_id, message.clone()).await;
```

修改 `start_conversation_listener` 调用：

当前：

```rust
start_conversation_listener(conversation_id.clone(), view.clone())
```

目标 Phase 8 可暂时：

```rust
start_conversation_listener(conversation_id.clone(), view.clone())
```

Phase 10 再移除 listener 对 view 的依赖。

验收扫描：

```text
rg -n "ConversationRuntimeUpdate::|ConversationViewHandle|ConversationViewHandle::new" crates/agent-app-server/src/turn/service.rs
```

期望：

```text
无命中。
```

### Phase 9：迁移 server_request/service.rs

修改文件：

```text
crates/agent-app-server/src/server_request/service.rs
```

当前 import：

```rust
use crate::session::conversation_runtime::ConversationRuntimeUpdate;
use crate::session::view_broadcast::ConversationViewHandle;
```

目标：

```rust
use crate::session::conversation_watch::ConversationWatchManager;
```

修改函数签名，增加：

```rust
view: &ConversationWatchManager
```

或使用：

```rust
services: &AppSessionServices
```

替换：

```rust
view.apply(ConversationRuntimeUpdate::RequestResolved {
    conversation_id: resolved.conversation_id.clone(),
    request_id: request_id.clone(),
})
.await;
```

为：

```rust
view.note_server_request_resolved(
    &resolved.conversation_id,
    request_id.clone(),
)
.await;
```

需要在 manager 新增：

```rust
pub(crate) async fn note_server_request_resolved(
    &self,
    conversation_id: &str,
    request_id: RequestId,
) {
    self.apply(ConversationRuntimeUpdate::RequestResolved {
        conversation_id: conversation_id.to_string(),
        request_id,
    })
    .await;
}
```

验收：

```text
rg -n "ConversationRuntimeUpdate::|ConversationViewHandle" crates/agent-app-server/src/server_request/service.rs
```

期望：

```text
无命中。
```

### Phase 10：迁移 session/service.rs

修改文件：

```text
crates/agent-app-server/src/session/service.rs
```

当前 import：

```rust
use crate::session::conversation_runtime::ConversationRuntimeUpdate;
use crate::session::view_broadcast::ConversationViewHandle;
```

目标：

```rust
use crate::session::conversation_watch::ConversationWatchManager;
```

修改：

```rust
request_conversation_view(...)
```

当前：

```rust
hydrate_conversation_view(runtime, state, &conversation_id).await?;
ConversationViewHandle::new(event_tx.clone(), state.clone())
    .emit_current(&conversation_id)
    .await;
```

目标：

```rust
hydrate_conversation_view(runtime, view, &conversation_id).await?;
view.emit_current(&conversation_id).await;
```

修改：

```rust
conversation_view_snapshot(...)
```

当前从 `ServerState` 读：

```rust
state.conversation_view_snapshot(conversation_id)
```

目标：

```rust
view.snapshot(conversation_id).await
```

修改：

```rust
hydrate_conversation_view(...)
```

当前：

```rust
state.apply_conversation_runtime_update(ConversationRuntimeUpdate::MarkLoaded { ... });
state.apply_conversation_runtime_update(ConversationRuntimeUpdate::UpdateMessageCount { ... });
```

目标：

```rust
view.note_loaded(conversation_id, message_count).await;
```

修改：

```rust
publish_switched_conversation_state(...)
```

当前 replay view 通过 `ConversationViewHandle::new(...)`。

目标：

```rust
hydrate_conversation_view(runtime, view, conversation_id).await?;
view.emit_current(conversation_id).await;
```

验收：

```text
rg -n "ConversationRuntimeUpdate::|ConversationViewHandle|apply_conversation_runtime_update|conversation_view_snapshot" crates/agent-app-server/src/session/service.rs
```

期望：

```text
无命中。
```

### Phase 11：listener 降级为纯投影器

修改文件：

```text
crates/agent-app-server/src/session/listener.rs
```

当前 listener 仍依赖：

```rust
use crate::session::conversation_runtime::ConversationRuntimeUpdate;
use crate::session::view_broadcast::ConversationViewHandle;
```

目标：

删除这两个依赖。

修改 `start_conversation_listener` 签名。

当前：

```rust
pub(crate) fn start_conversation_listener(
    conversation_id: String,
    view: ConversationViewHandle,
) -> (ConversationListenerHandle, JoinHandle<()>)
```

目标：

```rust
pub(crate) fn start_conversation_listener(
    conversation_id: String,
    event_tx: mpsc::UnboundedSender<AppServerMessage>,
    state: Arc<Mutex<ServerState>>,
) -> (ConversationListenerHandle, JoinHandle<()>)
```

说明：

- listener 需要 `event_tx + state` 发送 transcript notifications。
- listener 不再需要 view manager。

删除 `ProjectEvent` 中这些逻辑：

```rust
EventMsg::TurnStarted { .. } => view.apply(...)
EventMsg::ContextCompactionStarted { .. } => view.apply(...)
EventMsg::ContextCompacted { .. } => view.apply(...)
view.apply(ConversationRuntimeUpdate::UpdateActiveTurn { ... })
```

保留：

```rust
let notifications = projector.project_turn_event(&event);
for notification in notifications {
    send_notification(&event_tx, &state, notification).await;
}
```

active turn snapshot 的更新迁移到 `turn/service.rs` 的 event callback：

当前 callback：

```rust
move |event| {
    let event = event.clone();
    ...
    listener_for_events.project_event(event);
}
```

目标：

```rust
move |event| {
    let event = event.clone();
    let view = view_for_events.clone();
    let conversation_id = conversation_id_for_events.clone();
    let listener = listener_for_events.clone();

    tokio::spawn(async move {
        match &event {
            EventMsg::TurnStarted { turn_id, .. } => {
                view.note_turn_started(&conversation_id, turn_id.clone()).await;
            }
            EventMsg::ContextCompactionStarted { .. } => {
                view.note_compaction_started(&conversation_id).await;
            }
            EventMsg::ContextCompacted { .. } => {
                view.note_compaction_finished(&conversation_id).await;
            }
            _ => {}
        }
        listener.project_event(event);
        if let Some(active_turn) = listener.active_turn_snapshot().await {
            view.note_active_turn_snapshot(&conversation_id, Some(active_turn)).await;
        }
    });
}
```

注意：

- 如果 callback 不允许 `tokio::spawn`，可以保持 listener 内部提供 active snapshot，然后由 listener 返回事件给 service；但最终原则是 listener 不直接写 view。
- 不要在 listener 内部 import `ConversationRuntimeUpdate`。

验收扫描：

```text
rg -n "ConversationRuntimeUpdate|ConversationViewHandle|ConversationWatchManager|note_.*turn|note_.*compaction" crates/agent-app-server/src/session/listener.rs
```

期望：

```text
无命中。
```

### Phase 12：命令路由注入 manager

需要找到 app-server 初始化 `ServerState` 和 `event_tx` 的地方。

搜索：

```text
rg -n "ServerState::new|handle_command\\(" crates/agent-app-server/src
```

在创建 `ServerState` 后创建：

```rust
let state = Arc::new(Mutex::new(ServerState::new(...)));
let view = ConversationWatchManager::new(event_tx.clone(), state.clone());
let services = AppSessionServices {
    event_tx: event_tx.clone(),
    state: state.clone(),
    view,
};
```

所有 command 处理改为传：

```rust
services.clone()
```

而不是反复传：

```rust
event_tx, state
```

修改：

```text
crates/agent-app-server/src/routing/command_router.rs
crates/agent-app-server/src/app/in_process.rs
crates/agent-app-server/src/transport/stdio.rs
```

具体以搜索结果为准。

验收：

```text
rg -n "handle_command\\(" crates/agent-app-server/src
```

每个调用点都传入 `AppSessionServices` 或等价依赖对象。

### Phase 13：协议和客户端确认

协议文件：

```text
crates/agent-protocol/src/view_state.rs
crates/agent-protocol/src/messages.rs
crates/agent-protocol/src/wire.rs
```

确认：

- `ConversationViewSnapshot` 字段足够 CLI/Web 使用。
- `ConversationViewChanged` 是唯一 view 状态通知。
- `conversation/view` 是唯一主动查询入口。
- 不恢复 `conversation/status`。

客户端文件：

```text
cli/src/app/core/conversation_state.rs
cli/src/state/reducer.rs
apps/node/src/node/session_state.rs
```

确认：

- CLI 只用 `ConversationViewSnapshot` 映射 `FrontendMode`。
- Node 只转发订阅 conversation 的 `ConversationViewChanged`。
- Web 未来可直接消费相同 snapshot。

验收扫描：

```text
rg -n "FrontendStateChanged|ConversationStatusResponse|RequestConversationStatus|conversation/status|AppServerNotification::ConversationStatus" crates cli apps
```

期望：

```text
无生产命中。
```

## 测试计划

### conversation_runtime_tests.rs

保留纯状态机测试：

```text
unknown_conversation_is_not_loaded
loaded_conversation_is_idle
turn_starting_projects_active_without_turn_id
turn_started_sets_real_turn_id
pending_request_sets_waiting_on_approval_until_resolved
resolving_missing_request_is_idempotent
waiting_on_user_input_count_projects_flag
terminal_turn_clears_active_flags_and_pending_requests
system_error_projects_terminal_error_status
watch_subscriber_receives_latest_snapshot_and_updates
```

### conversation_watch_tests.rs

新增 manager 级测试：

```text
apply_changed_status_emits_conversation_view_changed
duplicate_request_resolved_does_not_emit_notification
emit_current_replays_snapshot_even_when_unchanged
approval_guard_drop_clears_waiting_on_approval
approval_guard_release_is_idempotent
approval_guard_cancelled_future_clears_waiting_on_approval
user_input_guard_sets_and_clears_waiting_on_user_input
watch_subscriber_only_receives_own_conversation
latest_snapshot_updates_without_receivers
```

### server_request coordinator tests

确认一次性 resolve：

```text
resolve_consumes_pending_request_once
second_resolve_returns_none
pending_for_conversation_is_stably_ordered_by_request_id
```

### listener tests

确认 listener 不产生 view 状态：

```text
listener_projects_transcript_notifications_only
listener_does_not_emit_conversation_view_changed
```

### CLI / Node tests

保留已有测试，并新增或确认：

```text
cli maps WaitingOnApproval to WaitingForServerRequest
cli maps RunningTurn to Running
cli does not use pending_submitted_input to recover Running
node forwards ConversationViewChanged only for subscribed conversations
```

## 每阶段验证命令

每个阶段至少跑：

```text
cargo check -p agent-app-server
```

涉及协议或客户端时跑：

```text
cargo check -p agent-protocol -p agent-app-server -p agent-gateway -p cli -p node
```

状态机或 manager 改动跑：

```text
cargo test -p agent-app-server
```

最终跑：

```text
cargo test -p agent-app-server
cargo test -p cli
cargo test -p node
```

## 最终验收扫描

旧协议 / 旧前端状态：

```text
rg -n "FrontendStateChanged|ConversationStatusResponse|RequestConversationStatus|conversation/status|request_conversation_status|update_conversation_status|update_frontend_mode|AppServerNotification::ConversationStatus" crates cli apps
```

期望：

```text
无生产命中。
```

绕过 manager：

```text
rg -n "ConversationViewHandle::new|ConversationViewHandle|view_broadcast|apply_conversation_runtime_update|conversation_view_snapshot|subscribe_conversation_view" crates/agent-app-server/src
```

期望：

```text
无生产命中。
```

业务层直接构造 runtime update：

```text
rg -n "ConversationRuntimeUpdate::" crates/agent-app-server/src/turn crates/agent-app-server/src/server_request crates/agent-app-server/src/session/service.rs crates/agent-app-server/src/session/listener.rs
```

期望：

```text
无生产命中。
```

listener 写 view 状态：

```text
rg -n "ConversationRuntimeUpdate|ConversationWatchManager|note_.*turn|note_.*compaction" crates/agent-app-server/src/session/listener.rs
```

期望：

```text
无生产命中。
```

fake turn id：

```text
rg -n "\"manual_compaction\"|fake turn|placeholder turn|dummy turn" crates cli apps
```

期望：

```text
无生产 fake turn id。
```

前端业务推导：

```text
rg -n "pending_submitted_input.*FrontendMode|sync_frontend_mode|interrupt_requested|has_pending_submission\\(\\).*Running" cli/src
```

期望：

```text
CLI 不根据本地状态恢复业务模式。
CLI 只把 ConversationViewSnapshot 映射为显示模式。
```

## 推荐提交拆分

1. `app-server: add conversation watch manager`
2. `app-server: add mutate and publish for conversation view`
3. `app-server: add semantic conversation watch APIs`
4. `app-server: move runtime view out of server state`
5. `app-server: remove view broadcast handle`
6. `app-server: harden approval guard lifecycle`
7. `app-server: add user input guard support`
8. `app-server: migrate turn service to watch manager`
9. `app-server: migrate server request service to watch manager`
10. `app-server: migrate session service to watch manager`
11. `app-server: make listener transcript-only`
12. `app-server: inject session services through command router`
13. `tests: cover production conversation watch lifecycle`
14. `docs: finalize conversation state machine architecture`

## 完成定义

改完后必须满足：

- 后端状态机是唯一业务状态真相。
- `ConversationWatchManager` 是 view 状态唯一生产入口。
- `ServerState` 不再持有 runtime view。
- `ConversationViewChanged` 只从 manager 的 publish 路径发出。
- 状态未变化时不广播重复事件。
- approval / user input 等等待态有 guard 兜底释放。
- watcher 可随时订阅当前最新 snapshot。
- listener 不写 view 状态。
- projector 不拥有 view 状态。
- CLI / Web / Node 只消费 `ConversationViewSnapshot`。
- 中断、错误、审批取消、turn 失败不会留下卡死状态。
- 测试覆盖正常流、异常流、重复事件、订阅隔离。
