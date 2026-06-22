# 通知系统实施方案

## 1. 目标

把 CloudAgent CLI 里的提示拆成稳定的语义层次，对齐 Codex 的边界，但保留 CloudAgent 现有的工程结构与命名习惯。

我们要的不是“一个更大的 banner”，而是四个职责明确的输出层：

- 常驻状态：持续运行态、执行态、压缩态
- 短暂通知：几秒自动消失的轻量提示
- 常驻提示卡片：需要保留在历史区、但不阻塞用户继续输入
- 阻塞式操作框：必须用户处理后才能继续

## 2. 现状

当前仓库已经把一部分边界拆出来了：

- `BottomPaneRuntimeState` 只负责持续运行态
- `NotificationStore` 已经独立承载 toast
- `ChatSurface` 已经分开渲染 status area 和 toast area
- `InputPane` 继续负责 modal / popup / picker
- 常驻状态、短暂通知、阻塞式操作框的语义边界已经开始分层

但还有一些职责没有完全收紧：

- 业务层仍然在很多地方直接发 toast 字符串
- `StatusViewModel` 还带着过渡性质字段
- `ServerAction::PushNoticeCell` 仍然偏“直接显示文本”，不是结构化通知
- `history_cell` 里的 notice 渲染还带有通用兜底色彩
- 常驻提示卡片还没有完全独立成自己的历史语义分支

## 3. 对照 Codex 的结论

Codex 的边界很稳定：

- 历史卡片负责长期可回看的语义内容
- 短暂通知只负责轻量提醒，且会去重、限时、按优先级合并
- 权限 / 确认类交互走阻塞弹窗，不混进历史卡片
- 状态栏只承载持续状态，不承载所有提示

对应源码参考：

- [codex-rs/tui/src/history_cell/notices.rs](D:/Software/Projects/codex-main/codex-rs/tui/src/history_cell/notices.rs)
- [codex-rs/tui/src/history_cell/base.rs](D:/Software/Projects/codex-main/codex-rs/tui/src/history_cell/base.rs)
- [codex-rs/tui/src/chatwidget/notifications.rs](D:/Software/Projects/codex-main/codex-rs/tui/src/chatwidget/notifications.rs)
- [codex-rs/tui/src/chatwidget/permission_popups.rs](D:/Software/Projects/codex-main/codex-rs/tui/src/chatwidget/permission_popups.rs)
- [codex-rs/tui/src/bottom_pane/approval_overlay.rs](D:/Software/Projects/codex-main/codex-rs/tui/src/bottom_pane/approval_overlay.rs)

## 4. 设计原则

1. 语义优先于展示
2. 每种提示只负责一种展示形态
3. 状态栏只显示持续状态
4. 短暂通知只负责快速反馈
5. 常驻提示卡片只进入历史区，不参与实时状态栏竞争
6. 阻塞式操作框只负责明确的用户决策
7. UI 层不要反向猜测后端语义
8. 所有提示都要有稳定来源和稳定 key
9. 避免一个字段承接多种语义
10. 迁移要渐进，先拆边界，再替换旧逻辑

## 5. 目标架构

### 5.1 四层模型

#### A. Notification Event

只描述“发生了什么”，不描述怎么画。

示例：

- `ConversationInfo`
- `ToolWarning`
- `TransportError`
- `ApprovalRequested`
- `ContextCompactionStarted`
- `ContextCompacted`
- `SetupFollowUpRequired`

#### B. Notification Policy

只决定“应该进入哪种 UI 层”。

示例：

- `Status`
- `Toast`
- `Card`
- `Modal`

#### C. Notification Store

统一保存当前可见提示与历史提示，负责：

- 去重
- TTL
- sticky / pinned
- 过期
- ack / dismiss
- 排队

#### D. Presentation Layer

只负责画界面：

- `status banner`
- `toast`
- `history card`
- `modal`

## 6. 当前映射

### 6.1 常驻状态

适合：

- `Working`
- `Thinking`
- `reconnecting (...)`
- `Compacting context (~n tokens)`
- `running command: ...`
- `executing tool: ...`

现有落点：

- [cli/src/state/bottom_pane_runtime.rs](D:/Software/Projects/CloudAgent/cli/src/state/bottom_pane_runtime.rs)
- [cli/src/state/bottom_pane_controller.rs](D:/Software/Projects/CloudAgent/cli/src/state/bottom_pane_controller.rs)
- [cli/src/ui/chat_surface.rs](D:/Software/Projects/CloudAgent/cli/src/ui/chat_surface.rs)

### 6.2 短暂通知

适合：

- `interrupt requested`
- `no active turn`
- 一次性错误
- 一次性 info / warn

现有落点：

- [cli/src/state/notification.rs](D:/Software/Projects/CloudAgent/cli/src/state/notification.rs)
- [cli/src/state/notification_store.rs](D:/Software/Projects/CloudAgent/cli/src/state/notification_store.rs)
- [cli/src/app/conversation/actions/local_basic_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/local_basic_actions.rs)
- [cli/src/app/conversation/actions/local_command_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/local_command_actions.rs)
- [cli/src/app/conversation/actions/local_gateway_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/local_gateway_actions.rs)
- [cli/src/app/conversation/event_router.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/event_router.rs)

### 6.3 常驻提示卡片

适合：

- setup follow-up
- import follow-up
- 长期可见的 warning
- 需要用户稍后处理的 error
- Completed item 的结构化摘要

现有落点：

- [cli/src/ui/history_cell/notice_cards.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/notice_cards.rs)
- [cli/src/ui/history_cell/display.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/display.rs)
- [cli/src/ui/history_cell/transcript_cards.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/transcript_cards.rs)

### 6.4 阻塞式操作框

适合：

- 权限确认
- server request
- model picker
- session picker
- gateway / binding 对话框

现有落点：

- [cli/src/ui/bottom_pane/input_pane/mod.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/input_pane/mod.rs)
- [cli/src/ui/bottom_pane/dialogs/server_request/server_request_overlay.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/server_request/server_request_overlay.rs)
- [cli/src/ui/bottom_pane/dialogs/selection/model_picker.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/selection/model_picker.rs)
- [cli/src/ui/bottom_pane/dialogs/selection/session_picker.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/selection/session_picker.rs)
- [cli/src/ui/bottom_pane/dialogs/config_panel.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/config_panel.rs)
- [cli/src/ui/bottom_pane/dialogs/gateway_panel/mod.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/gateway_panel/mod.rs)

## 7. 具体实施方案

### Phase 1: 先拆语义，不拆视觉

#### 目标

先把“是什么提示”从“怎么显示”里拆出来。

#### 需要做的事

1. 新增或完善统一通知类型
   - 位置：`cli/src/state/`
   - 建议文件：`notification.rs`、`notification_store.rs`
   - 目标：让 `Toast` 成为独立概念，而不是 runtime 的附属字段

2. 保留 `NoticeLevel` 作为展示等级
   - 只负责 `Info / Warn / Error`
   - 不再承担“这条消息该进哪种 UI”的判断

3. 引入统一的通知事件枚举
   - 只表达业务上发生了什么
   - 不直接携带渲染细节

4. 引入通知策略层
   - 把 `Event -> Status / Toast / Card / Modal` 的判断集中起来
   - 避免在多个 action 里重复写字符串拼接

#### 受影响文件

- [cli/src/state/reducer.rs](D:/Software/Projects/CloudAgent/cli/src/state/reducer.rs)
- [cli/src/state/bottom_pane_controller.rs](D:/Software/Projects/CloudAgent/cli/src/state/bottom_pane_controller.rs)
- [cli/src/state/bottom_pane_runtime.rs](D:/Software/Projects/CloudAgent/cli/src/state/bottom_pane_runtime.rs)
- [cli/src/app/conversation/actions/local_basic_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/local_basic_actions.rs)
- [cli/src/app/conversation/actions/local_command_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/local_command_actions.rs)
- [cli/src/app/conversation/actions/local_gateway_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/local_gateway_actions.rs)

### Phase 2: 状态栏只留持续状态

#### 目标

状态栏只负责持续运行态，不再承载 transient notice。

#### 需要做的事

1. `BottomPaneRuntimeState` 只保留：
   - `active_tool`
   - `live_label`
   - `turn_active`
   - `turn_started_at`

2. 去掉 `live_banner` 作为“短暂提示出口”的角色

3. `BottomPaneController::build_status_view_model()` 只投影持续态

4. `ChatSurface::render()` 只把 status area 和 toast area 分开渲染

#### 受影响文件

- [cli/src/state/bottom_pane_runtime.rs](D:/Software/Projects/CloudAgent/cli/src/state/bottom_pane_runtime.rs)
- [cli/src/state/bottom_pane_controller.rs](D:/Software/Projects/CloudAgent/cli/src/state/bottom_pane_controller.rs)
- [cli/src/ui/chat_surface.rs](D:/Software/Projects/CloudAgent/cli/src/ui/chat_surface.rs)
- [cli/src/ui/chat_surface_model.rs](D:/Software/Projects/CloudAgent/cli/src/ui/chat_surface_model.rs)

### Phase 3: 短暂通知独立成通道

#### 目标

让短暂通知有统一去重、TTL 和优先级，而不是散落在各个 action 里。

#### 需要做的事

1. 保留独立通知存储
   - `NotificationStore` 负责 active toast
   - TTL 默认 4 秒
   - 同类通知覆盖旧值

2. 把所有一次性 notice 统一改为 `push_toast`
   - `interrupt requested`
   - `no active turn`
   - 历史页加载失败
   - 连接失败
   - 模型列表加载失败

3. 后端 reducer 不直接决定“怎么画”
   - reducer 只发结构化 action
   - action 执行层决定是否进 toast

#### 受影响文件

- [cli/src/state/notification.rs](D:/Software/Projects/CloudAgent/cli/src/state/notification.rs)
- [cli/src/state/notification_store.rs](D:/Software/Projects/CloudAgent/cli/src/state/notification_store.rs)
- [cli/src/app/conversation/actions/server_actions.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/actions/server_actions.rs)
- [cli/src/app/conversation/event_router.rs](D:/Software/Projects/CloudAgent/cli/src/app/conversation/event_router.rs)
- [cli/src/app/runtime/lifecycle.rs](D:/Software/Projects/CloudAgent/cli/src/app/runtime/lifecycle.rs)

### Phase 4: 长期提示进入历史区

#### 目标

把“需要看见，但不该占状态栏”的内容变成历史卡片。

#### 适合内容

- setup follow-up
- import follow-up
- 长期 warning
- 非阻塞 error recap
- 完成后的 item summary

#### 需要做的事

1. 给 `HistoryCell` 新增更明确的 notice/card 构造入口
2. `notice_cards.rs` 只负责 notice 专用渲染
3. `display.rs` 根据 `HistoryKind::Notice` 走专用分支
4. 不再把历史卡写成“工具卡兜底”

#### 受影响文件

- [cli/src/ui/history_cell/mod.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/mod.rs)
- [cli/src/ui/history_cell/display.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/display.rs)
- [cli/src/ui/history_cell/notice_cards.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/notice_cards.rs)
- [cli/src/ui/history_cell/transcript_cards.rs](D:/Software/Projects/CloudAgent/cli/src/ui/history_cell/transcript_cards.rs)
- [cli/src/app/tests.rs](D:/Software/Projects/CloudAgent/cli/src/app/tests.rs)

### Phase 5: 阻塞式操作框保持纯粹

#### 目标

权限确认、server request、picker、gateway 这类交互只做阻塞式决策，不和通知混用。

#### 需要做的事

1. `InputPane` 只管理 modal / popup / picker 的栈
2. `ServerRequestOverlay` 只负责阻塞式展示
3. `ServerRequestPresentation` 只负责标题、原因、预览
4. 不让 notice 文案回流到 modal 的主语义里

#### 受影响文件

- [cli/src/ui/bottom_pane/input_pane/mod.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/input_pane/mod.rs)
- [cli/src/ui/bottom_pane/dialogs/server_request/server_request_overlay.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/server_request/server_request_overlay.rs)
- [cli/src/ui/bottom_pane/dialogs/server_request/server_request_model.rs](D:/Software/Projects/CloudAgent/cli/src/ui/bottom_pane/dialogs/server_request/server_request_model.rs)
- [cli/src/state/reducer.rs](D:/Software/Projects/CloudAgent/cli/src/state/reducer.rs)

## 8. 推荐落地顺序

1. 先把 `show_transient_notice` 这类入口全部收敛到 `push_toast`
2. 再让 `build_status_view_model` 只保留持续状态
3. 再把 `PushNoticeCell` / `PushErrorCell` 改成结构化通知事件
4. 再整理 `notice_cards`，让历史卡片真正拥有 notice 语义
5. 最后清理掉旧的混合入口

## 9. 验收标准

### 9.1 架构验收

- 状态栏不再承载短暂通知
- toast 不再跟 runtime 混在同一字段里
- 历史卡片有独立 notice 语义
- modal 仍然只做阻塞式交互

### 9.2 行为验收

- 一次性错误只显示短暂通知
- 持续运行态稳定显示
- 需要留痕的提示进入历史区
- 权限确认不会被历史卡片替代

### 9.3 代码验收

- `state` 只保存状态
- `ui` 只做渲染
- `reducer` 只做消息翻译
- 测试文件与实现文件分离
