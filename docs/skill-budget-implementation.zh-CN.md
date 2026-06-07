# Skill 预算与截断实施方案（CloudAgent）

> 目标：把 skill 的发现、选择、渲染、预算、截断拆成清晰边界，保证长期稳定、可维护、可扩展。

## 1. 背景与目标

CloudAgent 当前已经具备 skill 发现、catalog 渲染、显式 skill 注入和上下文预算能力，但 skill 预算仍然和 memory / mcp 的预算逻辑集中在一起，策略边界不够清晰。

本方案的目标是：

- skill 的“知识内容”与“预算策略”解耦
- skill 作为独立预算分支处理
- 显式 skill 与隐式 skill 的上下文策略分开
- 预算器只消费结构化输入，不直接读文件系统
- 测试文件完全抽离，不混入业务代码文件

## 2. 结论：应该怎么做

建议采用：**在预算器里把 skill 单独拿出来处理**，而不是在 skill 加载阶段提前做永久压缩或截断。

原因如下：

- skill 文件应保持原始、完整、可审计
- 预算策略属于运行时决策，不应写回 skill 内容
- 预算器统一管理 memory / skill / mcp，结构更稳定
- 后续扩展新 bucket 时不需要重构 skill 加载逻辑

不建议：

- 在 `SKILL.md` 创建阶段直接写入压缩版内容
- 在 `SkillRuntime` 里做 token 级预算裁剪
- 把 skill 的运行时策略混进 skill 定义文件

## 3. 当前代码结构与职责边界

### 3.1 `crates/agent-core/src/skill/runtime.rs`

职责：

- skill 根目录发现
- skill catalog 加载
- 显式 skill 文档加载
- `$skill-name` 与结构化 skill 附件匹配

现有关键函数：

- `SkillRuntime::load_catalog`
- `SkillRuntime::watch_roots`
- `SkillRuntime::collect_turn_explicit_skill_documents`
- `SkillRuntime::skill_roots`
- `SkillRuntime::collect_skills_from_root`
- `SkillRuntime::ensure_system_skills`

结论：这里**不应该**承担预算策略。

### 3.2 `crates/agent-core/src/skill/render.rs`

职责：

- 将 skill catalog 渲染为 prompt 友好的摘要
- 将 skill 文档渲染为注入片段
- 提供最小的渲染辅助函数

现有关键函数：

- `render_skill_catalog`
- `render_skill_injection`
- `latest_user_items`

结论：这里应继续保持“纯渲染”，不做预算决策。

### 3.3 `crates/agent-core/src/context/budget.rs`

职责：

- 构造预算后的上下文 fragments
- 同时处理 memory / skills / mcp bucket
- 基于上下文窗口做 token 预算与截断

现有关键类型与函数：

- `MemoryBudgetSource`
- `BucketAudit`
- `BudgetedFragments`
- `build_memory_budgeted_fragments`
- `fit_bucket`
- `estimate_text_tokens`

结论：这里就是 skill 预算的主落点，但需要把 skill 分支进一步独立成清晰模块。

### 3.4 `crates/agent-core/src/turn/regular.rs`

职责：

- 组装本 turn 的上下文
- 获取 skill catalog
- 生成 skill summary
- 收集显式 skill 注入
- 调用预算器
- 生成最终 model request

现有关键位置：

- `render_skill_catalog(&skill_catalog.skills_allowed_for_implicit_invocation())`
- `collect_turn_explicit_skill_documents(...)`
- `build_budgeted_fragments_for_current_history(...)`

结论：这里应只做编排，不承载复杂策略。

## 4. 总体设计

### 4.1 分层模型

建议将 skill 处理拆成四层：

1. **发现层**：找出本地 skill
2. **选择层**：判断本 turn 需要哪些 skill
3. **渲染层**：把 skill 转成 prompt 可消费内容
4. **预算层**：决定是否进入上下文、进入多少、是否截断

### 4.2 处理原则

- skill 文件内容不变
- skill 预算只在 request 构造阶段发生
- catalog summary 与 explicit skill body 分开处理
- 预算器不直接访问文件系统
- 运行时策略只在预算器与 turn 编排中出现

## 5. 具体实施方案

## 5.1 `crates/config/src/lib.rs`

### 目标

把 skill budget 作为显式配置项保留下来，方便调参和稳定演进。

### 现有字段

- `enable_skill_bucket`
- `post_compact_skills_token_budget`
- `post_compact_max_tokens_per_skill`

### 建议动作

1. 保留现有字段，不破坏兼容
2. 如果后续要细分预算，可再加入以下字段：
   - `post_compact_skill_summary_budget_tokens`
   - `post_compact_skill_body_budget_tokens`
   - `post_compact_skill_item_floor_tokens`
   - `post_compact_skill_explicit_priority_boost`

### 需要关注的函数

- `AgentConfig::defaults`
- `AgentConfig::apply_partial`

### 建议默认值

- `enable_skill_bucket = false` 或维持当前默认
- `post_compact_skills_token_budget = 25_000`
- `post_compact_max_tokens_per_skill = 5_000`

## 5.2 `crates/agent-core/src/skill/model.rs`

### 目标

保持 skill 模型纯净，只放结构体与枚举，不写预算逻辑。

### 保留内容

- `SkillInvocationMode`
- `SkillScope`
- `SkillMetadata`
- `SkillDependencies`
- `SkillDocument`
- `SkillCatalog`

### 不建议放入

- token 预算逻辑
- 截断策略
- 渲染策略

## 5.3 `crates/agent-core/src/skill/runtime.rs`

### 目标

只负责“发现、匹配、加载”，不负责预算。

### 建议新增函数

#### `collect_turn_skill_candidates(...)`

返回本 turn 可能涉及的 skill metadata 列表，只输出结构化候选项，不输出全文。

用途：

- 供 turn 编排层和预算层使用
- 让预算器可以按 metadata 做更细粒度决策

#### `load_skill_documents_for_explicit_use(...)`

如果后续需要对 explicit skill 做更精确的批量加载，可引入此类辅助函数，但仍应保持“加载职责”而非“预算职责”。

### 现有函数建议保持

- `load_catalog`
- `collect_turn_explicit_skill_documents`
- `skill_roots`
- `collect_skills_from_root`
- `ensure_system_skills`

### 匹配行为建议

保留现有规则：

- 结构化 `InputItem::Skill` 优先
- `$skill-name` 明确 mention 触发
- plain text 描述匹配只用于隐式候选
- explicit 与 implicit 的边界要清楚

## 5.4 `crates/agent-core/src/skill/render.rs`

### 目标

只做 prompt 友好的文本渲染，不做调度和预算判断。

### 现有函数

- `render_skill_catalog`
- `render_skill_injection`
- `latest_user_items`

### 建议新增函数

#### `render_skill_budget_summary(skills: &[SkillMetadata]) -> Option<String>`

用途：输出更短、更规整、预算友好的 skill summary。

#### `render_skill_summary_item(skill: &SkillMetadata) -> String`

用途：让预算器可以逐项渲染与截断。

#### `render_truncated_skill_injection(document: &SkillDocument, max_chars: usize) -> ResponseItem`

用途：为预算受限时的 explicit skill body 提供受控截断入口。

### 设计原则

- summary 和 body 分离
- 渲染函数不判断是否需要使用某个 skill
- 渲染函数不读取任何配置

## 5.5 `crates/agent-core/src/context/budget.rs`

### 目标

把 skill 预算逻辑做成独立分支，和 memory / mcp 一样可控。

### 当前主入口

- `build_memory_budgeted_fragments(...)`

### 建议的内部结构

建议在同一个文件内按大段落组织：

- memory bucket
- skill bucket
- mcp bucket
- truncation helpers
- token estimation helpers

这样能满足“单文件承载业务逻辑”的要求，同时保持模块边界清晰。

### 建议新增结构

#### `SkillBudgetSource`

用于将 skill 预算相关输入收敛成一个结构体，例如：

```rust
pub struct SkillBudgetSource {
    pub summary: Option<String>,
    pub enable_bucket: bool,
    pub post_compact_budget_tokens: usize,
    pub max_tokens_per_item: usize,
}
```

如果后续要把 explicit skill body 也纳入预算，可再扩展为：

```rust
pub struct SkillBudgetSource {
    pub summary: Option<String>,
    pub explicit_documents: Vec<String>,
    pub enable_bucket: bool,
    pub post_compact_budget_tokens: usize,
    pub max_tokens_per_item: usize,
}
```

### 建议新增审计结构

#### `SkillBucketAudit`

```rust
pub struct SkillBucketAudit {
    pub before: usize,
    pub after: usize,
    pub truncated: bool,
    pub kept_items: usize,
}
```

用于追踪 skill bucket 的裁剪行为。

### 建议新增函数

#### `budget_skill_bucket(...)`

职责：

- 计算剩余 token
- 判断 skill bucket 是否启用
- 对 skill summary / body 进行截断
- 返回可注入 fragments 和审计结果

#### `truncate_skill_context(...)`

职责：

- 处理字符串级或 section 级截断
- 保留标题、说明、触发规则等高价值内容

### 现有函数建议保留

- `fit_bucket(...)`
- `estimate_text_tokens(...)`

### 改进建议

当前 `fit_bucket(...)` 是粗粒度字符截断，长期可用，但不够语义友好。建议后续升级为：

1. 优先保留结构化 section
2. 再按 token 预算截断正文
3. 如果空间不足，优先保留 trigger / usage / policy 信息

## 5.6 `crates/agent-core/src/turn/regular.rs`

### 目标

把 turn 层保持成编排层，避免混入复杂预算策略。

### 当前关键逻辑

```rust
let skill_catalog = skill_runtime.load_catalog(&settings.workspace_root);
let skill_summary = render_skill_catalog(&skill_catalog.skills_allowed_for_implicit_invocation());
let turn_explicit_skill_fragments = skill_runtime
    .collect_turn_explicit_skill_documents(&context_manager.history().messages, &skill_catalog)
    .into_iter()
    .map(|document| render_skill_injection(&document))
    .collect::<Vec<_>>();
```

### 建议改造

将这段逻辑封装为一个纯编排函数：

#### `build_turn_skill_context(...)`

返回：

```rust
pub struct TurnSkillContext {
    pub catalog_summary: Option<String>,
    pub explicit_fragments: Vec<ResponseItem>,
}
```

这样 `execute_regular_turn(...)` 只负责：

- 拿数据
- 调预算器
- 组 request

而不是自己决定 skill 策略。

## 6. 推荐代码文件级落点

### `crates/agent-core/src/context/budget.rs`

负责：

- 总预算入口
- memory bucket
- skill bucket
- mcp bucket
- 截断 helper

### `crates/agent-core/src/skill/runtime.rs`

负责：

- skill discovery
- skill catalog load
- explicit skill matching
- explicit skill document loading

### `crates/agent-core/src/skill/render.rs`

负责：

- skill catalog summary render
- skill injection render
- budget-friendly summary render

### `crates/agent-core/src/turn/regular.rs`

负责：

- turn context 编排
- 调用 skill runtime
- 调用 budgeter
- 组装最终请求

### `crates/config/src/lib.rs`

负责：

- skill budget 默认值
- 配置覆盖
- 运行时参数收敛

## 7. 测试文件拆分方案

> 要求：测试文件独立，不混在业务代码文件中。

### 7.1 `crates/agent-core/src/context/budget/tests.rs`

测试点：

- skill bucket 开关是否生效
- skill budget 是否受总预算限制
- skill 截断是否正确记录 audit
- memory / skill / mcp 三者的预算顺序是否符合预期

### 7.2 `crates/agent-core/src/skill/runtime/tests.rs`

测试点：

- skill root discovery
- `$skill-name` 匹配
- plain text description 匹配
- explicit 与 implicit 的优先级
- skill 不跨 turn 泄漏

### 7.3 `crates/agent-core/src/skill/render/tests.rs`

测试点：

- catalog 输出格式稳定
- summary 是否包含 name / description / path
- injection 格式是否正确
- budget summary 是否比完整 catalog 更短

### 7.4 `crates/agent-core/src/turn/regular/tests.rs`

测试点：

- explicit skill 只在当前 turn 注入
- compaction 后 skill context 是否重新评估
- repeated turn 下 skill 不应无条件继承

## 8. 推荐实施顺序

### Phase 1：结构整理

- 保持行为不变
- 将 skill bucket 逻辑从混合结构中整理出来
- 让 budget.rs 里的 skill 分支清晰化

### Phase 2：渲染拆分

- 增加 budget-friendly skill summary
- 明确 summary / body 分离

### Phase 3：预算细化

- 引入 skill 专属 audit
- 让 explicit skill 优先级更明确
- 让截断策略更语义化

### Phase 4：测试锁定

- 完善各层测试
- 固化 skill 不跨 turn 泄漏的行为
- 固化 budget 截断边界

## 9. 风险与约束

### 风险 1：预算与渲染边界混淆

解决：渲染函数只渲染，预算器只决策。

### 风险 2：skill 内容被永久改写

解决：不在创建和加载阶段做持久化截断。

### 风险 3：预算字段不断膨胀

解决：先保留现有字段，必要时再逐步细分。

### 风险 4：turn 层逻辑过重

解决：把 skill 组装抽成 `build_turn_skill_context(...)`。

## 10. 最终结论

这套方案的核心判断是：

> **skill 不应提前做永久压缩，而应在预算器里作为独立分支处理。**

这样可以同时满足：

- 长期稳定
- 结构干净
- 职责解耦
- 易于测试
- 易于扩展

如果后续要落地代码，推荐优先改：

1. `crates/agent-core/src/context/budget.rs`
2. `crates/agent-core/src/turn/regular.rs`
3. `crates/agent-core/src/skill/render.rs`
4. `crates/agent-core/src/skill/runtime.rs`
5. `crates/config/src/lib.rs`

然后再补测试文件。
