# Token Usage 与自动压缩窗口对齐 Codex 的实施方案

## 背景

这份文档只覆盖两件事：

1. 会话级 token usage 状态，用服务端 usage 校准。
2. compaction window baseline，支持 `Total` / `BodyAfterPrefix` scope，避免压缩后重复触发。

它不替代 `docs/auto-compaction-codex-alignment.zh-CN.md`。那份文档关注 compaction checkpoint、history reconstruction、turn abort、tool loop 等更大的压缩生命周期；本方案关注自动压缩触发条件和 token 账本。

## Codex 怎么做

Codex 把状态拆成两层。

第一层是会话级 usage 账本：

```text
TokenUsageInfo
  total_token_usage       // 整个 session 累计消耗
  last_token_usage        // 最近一次模型响应 usage
  model_context_window    // 当前模型窗口
```

对应参考文件：

```text
D:\learn\AIbac\JiangFang\codex\codex-rs\protocol\src\protocol.rs
D:\learn\AIbac\JiangFang\codex\codex-rs\core\src\context_manager\history.rs
D:\learn\AIbac\JiangFang\codex\codex-rs\core\src\state\session.rs
```

`TokenUsageInfo::new_or_append` 的语义是：有新的服务端 usage 时，把它 append 到 `total_token_usage`，并把它设为 `last_token_usage`。压缩不会清空这个 session total。

第二层是 compaction window：

```text
AutoCompactWindow
  ordinal
  prefill_input_tokens: Estimated | ServerObserved
```

对应参考文件：

```text
D:\learn\AIbac\JiangFang\codex\codex-rs\core\src\state\auto_compact_window.rs
```

它只服务于 `BodyAfterPrefix` scope：压缩后先记录一个 estimated baseline，后续拿到服务端 usage 后用 server observed input tokens 替换 estimated baseline。这样压缩后的“前缀/基座”不会继续被算作新增 body，避免刚压缩完下一轮又马上触发。

Codex 的阈值逻辑：

```text
derived_limit = model_context_window * 0.9
auto_compact_limit = min(configured_auto_compact_limit, derived_limit)
```

如果没有显式配置：

```text
auto_compact_limit = model_context_window * 0.9
```

触发判断：

```text
active_context_tokens >= auto_compact_limit
```

scope 语义：

```text
Total:
  scope_tokens = active_context_tokens

BodyAfterPrefix:
  baseline = auto_compact_window.prefill_input_tokens.unwrap_or(active_context_tokens)
  scope_tokens = active_context_tokens - baseline
```

`BodyAfterPrefix` 还有一个硬保护：即使 body 增长没到 scope limit，只要 `active_context_tokens >= model_context_window`，也要触发压缩，避免真的打满模型窗口。

关键结论：

- session total usage 是真实累计账本，不因压缩重置。
- compaction window baseline 是自动压缩判断用的窗口状态，不等于 session total。
- 服务端 usage 优先，本地估算只做 fallback / before-first-response / resume baseline。

## CloudAgent 当前状态

当前相关文件：

```text
crates/agent-core/src/turn/token_usage.rs
crates/agent-core/src/turn/token_usage_tests.rs
crates/agent-core/src/turn/regular.rs
crates/agent-core/src/turn/host.rs
crates/agent-core/src/host/agent.rs
crates/config/src/lib.rs
```

当前已经具备的基础：

- `TokenUsageUpdated` 已包含 `last_usage`、`total_usage`、`model_context_window`、`request_estimated_tokens`。
- `regular.rs` 已在收到 `response.usage` 后累计 `session_total_usage`。
- `latest_budget_baseline_from_rollout_items` 可以从 rollout 恢复最近 usage / compaction 后 baseline。
- 自动压缩后会用 `post_context_tokens_estimate` 更新 `last_sdk_context_tokens` 和 `last_request_estimated_tokens`，降低重复触发概率。

主要差距：

- `RestoredBudgetBaseline` 同时承担 session usage 恢复和 compaction window baseline，概念混在一起。
- 没有独立 `AutoCompactWindow` 状态。
- 没有 `Total | BodyAfterPrefix` scope 配置。
- 没有 `model_auto_compact_token_limit` 显式配置，以及 `min(configured, window * ratio)` 的统一 policy。
- 自动压缩触发仍主要落在 `regular.rs` 的临时变量里，业务逻辑、估算、日志、触发策略耦合较重。

## 目标模块结构

新增文件：

```text
crates/agent-core/src/turn/auto_compact_window.rs
crates/agent-core/src/turn/auto_compact_window_tests.rs
crates/agent-core/src/turn/auto_compact_policy.rs
crates/agent-core/src/turn/auto_compact_policy_tests.rs
```

调整现有文件：

```text
crates/agent-core/src/turn/token_usage.rs
crates/agent-core/src/turn/token_usage_tests.rs
crates/agent-core/src/turn/regular.rs
crates/agent-core/src/turn/host.rs
crates/agent-core/src/turn/mod.rs
crates/agent-core/src/host/agent.rs
crates/config/src/lib.rs
configs/config.toml.example
```

原则：

- `token_usage.rs` 管 session usage ledger 和 rollout 恢复。
- `auto_compact_window.rs` 管 compaction window baseline。
- `auto_compact_policy.rs` 管阈值和触发计算。
- `regular.rs` 只编排：取状态、算 status、决定是否调用 compaction、收到 usage 后更新状态。

## 配置设计

文件：

```text
crates/config/src/lib.rs
```

新增 enum：

```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AutoCompactTokenLimitScope {
    #[default]
    Total,
    BodyAfterPrefix,
}
```

在 `RuntimeConfig` 新增：

```rust
pub model_auto_compact_token_limit: Option<usize>,
pub model_auto_compact_token_limit_scope: AutoCompactTokenLimitScope,
```

在 `PartialRuntimeConfig` 新增：

```rust
model_auto_compact_token_limit: Option<Option<usize>>,
model_auto_compact_token_limit_scope: Option<AutoCompactTokenLimitScope>,
```

默认值：

```rust
model_auto_compact_token_limit: None,
model_auto_compact_token_limit_scope: AutoCompactTokenLimitScope::Total,
```

说明：

- 默认 `Total`，保持 Codex 默认语义。
- 用户要防重复压缩时可以配置 `BodyAfterPrefix`。
- `model_auto_compact_token_limit = None` 时使用 `model_context_window * context_compaction_trigger_ratio`。
- 如果显式设置 limit，最终使用 `min(configured, derived_limit)`，避免配置超过安全比例。

`apply_partial`：

```rust
if let Some(value) = runtime.model_auto_compact_token_limit {
    self.runtime.model_auto_compact_token_limit = value.map(|v| v.max(1));
}
if let Some(value) = runtime.model_auto_compact_token_limit_scope {
    self.runtime.model_auto_compact_token_limit_scope = value;
}
```

`RegularTurnSettings`：

文件：

```text
crates/agent-core/src/turn/host.rs
```

新增字段：

```rust
pub model_auto_compact_token_limit: Option<usize>,
pub model_auto_compact_token_limit_scope: AutoCompactTokenLimitScope,
```

`AgentHostParts` / `AgentHost` 构造 settings 时同步赋值。

`configs/config.toml.example` 增加示例：

```toml
# model_auto_compact_token_limit = 180000
# model_auto_compact_token_limit_scope = "total"
# 可选: "body_after_prefix"，压缩后只按新增 body token 计阈值。
```

## Session Token Usage 状态

文件：

```text
crates/agent-core/src/turn/token_usage.rs
```

将当前 `RestoredBudgetBaseline` 拆成更明确的类型。

新增：

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TokenUsageState {
    pub total_usage: ModelUsage,
    pub last_usage: Option<ModelUsage>,
    pub model_context_window: Option<u64>,
}
```

方法：

```rust
impl TokenUsageState {
    pub fn restore(total_usage: ModelUsage, last_usage: Option<ModelUsage>, model_context_window: Option<u64>) -> Self;
    pub fn append_server_usage(&mut self, usage: ModelUsage, model_context_window: Option<u64>);
    pub fn total_tokens(&self) -> usize;
    pub fn active_context_tokens_from_last_usage(&self) -> Option<usize>;
}
```

语义：

- `append_server_usage` 用服务端 usage 更新 session total。
- `total_usage` 是 session 累计，不受 compaction 重置影响。
- `active_context_tokens_from_last_usage` 用 `last_usage.total_tokens` 表示最近一次服务端观测到的 active context。

保留兼容结构，但改名建议：

```rust
pub struct RestoredTurnTokenState {
    pub usage: TokenUsageState,
    pub budget_baseline: RequestTokenBaseline,
    pub auto_compact_window: AutoCompactWindowSnapshot,
}

pub struct RequestTokenBaseline {
    pub server_context_tokens: Option<usize>,
    pub request_estimated_tokens: Option<usize>,
}
```

如果为了降低改动面，短期可以保留 `RestoredBudgetBaseline` 名字，但字段必须表达清楚：

```rust
pub struct RestoredBudgetBaseline {
    pub request_baseline: RequestTokenBaseline,
    pub usage: TokenUsageState,
    pub auto_compact_window: AutoCompactWindowSnapshot,
}
```

替换函数：

```rust
pub fn latest_budget_baseline_from_rollout_items(
    rollout_items: &[RolloutItem],
) -> Option<RestoredTurnTokenState>;
```

内部规则：

1. 遇到 `TokenUsageUpdated`：
   - 更新 `usage.total_usage`
   - 更新 `usage.last_usage`
   - 更新 `usage.model_context_window`
   - `request_baseline.server_context_tokens = Some(last_usage.total_tokens)`
   - `request_baseline.request_estimated_tokens = Some(request_estimated_tokens)`
2. 遇到 `ContextCompacted`：
   - 不改 `usage.total_usage`
   - `request_baseline.server_context_tokens = Some(post_context_tokens_estimate)`
   - `request_baseline.request_estimated_tokens = Some(post_context_tokens_estimate)`
   - `auto_compact_window.start_next()`
   - `auto_compact_window.set_estimated_prefill(post_context_tokens_estimate)`

测试文件：

```text
crates/agent-core/src/turn/token_usage_tests.rs
```

新增测试：

```rust
#[test]
fn server_usage_appends_to_session_total();

#[test]
fn compaction_does_not_reset_session_total_usage();

#[test]
fn rollout_restore_keeps_last_server_usage_and_estimated_compaction_prefill();
```

## AutoCompactWindow

文件：

```text
crates/agent-core/src/turn/auto_compact_window.rs
```

定义：

```rust
use crate::model::ModelUsage;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutoCompactWindowSnapshot {
    pub ordinal: u64,
    pub prefill_input_tokens: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AutoCompactWindowPrefill {
    ServerObserved(usize),
    Estimated(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoCompactWindow {
    ordinal: u64,
    prefill_input_tokens: Option<AutoCompactWindowPrefill>,
}
```

方法：

```rust
impl AutoCompactWindow {
    pub fn new() -> Self;
    pub fn from_snapshot(snapshot: AutoCompactWindowSnapshot) -> Self;
    pub fn clear_prefill(&mut self);
    pub fn start_next(&mut self);
    pub fn set_estimated_prefill(&mut self, tokens: usize);
    pub fn ensure_server_observed_prefill_from_usage(&mut self, usage: &ModelUsage);
    pub fn snapshot(&self) -> AutoCompactWindowSnapshot;
}
```

语义与 Codex 保持一致：

- `ordinal` 从 1 开始。
- `start_next` 递增 ordinal，并清空 prefill。
- `set_estimated_prefill` 只能在没有 server observed 时写入。
- `ensure_server_observed_prefill_from_usage` 用 `usage.input_tokens` 覆盖 estimated。
- 已有 server observed 时，不再被后续 estimated 或 server usage 改写。

测试文件：

```text
crates/agent-core/src/turn/auto_compact_window_tests.rs
```

测试：

```rust
#[test]
fn starts_with_first_window_without_prefill();

#[test]
fn estimated_prefill_is_replaced_by_first_server_observed_usage();

#[test]
fn server_observed_prefill_is_sticky();

#[test]
fn start_next_advances_ordinal_and_clears_prefill();

#[test]
fn negative_or_missing_values_are_not_possible_with_usize();
```

`mod.rs`：

```text
crates/agent-core/src/turn/mod.rs
```

增加：

```rust
mod auto_compact_window;

#[cfg(test)]
mod auto_compact_window_tests;

pub use auto_compact_window::{AutoCompactWindow, AutoCompactWindowSnapshot};
```

## AutoCompactPolicy

文件：

```text
crates/agent-core/src/turn/auto_compact_policy.rs
```

定义状态：

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AutoCompactTokenStatus {
    pub active_context_tokens: usize,
    pub scope_tokens: usize,
    pub limit_tokens: usize,
    pub full_context_window_limit: Option<usize>,
    pub window_ordinal: Option<u64>,
    pub window_prefill_tokens: Option<usize>,
    pub full_context_window_limit_reached: bool,
    pub token_limit_reached: bool,
}
```

定义输入：

```rust
pub struct AutoCompactPolicyInput {
    pub model_context_window: usize,
    pub trigger_ratio: f32,
    pub configured_limit: Option<usize>,
    pub scope: AutoCompactTokenLimitScope,
    pub active_context_tokens: usize,
    pub window: AutoCompactWindowSnapshot,
}
```

函数：

```rust
pub fn derived_auto_compact_limit(model_context_window: usize, trigger_ratio: f32) -> usize;

pub fn effective_auto_compact_limit(
    model_context_window: usize,
    trigger_ratio: f32,
    configured_limit: Option<usize>,
) -> usize;

pub fn auto_compact_token_status(input: AutoCompactPolicyInput) -> AutoCompactTokenStatus;
```

实现：

```rust
pub fn derived_auto_compact_limit(model_context_window: usize, trigger_ratio: f32) -> usize {
    ((model_context_window as f32) * trigger_ratio).floor().max(1.0) as usize
}

pub fn effective_auto_compact_limit(
    model_context_window: usize,
    trigger_ratio: f32,
    configured_limit: Option<usize>,
) -> usize {
    let derived = derived_auto_compact_limit(model_context_window, trigger_ratio);
    configured_limit.map(|limit| limit.min(derived)).unwrap_or(derived)
}
```

`Total`：

```rust
scope_tokens = active_context_tokens;
limit_tokens = effective_auto_compact_limit(...);
full_context_window_limit = None;
token_limit_reached = scope_tokens >= limit_tokens;
```

`BodyAfterPrefix`：

```rust
baseline = window.prefill_input_tokens.unwrap_or(active_context_tokens);
scope_tokens = active_context_tokens.saturating_sub(baseline);
limit_tokens = effective_auto_compact_limit(...);
full_context_window_limit = Some(model_context_window);
full_context_window_limit_reached = active_context_tokens >= model_context_window;
token_limit_reached = scope_tokens >= limit_tokens || full_context_window_limit_reached;
```

测试文件：

```text
crates/agent-core/src/turn/auto_compact_policy_tests.rs
```

测试：

```rust
#[test]
fn default_limit_is_context_window_times_ratio();

#[test]
fn configured_limit_is_capped_by_ratio_limit();

#[test]
fn total_scope_counts_full_active_context();

#[test]
fn body_after_prefix_subtracts_prefill_baseline();

#[test]
fn body_after_prefix_without_prefill_uses_active_context_as_baseline();

#[test]
fn body_after_prefix_still_triggers_at_full_context_window();
```

## Regular Turn 集成

文件：

```text
crates/agent-core/src/turn/regular.rs
```

### 1. 恢复状态

替换：

```rust
let (mut last_sdk_context_tokens, mut last_request_estimated_tokens, restored_total_usage) =
    restore_budget_baseline_from_host(host, conversation_id).await?;
session_total_usage = restored_total_usage;
```

目标：

```rust
let restored = restore_turn_token_state_from_host(host, conversation_id).await?;
let mut token_usage_state = restored.usage;
let mut request_baseline = restored.request_baseline;
let mut auto_compact_window = AutoCompactWindow::from_snapshot(restored.auto_compact_window);
```

保留本地估算校准：

```rust
let active_context_tokens = match (
    token_usage_state.active_context_tokens_from_last_usage(),
    request_baseline.request_estimated_tokens,
) {
    (Some(server_tokens), Some(previous_request_tokens)) => {
        apply_signed_token_delta(server_tokens, candidate_request_tokens, previous_request_tokens)
    }
    _ => candidate_request_tokens,
};
```

命名建议：

- 把 `estimated_total_tokens` 改成 `active_context_tokens_estimate`。
- 把 `last_sdk_context_tokens` 改成 `request_baseline.server_context_tokens`。
- 把 `last_request_estimated_tokens` 改成 `request_baseline.request_estimated_tokens`。

### 2. 自动压缩触发判断

在构建 candidate request 后：

```rust
let token_status = auto_compact_token_status(AutoCompactPolicyInput {
    model_context_window: settings.model_context_window as usize,
    trigger_ratio: settings.context_compaction_trigger_ratio,
    configured_limit: settings.model_auto_compact_token_limit,
    scope: settings.model_auto_compact_token_limit_scope,
    active_context_tokens: active_context_tokens_estimate.max(compaction_estimated_total_tokens),
    window: auto_compact_window.snapshot(),
});
```

传给 compaction：

```rust
CompactionMode::Automatic {
    estimated_total_tokens: token_status.active_context_tokens,
    continuation: compaction_continuation(roundtrip_count),
}
```

如果短期保留 `maybe_compact_history_with_start_callback` 内部判断，它也必须接收 policy 后的 `token_limit_reached`，避免外层算了 status、内层又用旧公式重算。推荐新增 wrapper：

```rust
async fn maybe_compact_when_token_status_reached<H: TurnHost>(
    host: &H,
    history: &mut ConversationHistory,
    cancellation_token: &CancellationToken,
    token_status: AutoCompactTokenStatus,
    continuation: CompactionContinuation,
    on_start: impl FnOnce(CompactionStart),
) -> Result<Option<AppliedCompaction>>;
```

更理想的拆法是在下一阶段放到：

```text
crates/agent-core/src/turn/auto_compact.rs
```

### 3. 压缩成功后启动新 window

当前：

```rust
last_sdk_context_tokens = Some(compacted.post_context_tokens_estimate as usize);
last_request_estimated_tokens = Some(compacted.post_context_tokens_estimate as usize);
```

目标：

```rust
request_baseline.server_context_tokens = Some(compacted.post_context_tokens_estimate as usize);
request_baseline.request_estimated_tokens = Some(compacted.post_context_tokens_estimate as usize);
auto_compact_window.start_next();
auto_compact_window.set_estimated_prefill(compacted.post_context_tokens_estimate as usize);
```

注意：

- 不要重置 `token_usage_state.total_usage`。
- `ContextCompacted` 事件可以暂时保持不变，baseline 可从 rollout 的 `ContextCompacted.post_context_tokens_estimate` 恢复。

### 4. 收到服务端 usage 后校准

当前：

```rust
session_total_usage.add_assign(&usage);
last_sdk_context_tokens = Some(usage.total_tokens as usize);
last_request_estimated_tokens = Some(final_budget.estimated_tokens);
emit TokenUsageUpdated { ... }
```

目标：

```rust
token_usage_state.append_server_usage(usage.clone(), Some(settings.model_context_window));
request_baseline.server_context_tokens = Some(usage.total_tokens as usize);
request_baseline.request_estimated_tokens = Some(final_budget.estimated_tokens);

if settings.model_auto_compact_token_limit_scope == AutoCompactTokenLimitScope::BodyAfterPrefix {
    auto_compact_window.ensure_server_observed_prefill_from_usage(&usage);
}

emit_event(
    &mut events,
    on_event,
    EventMsg::TokenUsageUpdated {
        turn_id: turn_id.to_string(),
        last_usage: usage,
        total_usage: token_usage_state.total_usage.clone(),
        model_context_window: token_usage_state.model_context_window,
        request_estimated_tokens: final_budget.estimated_tokens as u64,
    },
);
```

这里和 Codex 的差异点要明确：Codex 的 `ensure_server_observed_prefill_from_usage` 记录第一条服务端 usage 的 `input_tokens`，不是 `total_tokens`。CloudAgent 也应使用 `ModelUsage.input_tokens`。

### 5. context budget log

`append_context_budget_log` 目前写：

```rust
compaction_triggered: compaction_triggered_now,
sdk_total_tokens: last_sdk_context_tokens,
estimated_total_tokens,
```

建议扩展 `ContextBudgetLogEntry`：

```rust
pub active_context_tokens: usize,
pub auto_compact_scope_tokens: usize,
pub auto_compact_limit_tokens: usize,
pub auto_compact_scope: AutoCompactTokenLimitScope,
pub auto_compact_window_ordinal: Option<u64>,
pub auto_compact_window_prefill_tokens: Option<usize>,
pub full_context_window_limit_reached: bool,
```

短期如果不想改日志 schema，至少把 `compaction_triggered_now` 改成：

```rust
let compaction_triggered_now = token_status.token_limit_reached;
```

## TurnHost 与 AgentHost

文件：

```text
crates/agent-core/src/turn/host.rs
```

将：

```rust
async fn restore_budget_baseline(...) -> Result<Option<RestoredBudgetBaseline>>;
```

替换或新增兼容：

```rust
async fn restore_turn_token_state(
    &self,
    conversation_id: &str,
) -> Result<Option<RestoredTurnTokenState>>;
```

短期可保留旧方法，内部调用新方法：

```rust
async fn restore_budget_baseline(...) -> Result<Option<RestoredBudgetBaseline>> {
    Ok(self.restore_turn_token_state(...).await?.map(Into::into))
}
```

文件：

```text
crates/agent-core/src/host/agent.rs
```

实现：

```rust
async fn restore_turn_token_state(
    &self,
    conversation_id: &str,
) -> Result<Option<RestoredTurnTokenState>> {
    self.rollout_recorder.flush().await?;
    let rollout_items = self.store.load_rollout_items(conversation_id).await?;
    Ok(latest_turn_token_state_from_rollout_items(&rollout_items))
}
```

## 是否需要新增 rollout event

第一阶段不新增。

理由：

- 当前 `TokenUsageUpdated` 已能恢复 session usage。
- 当前 `ContextCompacted.post_context_tokens_estimate` 已能恢复 estimated prefill。
- 新增 event 会牵涉协议、投影、网关、UI，改动面更大。

第一阶段恢复规则：

```text
TokenUsageUpdated -> restore TokenUsageState
ContextCompacted  -> auto_compact_window.start_next + estimated prefill
```

第二阶段如果需要更精确持久化 server observed prefill，再新增事件：

```rust
EventMsg::AutoCompactWindowUpdated {
    turn_id: TurnId,
    ordinal: u64,
    prefill_input_tokens: Option<u64>,
    prefill_source: AutoCompactWindowPrefillSource,
}
```

但这不是本次必须项。原因是 server observed prefill 丢失后，重启恢复会退回 estimated prefill；这会有误差，但不会破坏“避免刚压缩完重复触发”的主目标。

## 实施顺序

### Commit 1：配置与 policy

修改：

```text
crates/config/src/lib.rs
crates/agent-core/src/turn/host.rs
crates/agent-core/src/host/agent.rs
crates/agent-core/src/turn/mod.rs
configs/config.toml.example
```

新增：

```text
crates/agent-core/src/turn/auto_compact_policy.rs
crates/agent-core/src/turn/auto_compact_policy_tests.rs
```

验收：

```text
cargo test -p agent-core auto_compact_policy
cargo check -p config
```

### Commit 2：AutoCompactWindow

新增：

```text
crates/agent-core/src/turn/auto_compact_window.rs
crates/agent-core/src/turn/auto_compact_window_tests.rs
```

修改：

```text
crates/agent-core/src/turn/mod.rs
```

验收：

```text
cargo test -p agent-core auto_compact_window
```

### Commit 3：TokenUsageState 与 rollout 恢复

修改：

```text
crates/agent-core/src/turn/token_usage.rs
crates/agent-core/src/turn/token_usage_tests.rs
crates/agent-core/src/turn/host.rs
crates/agent-core/src/host/agent.rs
crates/agent-core/src/turn/mod.rs
```

验收：

```text
cargo test -p agent-core token_usage
cargo test -p agent-core rollout_reconstruction
```

### Commit 4：regular.rs 接入

修改：

```text
crates/agent-core/src/turn/regular.rs
```

要点：

- 用 `TokenUsageState` 替代 `session_total_usage`。
- 用 `RequestTokenBaseline` 替代两个散落的 `last_*_tokens`。
- 用 `AutoCompactWindow` 管 compaction baseline。
- 用 `auto_compact_token_status` 替代临时阈值判断。
- 收到服务端 usage 后校准 `TokenUsageState`、`RequestTokenBaseline`、`AutoCompactWindow`。

验收：

```text
cargo test -p agent-core regular
cargo test -p agent-core token_usage
cargo test -p agent-core auto_compact
```

### Commit 5：日志与回归测试

修改：

```text
crates/agent-core/src/context/budget.rs 或 ContextBudgetLogEntry 定义位置
crates/agent-core/src/turn/regular.rs
```

新增测试：

```text
crates/agent-core/src/turn/auto_compact_policy_tests.rs
crates/agent-core/src/turn/token_usage_tests.rs
```

可选新增集成测试：

```text
crates/agent-core/tests/auto_compaction_budget.rs
```

验收：

```text
cargo fmt --all --check
cargo test -p agent-core
```

## 具体测试矩阵

`auto_compact_policy_tests.rs`：

```text
default limit = window * ratio
configured limit smaller than derived -> configured
configured limit larger than derived -> derived
Total scope: active 180000 / limit 180000 -> trigger
BodyAfterPrefix: active 190000, prefill 50000, limit 180000 -> no trigger
BodyAfterPrefix: active 231000, prefill 50000, limit 180000 -> trigger
BodyAfterPrefix: active >= model_context_window -> trigger even if scope body below limit
BodyAfterPrefix without prefill -> baseline = active, scope = 0
```

`auto_compact_window_tests.rs`：

```text
new window ordinal is 1
estimated prefill is recorded
server observed input_tokens replaces estimated
second server observed sample does not replace first
estimated sample does not replace server observed sample
start_next increments ordinal and clears prefill
```

`token_usage_tests.rs`：

```text
append_server_usage accumulates session total
last_usage is replaced by latest usage
compaction rollout keeps session total unchanged
compaction rollout starts a new window with estimated prefill
TokenUsageUpdated after compaction restores server baseline and request estimate
```

`regular.rs` 现有测试或新增就近测试：

```text
response usage emits cumulative TokenUsageUpdated total_usage
auto compaction sets estimated prefill to post_context_tokens_estimate
body_after_prefix uses server usage input_tokens as prefill after first response
total scope ignores compaction prefill
```

## 验收标准

功能验收：

- 没有显式 `model_auto_compact_token_limit` 时，阈值是 `model_context_window * context_compaction_trigger_ratio`。
- 显式 limit 大于默认比例阈值时，最终仍被 cap 到默认比例阈值。
- `Total` scope 下，压缩后 active context 仍按全量计算。
- `BodyAfterPrefix` scope 下，压缩后使用 baseline，只计算压缩窗口之后新增 token。
- 收到服务端 usage 后，用 `usage.total_tokens` 校准 active context，用 `usage.input_tokens` 校准 window prefill。
- 压缩不会清空或重置 session `total_usage`。

结构验收：

- `regular.rs` 不新增大段 token policy 细节。
- 自动压缩阈值计算集中在 `auto_compact_policy.rs`。
- compaction baseline 集中在 `auto_compact_window.rs`。
- usage rollout 恢复集中在 `token_usage.rs`。
- 测试在独立 `*_tests.rs`，不塞进业务文件主体。

## 推荐最终形态

主循环应接近：

```text
execute_regular_turn
  -> restore TokenUsageState + RequestTokenBaseline + AutoCompactWindow
  -> build candidate model request
  -> estimate request tokens
  -> active_context_tokens = server usage calibrated estimate
  -> token_status = auto_compact_policy(...)
  -> if token_status.token_limit_reached:
       run automatic compaction
       auto_compact_window.start_next()
       auto_compact_window.set_estimated_prefill(post_context_tokens_estimate)
  -> send model request
  -> on response usage:
       token_usage_state.append_server_usage(...)
       request_baseline = server total + final estimated request tokens
       auto_compact_window.ensure_server_observed_prefill_from_usage(...)
       emit TokenUsageUpdated with cumulative total_usage
```

这就是和 Codex 对齐的关键：会话账本保持真实累计，压缩窗口 baseline 单独存在，触发策略独立可测。
