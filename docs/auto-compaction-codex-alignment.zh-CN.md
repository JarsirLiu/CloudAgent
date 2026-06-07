# 自动压缩对齐 Codex 的实施方案

## 目标

把当前自动压缩从“普通 turn 循环中直接修改 `history.messages`”升级为稳定的 compaction checkpoint 机制。

改完后应满足：

- 手动压缩、自动 pre-turn 压缩、自动 mid-turn 压缩共享压缩内核，但入口、phase、上下文注入策略清晰分离。
- 自动压缩成功后产生明确的 `Compacted` checkpoint，恢复会话时从最新 checkpoint 重建历史。
- 压缩后同一个 turn 继续执行时，不把不完整工具链当成普通完成历史。
- turn 被中断时，模型历史包含 `<turn_aborted>` 边界。
- 自动压缩后模型进入 tool-only 循环时，有可控上限和可见失败信息。
- 代码按业务模块拆分，不继续把逻辑堆到 `regular.rs`、`compaction.rs` 或 `transcript.rs`。

## 当前问题

当前自动压缩路径主要在：

```text
crates/agent-core/src/turn/regular.rs
crates/agent-core/src/turn/compaction.rs
crates/agent-core/src/context/compaction.rs
crates/agent-core/src/projection/transcript.rs
```

当前形态：

```text
execute_regular_turn
  -> maybe_compact_history(...)
     -> build summary request
     -> complete_model_request(...)
     -> apply_compaction(&mut history.messages, ...)
  -> persist RolloutItem::Compacted
  -> 继续同一个 turn 的 model request / tool loop
```

主要问题：

- `ContextCompactionStarted` 在 `maybe_compact_history(...).await` 之后才发，生命周期边界不准确。
- `maybe_compact_history` 同时承担策略判断、摘要请求、应用历史、manual / auto 模式分支，职责过重。
- `apply_history_compaction` 直接生成 `system + summary + preserved_tail`，而 Codex 压缩后的主体更接近“最近真实用户消息 + 摘要”。
- `conversation_history_from_rollout_items` 线性 replay rollout，缺少“从最新 compaction checkpoint 反向定位，再 replay suffix”的重建语义。
- 自动压缩后同一个 turn 继续产生 tool calls，若一直没有 assistant content，CLI 只能显示 working。
- `max_tool_roundtrips` 默认 `None`，压缩后 tool-only 循环没有稳定上限。

## Codex 对齐目标

Codex 的关键语义不是“所有压缩入口完全同一条路径”，而是：

```text
共享压缩内核
入口分离
phase 分离
checkpoint 分离
恢复重建分离
```

## 方案审查结论

本方案是合理方向，但实施时必须守住三个边界：

1. `replacement_history` 不是“把旧 history 截短”，而是一个新的模型上下文基座。
   它应该由最近真实用户消息和压缩摘要组成，工具事实进入摘要，不再把原始 assistant/tool 链条塞进 replacement history 主体。

2. 压缩 checkpoint 和 checkpoint 后的 suffix 是两个概念。
   checkpoint 负责替换旧历史；checkpoint 后同一个 turn 新产生的 assistant/tool 才作为 suffix replay。恢复时必须从最新 checkpoint 开始，而不是从头线性 replay。

3. 自动压缩后的下一次模型请求必须可检查。
   每次压缩完成后，audit/debug 日志要能记录最终发给模型的 message role 序列、message_count、是否含 raw tool tail、summary 位置、latest real user 位置。

当前文档里最需要特别注意的点：

- `build_compacted_replacement_history` 不能简单丢失工具事实；它要通过 `CompactionSummary` 的 `Tool / Code Facts` 承载工具事实，但不要保留原始 tool message。
- `ContextInjectionStrategy::Standard` 不能再简单找最后一个 `ResponseItem::User`，否则会把 `[Context Summary]` 当成普通用户消息。插入上下文时必须使用 `last real user or summary` 语义。
- `max_tool_roundtrips` 默认值必须收敛，否则自动压缩后模型持续 tool-only 响应时，用户仍然只能看到 working。
- `ContextCompactionStarted` 必须在摘要模型请求之前发出，否则 UI 和 audit 都会误判压缩耗时。

## 自动压缩后发给模型的上下文格式

这一节是实施验收重点。每次改完压缩逻辑，都要对照这里看最终 `ModelRequest.messages`。

### 当前旧实现的实际格式

当前旧实现压缩后 `replacement_history` 是：

```text
0 system: system prompt
1 system 或 user: [Context Summary] ...
2..N preserved_tail 原始尾巴
```

旧坏会话里的证据：

```text
context_compacted:
  pre_message_count = 95
  post_message_count = 7

下一次 model.requested:
  message_count = 8
```

这说明压缩后实际请求大概是：

```text
0 system: system prompt
1 system: [Context Summary] ...       // 旧逻辑里是第二个 system
2 user: 最近保留的用户问题
3 assistant: 最近保留的回答
4 user: 最近保留的用户问题
5 assistant: 最近保留的回答
6 user: 当前用户请求
7 user: <environment_context>...      // 或被插入到最新 user 前后的上下文片段
tools: [exec_command, apply_patch, ...]
```

更一般地说，当前实现会形成：

```text
system
summary
raw preserved_tail(user/assistant/tool)
context fragments
tools
```

问题：

- 旧 rollout 可能出现第二个 `system`。
- `preserved_tail` 可能包含原始 assistant/tool 链条。
- 压缩后同一个 turn 继续执行，会继续追加新的 assistant/tool。
- 如果模型压缩后只返回 tool calls，不返回 content，CLI 看不到 assistant 文本。

### 目标格式：Auto PreTurn

Auto pre-turn 的压缩发生在本轮模型采样前。压缩后发给模型的 `ModelRequest.messages` 目标形状是：

```text
0 system: system prompt
1 user: <environment_context>...</environment_context>       // 当前 turn 运行上下文
2 user: <long_term_memory>...</long_term_memory>             // 可选，预算允许才有
3 user: <skills_context>...</skills_context>                 // 可选，预算允许才有
4 user: 最近真实用户消息 A
5 user: 最近真实用户消息 B
6 user: 当前用户消息
7 user: [Context Summary]
        Current Task:
        - ...
        Progress:
        - ...
        Key Decisions:
        - ...
        Important Context:
        - ...
        Tool / Code Facts:
        - ...
        Next Steps:
        - ...
tools: 当前可见工具 specs
```

约束：

- `system` 最多一条，且只能在第 0 条。
- `[Context Summary]` 必须是 `user`，不能是 `system`。
- `assistant/tool` 原始消息不进入 replacement history 主体。
- 最近真实用户消息按 token 预算保留。
- 工具执行事实写进 summary 的 `Tool / Code Facts`。
- 上下文片段插入时不能把 `[Context Summary]` 当成普通用户消息；应插到最后真实用户前，或者在没有真实用户时插到 summary 前。

### 目标格式：Auto MidTurn

Auto mid-turn 的压缩发生在模型已经采样过、还需要 follow-up 的场景。压缩后继续同一个 turn 的下一次 `ModelRequest.messages` 目标形状是：

```text
0 system: system prompt
1 user: <environment_context>...</environment_context>
2 user: <long_term_memory>...</long_term_memory>             // 可选
3 user: <skills_context>...</skills_context>                 // 可选
4 user: 最近真实用户消息 A
5 user: 当前用户消息
6 user: [Context Summary]
        Current Task:
        - ...
        Progress:
        - ...
        Tool / Code Facts:
        - 已完成的工具事实被总结在这里
        Next Steps:
        - ...
tools: 当前可见工具 specs
```

mid-turn 的特殊点：

- initial context 使用 `BeforeLastUserMessage` 策略。
- summary 保持为压缩 checkpoint 的最后语义项。
- 压缩前已经发生的工具结果不作为 raw `tool` message 保留，而是进入 summary。
- 压缩后新产生的 assistant/tool 才作为 checkpoint suffix 记录。

### 目标格式：中断后的下一 turn

如果压缩后同一个 turn 被用户中断，下一次用户发消息时目标上下文是：

```text
0 system: system prompt
1 user: <environment_context>...</environment_context>
2 user: 最近真实用户消息
3 user: [Context Summary] ...
4 assistant/tool: checkpoint 后已经完成且安全 replay 的工具事实       // 可选
5 user: <turn_aborted>
        The user interrupted the previous turn on purpose...
        </turn_aborted>
6 user: 新用户消息
tools: 当前可见工具 specs
```

约束：

- `<turn_aborted>` 必须在 `TurnCancelled` 前持久化。
- 只有 checkpoint 后已经完成且协议完整的 assistant/tool 才能 replay。
- user-only cancelled turn 直接丢弃，不污染下一次请求。

### Debug 日志要求

压缩完成后的第一条 `model.requested` 应记录：

```json
{
  "message_count": 8,
  "tool_count": 4,
  "compaction_phase": "pre_turn",
  "message_roles": ["system", "user", "user", "user", "user"],
  "summary_index": 4,
  "raw_tool_messages_before_summary": 0,
  "raw_assistant_messages_before_summary": 0,
  "latest_real_user_index": 3
}
```

这样以后再出现“压缩后不回消息”，可以直接判断是：

- 上下文形状错了。
- provider 卡住。
- 模型进入 tool-only 循环。
- tool loop 上限没有触发。

目标形态：

```text
Manual /compact
  trigger = Manual
  phase = StandaloneTurn
  initial_context_injection = DoNotInject
  压缩完成后结束 standalone compaction turn

Auto pre-turn
  trigger = Auto
  phase = PreTurn
  initial_context_injection = DoNotInject
  压缩完成后再发送本轮模型请求

Auto mid-turn
  trigger = Auto
  phase = MidTurn
  initial_context_injection = BeforeLastUserMessage
  压缩完成后继续当前 turn
```

压缩后的 replacement history 目标结构：

```text
可选 initial context
最近真实用户消息
user: [Context Summary] ...
```

不把摘要作为第二个 `system`。
不把旧 assistant/tool 全量链条作为压缩后主体。
必要的工具事实只应作为安全 suffix 由 rollout reconstruction 决定是否 replay。

## 目标模块结构

新增或拆分为：

```text
crates/agent-core/src/turn/compaction/
  mod.rs
  model.rs          // trigger / reason / phase / injection / outcome
  service.rs        // 压缩主服务，串联 planner / summarizer / applier
  planner.rs        // 是否压缩、选择源历史、选择 preserved users
  summarizer.rs     // 构造摘要请求、调用模型、解析摘要
  applier.rs        // 生成 replacement history，不做 I/O
  lifecycle.rs      // started / compacted / failed 事件和 audit

crates/agent-core/src/rollout/
  reconstruction.rs // 模型历史重建，反向找最新 checkpoint
  markers.rs        // <turn_aborted> 等模型可见 marker

crates/agent-core/src/turn/
  auto_compact.rs   // regular turn 内自动压缩入口
  manual_compact.rs // /compact standalone 入口
  regular.rs        // 只保留主 turn loop，移出压缩细节
```

如果希望先小步落地，可以先不移动文件夹，但必须先按这些模块边界拆函数。最终不应让单个文件继续增长成大单体。

## 数据结构

### 新增 `CompactionTrigger`

文件：

```text
crates/agent-core/src/turn/compaction/model.rs
```

定义：

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionTrigger {
    Manual,
    Auto,
}
```

### 新增 `CompactionReason`

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionReason {
    UserRequested,
    ContextLimit,
    ModelDownshift,
}
```

### 新增 `CompactionPhase`

替代或包裹当前 `CompactionContinuation`：

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPhase {
    StandaloneTurn,
    PreTurn,
    MidTurn,
}
```

兼容层：

```rust
impl From<CompactionPhase> for CompactionContinuation {
    fn from(phase: CompactionPhase) -> Self {
        match phase {
            CompactionPhase::StandaloneTurn | CompactionPhase::PreTurn => {
                CompactionContinuation::PreTurn
            }
            CompactionPhase::MidTurn => CompactionContinuation::MidTurn,
        }
    }
}
```

### 新增 `InitialContextInjection`

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InitialContextInjection {
    DoNotInject,
    BeforeLastUserMessage,
}
```

### 新增 `CompactionRequest`

```rust
pub struct CompactionRequest {
    pub conversation_id: String,
    pub turn_id: String,
    pub trigger: CompactionTrigger,
    pub reason: CompactionReason,
    pub phase: CompactionPhase,
    pub initial_context_injection: InitialContextInjection,
    pub estimated_total_tokens: Option<usize>,
    pub minimum_history_tokens: usize,
}
```

### 新增 `CompactionOutcome`

替代当前 `AppliedCompaction` 扩展字段：

```rust
pub struct CompactionOutcome {
    pub summary: CompactionSummary,
    pub rendered_summary: String,
    pub replacement_history: Vec<ResponseItem>,
    pub trigger: CompactionTrigger,
    pub reason: CompactionReason,
    pub phase: CompactionPhase,
    pub pre_context_tokens_estimate: u64,
    pub post_context_tokens_estimate: u64,
    pub pre_message_count: usize,
    pub post_message_count: usize,
    pub preserved_user_count: usize,
}
```

## 实施步骤

## 第一步：拆出压缩模型和 marker

新增文件：

```text
crates/agent-core/src/turn/compaction/model.rs
crates/agent-core/src/rollout/markers.rs
```

移动现有函数：

```text
projection/transcript.rs::turn_aborted_marker_text
projection/transcript.rs::append_turn_aborted_marker_if_needed
projection/transcript.rs::response_item_is_turn_aborted_marker
projection/transcript.rs::response_item_counts_as_user_turn
```

目标 API：

```rust
pub fn turn_aborted_marker_text() -> &'static str;
pub fn turn_aborted_marker_item() -> ResponseItem;
pub fn is_turn_aborted_marker(item: &ResponseItem) -> bool;
pub fn is_context_summary_item(item: &ResponseItem) -> bool;
pub fn counts_as_real_user_turn(item: &ResponseItem) -> bool;
```

修改：

```text
crates/agent-core/src/projection/transcript.rs
crates/agent-core/src/context/compaction.rs
crates/agent-core/src/context/fragments.rs
```

把各处重复的 `[Context Summary]` / `<turn_aborted>` 判断改为调用 `rollout::markers`。

测试：

```text
cargo test -p agent-core marker --lib
cargo test -p agent-core conversation_history_ --lib
```

## 第二步：把压缩 planning 从 `maybe_compact_history` 拆出

新增文件：

```text
crates/agent-core/src/turn/compaction/planner.rs
```

新增类型：

```rust
pub struct CompactionPlanDecision {
    pub should_compact: bool,
    pub minimum_history_tokens: usize,
    pub estimated_history_tokens: usize,
    pub config: ContextCompactionConfig,
}
```

新增函数：

```rust
pub fn plan_compaction_for_mode(
    history: &ConversationHistory,
    settings: &RegularTurnSettings,
    environment_context: &EnvironmentContext,
    mode: CompactionMode,
    context_facade: &ContextFacade,
) -> Option<CompactionPlanDecision>;
```

迁移逻辑：

```text
turn/compaction.rs::maybe_compact_history
  - estimated_history_tokens
  - ContextCompactionConfig 构造
  - Manual minimum_history_tokens 判断
  - Automatic available_history_tokens 判断
```

保留 `context::plan_manual_history_compaction` 作为纯上下文算法，不在这里做 I/O。

验收：

- `maybe_compact_history` 中不再出现 budget 判断细节。
- planner 无 async、无 host、无模型调用。

## 第三步：把摘要请求拆出 summarizer

新增文件：

```text
crates/agent-core/src/turn/compaction/summarizer.rs
```

新增函数：

```rust
pub async fn summarize_compaction_source<H: TurnHost>(
    host: &H,
    cancellation_token: &CancellationToken,
    filtered_plan: &ContextCompactionPlan,
    config: ContextCompactionConfig,
    temperature: f32,
) -> Result<CompactionSummary>;
```

迁移逻辑：

```text
build_compaction_summary_request(...)
host.complete_model_request(...)
CompactionSummary::from_model_output(...)
fallback_from_plan(...)
```

注意：

- summarizer 只负责生成摘要。
- summarizer 不修改 `history.messages`。
- summarizer 不持久化 rollout。

测试：

```text
cargo test -p agent-core compaction_summary --lib
```

## 第四步：重写 applier，生成 Codex 风格 replacement history

新增文件：

```text
crates/agent-core/src/turn/compaction/applier.rs
```

新增函数：

```rust
pub fn build_compacted_replacement_history(
    history: &[ResponseItem],
    summary: &CompactionSummary,
    plan: &ContextCompactionPlan,
    injection: InitialContextInjection,
    initial_context: &[ResponseItem],
) -> Vec<ResponseItem>;
```

内部拆函数：

```rust
fn collect_recent_real_user_messages(
    history: &[ResponseItem],
    max_tokens: usize,
) -> Vec<ResponseItem>;

fn summary_response_item(summary: &CompactionSummary, max_tokens: usize) -> ResponseItem;

fn insert_initial_context_before_last_real_user_or_summary(
    replacement_history: Vec<ResponseItem>,
    initial_context: &[ResponseItem],
) -> Vec<ResponseItem>;
```

目标 replacement history：

```text
可选 initial context
最近真实用户消息
user: [Context Summary] ...
```

不要再生成：

```text
system
system: [Context Summary]
preserved assistant/tool tail
```

如果短期必须保留 `system`，需要满足：

```text
system 只能是第 0 条
summary 必须是 user
assistant/tool tail 不能作为 replacement_history 主体
```

修改：

```text
crates/agent-core/src/context/compaction.rs::apply_history_compaction
```

建议将其降级为纯算法兼容函数，最终由 `turn/compaction/applier.rs` 调用或替代。

测试：

```text
replacement_history_uses_user_summary_not_second_system
replacement_history_drops_assistant_tool_tail
mid_turn_replacement_injects_initial_context_before_last_real_user
summary_item_does_not_count_as_real_user_turn
```

## 第五步：新增 compaction service，替换 `maybe_compact_history`

新增文件：

```text
crates/agent-core/src/turn/compaction/service.rs
```

新增主入口：

```rust
pub async fn run_compaction<H: TurnHost>(
    host: &H,
    history: &mut ConversationHistory,
    cancellation_token: &CancellationToken,
    request: CompactionRequest,
) -> Result<Option<CompactionOutcome>>;
```

职责：

```text
1. planner 判断是否需要压缩
2. 构造 filtered_plan / raw_plan
3. summarizer 生成摘要
4. applier 生成 replacement_history
5. 用 replacement_history 替换 history.messages
6. 返回 CompactionOutcome
```

不得做：

- 不直接 emit UI event。
- 不直接 persist rollout。
- 不直接 flush rollout。

这样 service 可在 manual / auto / test 中复用。

保留兼容：

```rust
pub async fn maybe_compact_history<H>(...) -> Result<Option<AppliedCompaction>>
```

短期让它调用 `run_compaction`，后续删除。

## 第六步：抽出自动压缩入口

新增文件：

```text
crates/agent-core/src/turn/auto_compact.rs
```

新增函数：

```rust
pub async fn maybe_run_pre_turn_auto_compaction<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    context_manager: &mut ContextManager,
    cancellation_token: &CancellationToken,
    estimated_total_tokens: usize,
    on_event: &mut dyn FnMut(&EventMsg),
) -> Result<Option<CompactionOutcome>>;
```

新增函数：

```rust
pub async fn maybe_run_mid_turn_auto_compaction<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    turn_id: &str,
    context_manager: &mut ContextManager,
    cancellation_token: &CancellationToken,
    estimated_total_tokens: usize,
    on_event: &mut dyn FnMut(&EventMsg),
) -> Result<Option<CompactionOutcome>>;
```

职责：

- 在调用 `run_compaction` 前 emit `ContextCompactionStarted`。
- 调用 `run_compaction`。
- 成功后 persist `RolloutItem::Compacted`。
- 成功后 save history。
- 成功后 emit `ContextCompacted`。
- 成功后 reset compaction budget baseline。

需要新增 `TurnHost` 方法：

```rust
async fn reset_budget_baseline_after_compaction(
    &self,
    conversation_id: &str,
    sdk_total_tokens: Option<usize>,
    request_estimated_tokens: Option<usize>,
) -> Result<()>;
```

短期如果 baseline 存储还不完整，可以先实现为空，但接口要先建出来。

## 第七步：简化 `execute_regular_turn`

文件：

```text
crates/agent-core/src/turn/regular.rs
```

替换当前压缩块：

```rust
let compaction = maybe_compact_history(...).await?;
if compaction.is_some() {
    emit ContextCompactionStarted;
}
if let Some(compacted) = compaction.as_ref() {
    persist RolloutItem::Compacted;
    save_history;
    emit ContextCompacted;
}
```

改成：

```rust
let compaction = maybe_run_auto_compaction_for_roundtrip(
    host,
    conversation_id,
    turn_id,
    &mut context_manager,
    &cancellation_token,
    roundtrip_count,
    estimated_total_tokens.max(compaction_estimated_total_tokens),
    on_event,
).await?;
```

新增本地小函数或放入 `auto_compact.rs`：

```rust
fn compaction_phase_for_roundtrip(roundtrip_count: usize) -> CompactionPhase {
    if roundtrip_count <= 1 {
        CompactionPhase::PreTurn
    } else {
        CompactionPhase::MidTurn
    }
}
```

`regular.rs` 不再直接知道：

- `ContextCompactionConfig`
- `plan_manual_history_compaction`
- `build_compaction_summary_request`
- `RolloutItem::Compacted` 的字段细节

## 第八步：手动压缩改为 standalone 路径

新增文件：

```text
crates/agent-core/src/turn/manual_compact.rs
```

新增函数：

```rust
pub async fn run_manual_compaction<H: TurnHost>(
    host: &H,
    conversation_id: &str,
    minimum_history_tokens: usize,
) -> Result<ManualCompactionOutcome>;
```

行为：

```text
load history
run_compaction(trigger=Manual, phase=StandaloneTurn, reason=UserRequested)
persist RolloutItem::Compacted
save history
flush rollout
return outcome
```

不要复用 auto 入口。
只复用 `run_compaction` 内核。

## 第九步：重写 rollout reconstruction

新增文件：

```text
crates/agent-core/src/rollout/reconstruction.rs
```

目标函数：

```rust
pub fn reconstruct_model_history_from_rollout(
    conversation_id: &str,
    system_prompt: &str,
    items: &[RolloutItem],
) -> ConversationHistory;
```

替代：

```text
projection/transcript.rs::conversation_history_from_rollout_items
```

反向扫描规则：

```text
1. 从 newest -> oldest 扫描 rollout
2. 找到最新 surviving RolloutItem::Compacted 且 replacement_history 非空
3. 记录 base_replacement_history
4. rollout_suffix = compacted 后面的 items
5. 正向 replay suffix
```

suffix replay 规则：

- `ResponseItem::User` 开启 pending turn。
- `ResponseItem::Assistant` / `Tool` 加入 pending turn。
- `TurnCompleted` flush pending turn。
- `TurnFailed`：
  - 没有模型/工具输出：丢弃 user-only turn。
  - 有模型/工具输出：保留已发生事实，并追加 `<turn_aborted>` 或 `<turn_failed>` marker。
- `TurnCancelled`：
  - 没有模型/工具输出：丢弃 user-only turn。
  - 有模型/工具输出：保留已发生事实，并追加 `<turn_aborted>` marker。
- 遇到新的 `Compacted`：
  - 如果有 replacement_history，直接替换 base。

新增类型：

```rust
struct ReplaySegment {
    turn_id: Option<String>,
    items: Vec<ResponseItem>,
    has_model_output: bool,
    terminal_state: Option<TurnState>,
}
```

迁移：

```text
projection/transcript.rs
  conversation_history_from_rollout_items -> 调用 rollout::reconstruction
```

测试：

```text
reconstruction_starts_from_newest_compaction_checkpoint
reconstruction_replays_completed_suffix_after_compaction
reconstruction_drops_cancelled_user_only_suffix_turn
reconstruction_marks_cancelled_tool_suffix_as_aborted
reconstruction_ignores_older_compaction_checkpoint
legacy_second_system_summary_is_normalized
```

## 第十步：中断时持久化 `<turn_aborted>`

文件：

```text
crates/agent-core/src/turn/orchestrator.rs
crates/agent-core/src/turn/regular.rs
```

新增函数：

```rust
fn record_turn_aborted_marker_if_needed(
    history: &mut ConversationHistory,
) -> Option<ResponseItem>;
```

放到：

```text
crates/agent-core/src/rollout/markers.rs
```

在以下场景调用：

- `execute_regular_turn` 检测 cancellation 并返回 `TurnState::Cancelled` 前。
- `run_turn_with_approval` 捕获 interrupted error 分支内。
- tool batch 返回 cancelled 时。

行为：

```text
如果当前 history 最后一条不是 <turn_aborted>
  push ResponseItem::User(<turn_aborted>...)
  persist RolloutItem::ResponseItem(marker)
  save_history
然后 emit TurnCancelled
```

注意顺序：

```text
Raw marker / ResponseItem
TurnCancelled event
```

Codex 测试里也强调 marker 在 aborted event 前。

测试：

```text
cancelled_turn_persists_turn_aborted_marker_before_cancel_event
cancelled_user_only_turn_can_be_dropped_on_reconstruction
cancelled_tool_turn_keeps_tool_facts_then_marker
```

## 第十一步：自动压缩后 tool-only 循环保护

文件：

```text
crates/agent-core/src/turn/policy.rs
crates/config/src/lib.rs
crates/agent-core/src/turn/regular.rs
```

配置修改：

```rust
max_tool_roundtrips: Some(12)
```

不要默认 `None`。

新增策略：

```rust
pub struct ToolLoopPolicy {
    pub max_roundtrips: usize,
    pub max_tool_only_roundtrips_after_compaction: usize,
}
```

新增计数：

```rust
let mut tool_only_roundtrips_after_compaction = 0usize;
let mut saw_compaction_this_turn = false;
```

在每次 model response 后：

```rust
if saw_compaction_this_turn
    && response.content.as_deref().is_none_or(str::is_empty)
    && !tool_calls.is_empty()
{
    tool_only_roundtrips_after_compaction += 1;
}
```

达到上限后：

```text
emit assistant message:
  "Stopped after automatic compaction because the model continued requesting tools without producing an answer. Please retry or narrow the request."
emit TurnFailed
```

测试：

```text
auto_compaction_tool_only_loop_stops_with_user_visible_error
normal_pre_compaction_tool_loop_uses_general_roundtrip_limit
tool_loop_counter_resets_after_assistant_content
```

## 第十二步：事件和 audit 对齐

文件：

```text
crates/agent-core/src/turn/events.rs
crates/agent-core/src/host/agent.rs
```

扩展 `EventMsg::ContextCompactionStarted`：

```rust
ContextCompactionStarted {
    turn_id: String,
    trigger: CompactionTrigger,
    reason: CompactionReason,
    phase: CompactionPhase,
    estimated_tokens: u64,
}
```

扩展 `EventMsg::ContextCompacted`：

```rust
ContextCompacted {
    turn_id: String,
    trigger: CompactionTrigger,
    reason: CompactionReason,
    phase: CompactionPhase,
    pre_context_tokens_estimate: u64,
    post_context_tokens_estimate: u64,
    pre_message_count: usize,
    post_message_count: usize,
    preserved_user_count: usize,
}
```

短期兼容：

- 保留旧字段 `continuation` 或通过 serde default / alias 兼容旧 UI。
- 新 UI 使用 `phase`。

新增 audit：

```rust
fn audit_compaction_started(...)
fn audit_compaction_completed(...)
fn audit_compaction_failed(...)
```

payload 至少包含：

```json
{
  "trigger": "auto",
  "reason": "context_limit",
  "phase": "pre_turn",
  "pre_message_count": 95,
  "post_message_count": 4,
  "pre_context_tokens": 172415,
  "post_context_tokens": 4982
}
```

## 第十三步：测试目录和命名

不要把大测试继续塞进业务文件。

新增：

```text
crates/agent-core/tests/compaction_checkpoint.rs
crates/agent-core/tests/compaction_reconstruction.rs
crates/agent-core/tests/compaction_turn_abort.rs
crates/agent-core/tests/compaction_tool_loop.rs
```

如果当前 crate 还没有集成测试 helper，可先在：

```text
crates/agent-core/src/turn/compaction/service.rs
crates/agent-core/src/rollout/reconstruction.rs
```

保留少量单元测试，但不要让单文件测试继续膨胀。

核心测试矩阵：

```text
manual standalone compact:
  - trigger=Manual
  - phase=StandaloneTurn
  - DoNotInject
  - 压缩后不继续 regular turn

auto pre-turn compact:
  - trigger=Auto
  - phase=PreTurn
  - DoNotInject
  - 压缩后发送本轮请求

auto mid-turn compact:
  - trigger=Auto
  - phase=MidTurn
  - BeforeLastUserMessage
  - initial context 插到最后真实 user 前

rollout reconstruction:
  - 最新 compaction checkpoint 优先
  - 只 replay checkpoint 后 suffix
  - cancelled user-only turn 丢弃
  - cancelled tool turn 保留事实并追加 <turn_aborted>

tool loop:
  - 压缩后连续 tool-only 响应达到上限后失败
  - 用户能看到失败原因
```

## 迁移顺序

建议按以下提交拆分：

### Commit 1：marker 和 compaction model

内容：

- 新增 `turn/compaction/model.rs`
- 新增 `rollout/markers.rs`
- 迁移 marker 判断函数
- 不改变压缩行为

验证：

```text
cargo test -p agent-core conversation_history_ --lib
cargo check -p cli
```

### Commit 2：planner / summarizer / applier 拆分

内容：

- 新增 `planner.rs`
- 新增 `summarizer.rs`
- 新增 `applier.rs`
- `maybe_compact_history` 改为组合这些模块
- replacement history 改成 Codex 风格

验证：

```text
cargo test -p agent-core compaction --lib
cargo check -p cli
```

### Commit 3：auto / manual 入口拆分

内容：

- 新增 `auto_compact.rs`
- 新增 `manual_compact.rs`
- `regular.rs` 移除压缩细节
- `run_manual_compaction` 改 standalone phase

验证：

```text
cargo test -p agent-core automatic_compaction --lib
cargo test -p agent-core manual_compaction --lib
cargo check -p cli
```

### Commit 4：rollout reconstruction

内容：

- 新增 `rollout/reconstruction.rs`
- `projection/transcript.rs` 调用新 reconstruction
- 补 checkpoint + suffix replay 测试

验证：

```text
cargo test -p agent-core reconstruction --lib
cargo test -p agent-core conversation_history_ --lib
```

### Commit 5：turn aborted 持久化

内容：

- cancellation 分支持久化 `<turn_aborted>`
- marker 在 `TurnCancelled` 前写入 rollout
- reconstruction 使用 marker

验证：

```text
cargo test -p agent-core turn_aborted --lib
cargo check -p cli
```

### Commit 6：tool loop 保护和默认上限

内容：

- 默认 `max_tool_roundtrips: Some(12)`
- 新增压缩后 tool-only roundtrip 上限
- 用户可见错误

验证：

```text
cargo test -p agent-core tool_loop --lib
cargo check -p cli
```

## 验收标准

### 行为验收

- 长会话触发自动压缩后，CLI 能继续收到 assistant 文本或明确失败原因。
- 自动压缩后连续工具调用不会无限 working。
- 用户中断压缩后的 turn，再发消息不会带着无边界的半截 turn 继续。
- 手动 `/compact` 和自动压缩共享内核，但事件 phase 不同。
- 旧 rollout 中第二个 `system [Context Summary]` 能被兼容读取，但新 rollout 不再写这种形状。

### 结构验收

- `regular.rs` 不再直接实现 compaction planning / summarizing / applying。
- `turn/compaction.rs` 不再是压缩全功能大文件。
- `projection/transcript.rs` 不再承担模型历史 reconstruction 的全部职责。
- 压缩相关测试从业务代码中剥离到独立测试文件，或至少按模块就近小范围测试。

### 日志验收

自动压缩日志应能看到：

```text
compaction.started trigger=auto phase=pre_turn reason=context_limit
compaction.completed pre_tokens=... post_tokens=...
model.requested message_count=...
model.responded has_content=... tool_call_count=...
```

如果压缩后 tool-only loop：

```text
turn.failed error="Stopped after automatic compaction because the model continued requesting tools without producing an answer..."
```

## 不做事项

本方案不做：

- 不为某个旧坏会话写专用兼容恢复逻辑。
- 不把压缩摘要继续写成第二个 `system`。
- 不把所有压缩逻辑继续堆进 `regular.rs`。
- 不把测试大段塞进生产业务函数所在文件。
- 不依赖 provider timeout 作为主要稳定性手段。

## 最终目标形态

改完后主路径应接近：

```text
execute_regular_turn
  -> build candidate request
  -> maybe_run_pre_turn_auto_compaction
     -> run_compaction
        -> planner
        -> summarizer
        -> applier
     -> persist checkpoint
  -> send model request
  -> run tools
  -> maybe_run_mid_turn_auto_compaction
  -> complete / failed / cancelled

history_from_rollout
  -> reconstruct_model_history_from_rollout
     -> find newest compaction checkpoint
     -> replay safe suffix
     -> apply turn_aborted markers
```

这就是对齐 Codex 的关键：压缩是明确 checkpoint，不是普通 turn 中间随手改 history。
