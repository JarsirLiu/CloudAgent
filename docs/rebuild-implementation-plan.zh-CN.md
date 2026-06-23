# CloudAgent 重构实施方案

## 0. 使用方式

这份文档是 CloudAgent 后续重构的唯一实施依据。

执行时请遵守三条硬规则：

1. 先改文档里列出的单点，不要临时扩范围
2. 每个实现文件的测试必须独立到对应的 `*_tests.rs`
3. 任何 fallback、兼容分支、特例路径，都必须有明确删除目标

这份文档的目标不是“写得好看”，而是后面改代码时不会忘任务。

## 1. 目标

本次重构的核心目标是把 CloudAgent 的长期架构收口到下面五条主线：

- 协议与历史分层
- 运行态与历史态分层
- 通知与状态栏分层
- 命令与通用工具分层
- 兼容逻辑单点化

最终效果应当是：

- 新能力接入时，只改少量稳定入口
- UI 不再靠字符串猜语义
- 核心逻辑不再被特例撕裂
- 测试与实现分离，模块边界清晰
- 旧路径能删，而不是永久留着

## 2. 当前问题

当前项目不是不能用，而是“语义分裂”和“兼容分散”已经开始影响可维护性。

最主要的风险点如下：

1. 运行态分裂
   - 命令和通用工具仍是两套 active 路径
   - 进度、指标、完成收尾各自有分支

2. 历史展示仍依赖启发式
   - `turn_output` 还在根据 `summary` 文本猜结果类型
   - `web_search` 仍有专门特判

3. 通知职责不够纯
   - reducer 里同时做通知、错误清理、状态清理
   - 状态栏 / toast / 历史卡片的边界还不够硬

4. 兼容逻辑没有完全收口
   - fallback 散在多个文件里
   - 后续新增能力时容易继续长特例

## 3. 设计原则

1. 先定义语义，再定义展示
2. 先收口协议，再收口 UI
3. 一个语义只允许一个权威来源
4. fallback 只能存在单点，而且必须可删除
5. 新能力优先复用已有类型，不先开新特例
6. 大模块只做编排，不做深业务
7. 运行态和历史态必须分层
8. 测试代码和业务代码分离

## 4. 目标架构

### 4.1 协议层

协议层负责描述“发生了什么”，不负责“怎么画”。

稳定保留的内容：

- item started
- item progress
- item metrics
- item patch / delta
- item completed
- turn lifecycle
- server request

协议层只放强类型、可序列化、可测试的数据结构。

### 4.2 运行态层

运行态层负责描述“当前正在发生什么”。

职责：

- 追踪 active item
- 聚合输出、进度、指标、patch
- 为底部状态栏和 active 区提供当前状态
- 支持 restore / replay 时重建现场

运行态层不能直接决定历史卡片长什么样。

### 4.3 历史层

历史层负责描述“最终保留给用户和回放系统的结果”。

职责：

- 保存最终 transcript
- 提供 replay / restore
- 支持搜索、压缩、导出
- 作为稳定摘要，不再承担运行时协议职责

### 4.4 通知层

通知层负责把业务事件路由到不同 UI 语义：

- 状态栏
- toast
- 历史卡片
- modal / popup

通知层只能决定路由，不应该重复解释业务含义。

## 5. 统一对象

### 5.1 Runtime Item

所有运行中的可展示单元统一到 runtime item。

首版应至少包含：

- `id`
- `kind`
- `status`
- `source`
- `title`
- `summary`
- `progress`
- `metrics`

规则：

- 运行态统一从 runtime item 出发
- 历史态统一从 transcript 出发
- UI 不再自己猜“这是什么 item”

### 5.2 Tool Source

工具来源显式区分：

- built-in
- hosted
- mcp
- dynamic

来源只用于元数据和统计，不应决定展示链路是否分裂。

### 5.3 Structured Result

工具完成态应尽量落入结构化结果。

优先保留：

- command execution
- file change / patch
- web search
- mcp tool call
- dynamic tool call

这些结果应服务于统一的历史展示和统计，而不是形成更多特例。

### 5.4 Metrics

metrics 应该是轻量、可扩展、但不臃肿的。

首版只保留真正会被用到的字段，例如：

- elapsed
- tokens
- bytes
- files
- results

不要为了“未来可能用到”一次性把字段塞满。

## 6. 实施边界

### 6.1 这次要做的

- 统一 runtime item 生命周期
- 统一通知路由
- 统一历史 completed 展示
- 收口名称 / summary 启发式 fallback
- 合并命令和通用工具的 active 内核
- 给 gateway 提供稳定的运行态输入

### 6.2 这次不做的

- 完整 diff viewer
- 复杂的多层通知策略引擎
- 过度泛化的通用事件总线
- 把所有未来能力一次性塞入首版 runtime item

这次要做的是“稳定收口”，不是“提前实现一切”。

## 7. 分阶段实施

### Phase 1：收口协议与历史层

目标：

- 统一 runtime item started / progress / metrics / completed
- 删除历史层的专用特例
- 让 completed 历史卡片从结构化结果生成

必须完成的工作：

- 抽出统一的 runtime item 语义
- 收口 `ToolResult`、`CommandExecution`、`FileChange`
- 删除各层对特定工具名的散落判断
- 让历史展示优先吃 structured result

完成标准：

- 新工具接入时不需要在多处加字符串判断
- completed 历史内容都能从同一套结构推导
- 旧特例不再是主路径

### Phase 2：收口运行态内核

目标：

- 统一命令和通用工具的 active 生命周期
- 让 active 区、状态栏、进度条、metrics 共享同一运行态来源

必须完成的工作：

- 合并重复状态机
- 统一 progress / metrics 的更新入口
- 让运行态只由 item kind 决定呈现分流
- 降低“命令一条链、工具一条链”的分裂

完成标准：

- active item 不再依赖多套平行状态机
- command / tool / hosted tool 的行为更一致
- 底部 banner 不再充满临时分支

### Phase 3：收口通知系统

目标：

- 状态栏只做持续状态
- toast 只做短暂反馈
- 历史卡片只做长期留痕
- modal 只做阻塞决策

必须完成的工作：

- 统一 push toast 的入口
- 收敛错误、提示、请求确认的路由
- 去掉把多种语义混在同一字段里的做法

完成标准：

- 一次性提示不会污染状态栏
- 历史卡片不会承担临时错误回显
- 阻塞式弹窗不会和通知混淆

### Phase 4：收口 gateway 展示

目标：

- gateway 拿到稳定的 runtime item 事件后，能做轻量富展示
- 不要求一开始就和 CLI 一样丰富，但输入形状要稳定

必须完成的工作：

- 统一 progress 文案
- 统一 metrics 文案
- 统一安全降级策略

完成标准：

- gateway 不再只是日志/typing 适配层
- 各 adapter 共享统一的运行态文本生成逻辑

### Phase 5：删除兼容分支

目标：

- 删除已经被新模型覆盖的旧路径
- 让 fallback 变成单点、低频、可逐步消失的东西

必须完成的工作：

- 去掉分散的名称启发式
- 去掉历史层对旧特例的硬编码
- 去掉不再必要的兼容状态

完成标准：

- 旧逻辑不再是默认路径
- fallback 只在少数点存在
- 每个删除都能被测试覆盖

## 8. 文件级实施方案

### 8.1 `crates/agent-core/src/runtime_item.rs`

职责：

- runtime item 单点定义
- item kind / status / source / metrics / progress 的基础模型

需要做的修改：

- 保持 `RuntimeItem` 为核心数据结构
- 收紧首版字段，只保留必要字段
- 保持 `RuntimeItemMetrics` 为轻量结构

建议保留的方法：

- `RuntimeItem::started(...)`
- `RuntimeItem::completed(...)`
- `RuntimeItem::with_progress(...)`
- `RuntimeItem::with_metrics(...)`

原则：

- 不要继续扩大成“所有能力的大一统枚举”
- 只加入当前已经稳定消费的字段

测试文件：

- `crates/agent-core/src/runtime_item_tests.rs`

### 8.2 `crates/agent-core/src/web_search_presentation.rs`

职责：

- web search 的文案和摘要逻辑单点化

需要做的修改：

- 保留统一的 web search summary/detail helper
- 供 core / app-server / cli 共用
- 不要让多个层各自实现一套文案

建议保留或新增的方法：

- `web_search_detail(...)`
- `web_search_summary(...)`
- `started_runtime_item(...)`
- `completed_runtime_item(...)`

测试文件：

- `crates/agent-core/src/web_search_presentation_tests.rs`

### 8.3 `crates/agent-core/src/projection/transcript.rs`

职责：

- 历史态投影单点

需要重点检查的函数：

- `transcript_item_from_item_start(...)`
- `transcript_item_from_tool_response(...)`
- `transcript_item_is_empty(...)`
- `upsert_completed_turn_item(...)`
- `append_delta_to_completed_turn(...)`
- `append_delta_to_transcript_item(...)`

需要做的修改：

- 保持 transcript 作为历史摘要，不承载运行协议职责
- `web_search` 只能通过统一 structured result 表达
- 删除和收敛所有依赖字符串猜测的逻辑

测试文件：

- `crates/agent-core/src/projection/transcript_tests.rs`

### 8.4 `crates/agent-core/src/projection/turn_output.rs`

职责：

- 收口 turn output 的分类启发式

需要重点修改的逻辑：

- `TranscriptItem::ToolResult` 的分类逻辑
- `StructuredToolResult::WebSearch` 的专用分支
- 依赖 `summary.to_lowercase()` 的错误猜测

当前问题：

- 还在根据 `summary` 文本判断错误态
- 还在对 `web_search` 做单独特判

需要做的修改：

- 将 `web_search` 专用处理收敛到单一 helper
- `summary` 只能做最后 fallback
- 分类逻辑从“字符串推断”转为“结构化优先”

测试文件：

- `crates/agent-core/src/projection/turn_output_tests.rs`

### 8.5 `crates/agent-app-server/src/projection/conversation_notifications.rs`

职责：

- app-server 通知投影单点

需要重点修改的函数：

- `project_turn_event(...)`
- `project_core_transcript_event(...)`
- `observe_item_started(...)`
- `observe_item_progress(...)`
- `observe_item_metrics_updated(...)`

需要做的修改：

- started / completed 直接使用 runtime item
- 不再通过多层 fallback 推断 started item
- 通知只做投影，不重新解释业务

测试文件：

- `crates/agent-app-server/src/projection/conversation_notifications_tests.rs`

### 8.6 `crates/agent-app-server/src/projection/transcript_item_projection.rs`

职责：

- 只负责 transcript/history 映射

需要重点修改的函数：

- `projected_item_from_transcript_item(...)`
- `projected_item_to_transcript_item(...)`
- `turn_item_kind_for_transcript_item(...)`
- `projected_transcript_item_is_empty(...)`

需要做的修改：

- 只保留历史映射职责
- 收掉 `web_search` 特例的本地判断
- 转而依赖统一 structured result

测试文件：

- `crates/agent-app-server/src/projection/transcript_item_projection_tests.rs`

### 8.7 `cli/src/state/reducer.rs`

职责：

- reducer 只做消息翻译

需要重点修改的函数：

- `apply_server_message(...)`
- `summarize_args_preview(...)`
- `transport_closed_message(...)`

需要处理的动作：

- `PushNoticeCell`
- `PushErrorCell`
- `ClearActiveTool`
- `ContextCompacted`
- `Error` 中的特殊前缀识别

需要做的修改：

- 降低 reducer 内部的业务判断密度
- 将通知、错误、状态清理拆成更稳定的 helper
- 尽量把“要做什么”与“为什么做”分离

测试文件：

- `cli/src/state/reducer_tests.rs`

### 8.8 `cli/src/state/bottom_pane_runtime.rs`

职责：

- active runtime 内核

当前结构问题：

- `ActiveToolRuntimeState` 里有 `Command` 和 `Tool`
- `BottomPaneRuntimeState` 仍依赖不同路径处理 command / tool

需要重点修改的函数：

- `BottomPaneRuntimeState::reset(...)`
- `BottomPaneRuntimeState::on_turn_started(...)`
- `BottomPaneRuntimeState::on_tool_finished_for_item(...)`
- `BottomPaneRuntimeState::on_context_compaction_started(...)`
- `BottomPaneRuntimeState::on_context_compaction_finished(...)`
- `BottomPaneRuntimeState::on_turn_finished(...)`
- `BottomPaneRuntimeState::on_model_retrying(...)`
- `BottomPaneRuntimeState::on_active_item_started(...)`
- `BottomPaneRuntimeState::on_command_output_delta(...)`
- `BottomPaneRuntimeState::on_command_finished(...)`
- `BottomPaneRuntimeState::on_tool_output_delta(...)`
- `BottomPaneRuntimeState::on_item_progress(...)`
- `BottomPaneRuntimeState::on_item_metrics_updated(...)`

需要做的修改：

- 把 command/tool 的公共字段抽到统一的 active runtime 结构
- 保留 command 的输出特性，但避免生命周期分裂
- 让 progress / metrics / output 更新通过同一入口

测试文件：

- `cli/src/state/bottom_pane_runtime_tests.rs`
- `cli/src/state/bottom_pane_controller_tests.rs`

### 8.9 `cli/src/state/bottom_pane_controller.rs`

职责：

- bottom pane 和状态栏的编排层

需要重点修改的结构和函数：

- `StatusViewModel`
- `BottomPaneController::build_status_view_model(...)`
- `BottomPaneController::runtime_banner_text(...)`
- `BottomPaneController::push_toast(...)`

需要做的修改：

- 状态栏只保留持续状态
- `live_banner` 作为过渡字段应逐步收敛
- toast 的推送入口保持单点

测试文件：

- `cli/src/state/bottom_pane_controller_tests.rs`

### 8.10 `cli/src/app/core/active_turn.rs`

职责：

- active cell 编排

需要重点修改的函数：

- `ActiveItemView::new(...)`
- `ActiveTurnState::reduce(...)`
- `copyable_output(...)`
- `completed_agent_text(...)`
- `turn_item_kind(...)`
- `should_keep_completed_item_live(...)`

当前问题：

- `StartItem` 对 `CommandExecution` 仍然有提前返回
- `ToolResult` 的完成保持逻辑仍然分散

需要做的修改：

- 命令和通用工具尽量走统一 active 内核
- 保留 command 的特殊输出能力，但不单独拥有一条生命周期链
- 历史恢复和 live tail 逻辑保持一致

测试文件：

- `cli/src/app/core/running_turn_restore_tests.rs`
- `cli/src/app/core/transcript_projection_tests.rs`

### 8.11 `cli/src/app/conversation/actions/server_actions.rs`

职责：

- server action 执行单点

需要重点修改的函数：

- `execute_server_action(...)`
- `prepend_turn_page(...)`

当前问题：

- 对 `TranscriptItem::CommandExecution` 仍有收尾分支
- command/tool 的完成处理还没有完全统一

需要做的修改：

- 收尾逻辑通过统一 helper 处理
- 不要在 action 层重复解释 item 语义

测试文件：

- `cli/src/app/tests.rs`

### 8.12 `cli/src/ui/history_cell/tool_ui.rs`

职责：

- 历史工具卡片渲染入口

需要重点修改的函数：

- `render_command_execution(...)`
- `render_tool_result(...)`
- `humanize_tool_label(...)`

需要做的修改：

- `render_tool_result` 优先吃 structured result
- `humanize_tool_label` 只作为最后 fallback
- command / tool / web search 的展示逻辑尽量从统一 helper 取得

测试文件：

- `cli/src/ui/history_cell/render_entry_tests.rs`
- `cli/src/ui/history_cell/search_tests.rs`
- `cli/src/ui/history_cell/command_tests.rs`

### 8.13 `cli/src/ui/history_cell/render.rs`

职责：

- 历史渲染主入口

需要重点修改的函数：

- `render_history_entry(...)`
- `render_active_runtime_item(...)`

需要做的修改：

- 历史区主要依赖 completed item 和 structured result
- active 区只使用 runtime item
- 不把分类逻辑留在主渲染入口里

测试文件：

- `cli/src/ui/history_cell/render_entry_tests.rs`

### 8.14 `cli/src/ui/history_cell/search.rs`

职责：

- web search 历史展示单点

需要重点修改的函数：

- `web_search_action_detail(...)`
- `web_search_detail(...)`
- `render_tool_result(...)`
- `render_active_runtime_item(...)`

需要做的修改：

- web search 文案只保留一处 helper
- completed 与 active 的展示共享逻辑

测试文件：

- `cli/src/ui/history_cell/search_tests.rs`

### 8.15 `cli/src/app/runtime/controller.rs`

职责：

- 运行时边界控制

需要重点修改的函数：

- `should_stop_after_event_boundary(...)`
- `is_runtime_render_boundary_item(...)`

需要做的修改：

- 逐步消除基于具体工具名或具体 transcript 变体的边界判断
- 边界逻辑只保留必要兼容

测试文件：

- `cli/src/app/runtime/controller_tests.rs`

### 8.16 `crates/agent-gateway/src/adapter/weixin/runtime.rs`

职责：

- 微信适配器运行态输出

需要重点修改的函数：

- `PlatformRuntime::...`
- `notification_turn_id(...)`
- `event_name(...)`
- `log_outbounds(...)`

需要做的修改：

- progress / metrics 文案通过共享 helper 生成
- adapter 只保留平台差异

测试文件：

- `crates/agent-gateway/src/adapter/weixin/runtime_tests.rs`

### 8.17 `crates/agent-gateway/src/adapter/wecom/runtime.rs`

职责：

- 企业微信适配器运行态输出

需要重点修改的函数：

- `PlatformRuntime::...`
- `notification_turn_id(...)`
- `build_turn_content(...)`
- `render_request_prompt(...)`

需要做的修改：

- 共享 progress / metrics 文案 helper
- 避免平台之间出现各自独立的运行态解释逻辑

测试文件：

- `crates/agent-gateway/src/adapter/wecom/runtime_tests.rs`

### 8.18 `crates/agent-gateway/src/adapter/feishu/runtime.rs`

职责：

- 飞书适配器运行态输出

需要重点修改的函数：

- `PlatformRuntime::...`
- `notification_turn_id(...)`
- `render_request_prompt(...)`
- `build_approval_card(...)`

需要做的修改：

- 运行态文案共享 helper
- 复杂卡片逻辑保持平台特有，但不要混入业务解释

测试文件：

- `crates/agent-gateway/src/adapter/feishu/runtime_tests.rs`

## 9. 测试要求

### 9.1 原则

- 测试代码不要混在业务代码主文件里
- 新增测试优先放到独立 `*_tests.rs`
- 已有测试模块继续保留在其对应的测试文件中

### 9.2 测试分布

建议明确保留或新增以下测试文件：

- `crates/agent-core/src/runtime_item_tests.rs`
- `crates/agent-core/src/projection/transcript_tests.rs`
- `crates/agent-core/src/projection/turn_output_tests.rs`
- `crates/agent-app-server/src/projection/conversation_notifications_tests.rs`
- `crates/agent-app-server/src/projection/transcript_item_projection_tests.rs`
- `cli/src/state/reducer_tests.rs`
- `cli/src/state/bottom_pane_controller_tests.rs`
- `cli/src/state/bottom_pane_runtime_tests.rs`
- `cli/src/app/tests.rs`
- `cli/src/app/core/running_turn_restore_tests.rs`
- `cli/src/app/runtime/controller_tests.rs`
- `cli/src/ui/history_cell/render_entry_tests.rs`
- `cli/src/ui/history_cell/search_tests.rs`
- `cli/src/ui/history_cell/command_tests.rs`
- `crates/agent-gateway/src/adapter/weixin/runtime_tests.rs`
- `crates/agent-gateway/src/adapter/wecom/runtime_tests.rs`
- `crates/agent-gateway/src/adapter/feishu/runtime_tests.rs`

### 9.3 测试必须锁住的行为

- started item 进入 active 的方式
- completed item 的历史收口方式
- command / tool 的 active 生命周期一致性
- web search 的结构化展示方式
- toast / notice / modal 的路由方式
- fallback 只在单点触发

## 10. 迁移顺序

### 10.1 第一轮

先做三个最有收益的收口：

1. 收口 `turn_output` 的启发式 fallback
2. 收口 `bottom_pane_runtime` 的命令 / 工具分裂
3. 收口 `reducer` 的通知和错误分流

### 10.2 第二轮

再做四个结构性整理：

1. 收口 `transcript.rs`
2. 收口 `conversation_notifications.rs`
3. 收口 `history_cell` 的渲染入口
4. 收口 `runtime_controller` 的边界逻辑

### 10.3 第三轮

最后处理 gateway 和残留兼容：

1. 统一 gateway 文案 helper
2. 删除旧 fallback
3. 删除不再必要的过渡字段

## 11. 风险控制

这次重构最容易做坏的地方有三个：

1. Runtime item 过度膨胀，变成新的超级枚举
2. fallback 没有真正删除，最后变成永久兼容层
3. 通知系统抽象过重，反而增加理解成本

所以重构时要坚持：

- 首版只放真正要用的字段
- 每一类 fallback 都有明确归属
- 抽象要服务于减少复杂度，而不是制造复杂度

## 12. 完成定义

当下面这些条件满足时，这次重构才算完成：

- 运行态、历史态、通知层的职责都能一句话说清
- command / tool / hosted tool 的 active 生命周期收敛到同一内核
- `turn_output` 不再依赖散落的 summary 猜测
- `web_search` 的展示和摘要只保留单点 helper
- reducer 里不再堆积通知语义
- 所有新增测试都在独立测试文件中
- 旧特例路径可以被删除，而不是继续保留

## 13. 最终建议

这次重构建议按“先收口，再增强”的顺序推进：

1. 先收口协议和历史层
2. 再收口运行态和通知系统
3. 再统一 gateway 和 UI 展示
4. 最后删除旧兼容分支

不要反过来做：

- 先在 UI 上继续加补丁
- 再在 reducer 里继续加特判
- 最后让 core 模型被迫跟着补

这样会越修越散。

如果你要参照 Codex 学架构，最值得学的不是“它功能多”，而是：

- 边界清楚
- 语义单一
- 迁移可删除
- 兼容可定位
- 大模块不再继续膨胀

