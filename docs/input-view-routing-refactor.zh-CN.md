# 输入视图路由重构实施方案

## 文档定位

这是一份可直接实施的重构方案，不保留长期兼容层，不依赖补丁式兜底，也不允许新旧规则长期并存。

目标不是“先修 `/session` 的 `Esc`”，而是一次性把输入区、展开视图、返回、对话中断这四类行为纳入统一架构，让后续新增 `/config`、`/model`、`/permissions`、`/gateway`、server request 等视图时，不需要再改全局 `Esc` 优先级逻辑。

## 一、要解决的核心问题

当前问题不是某个视图自己没处理好 `Esc`，而是输入系统存在两个并行出口：

1. 活跃视图路径：
   - `BottomPaneNavigator::handle_key()` 处理当前 view stack。

2. 中断路径：
   - `InputPane::handle_escape_key()` 在 composer 不消费 `Esc` 时返回 `ComposerIntent::Interrupt`。
   - `TuiApp::handle_key()` 再把它映射成 `AppClientCommand::InterruptTurn`。

真正导致不确定性的点，是 navigator 允许某些活跃视图在 `Esc` 时不消费，而是继续冒泡到 composer 中断路径。

当前最关键的危险设计是：

```rust
NavigationKeyResult::FallthroughEscFromActionRequiredView
```

它让系统出现以下违背直觉的状态：

1. view 仍然活跃。
2. 用户按了一次 `Esc`。
3. 这个 `Esc` 没有先完成“关闭当前 view / 返回上一级”的语义。
4. 反而继续外溢，触发 running turn interrupt。

这就是为什么在发送消息进入 `working` 后，打开 `/session` 列表，再按一次 `Esc`，会稳定中断对话并关闭列表。

## 二、当前代码中的真实职责边界

重构必须尊重当前已有的业务边界，不能为了“拆文件”把职责反向塞回错误层。

### 1. `TuiApp`

文件：

- `cli/src/app/core/input_mapping.rs`

职责：

1. 顶层快捷键路由。
2. `InputPaneAction -> ParsedInput` 映射。
3. 仅在这里把 `ComposerIntent::Interrupt` 变成 `AppClientCommand::InterruptTurn`。

结论：

- interrupt 的业务决策最终 owner 仍然是 app input layer。
- `InputPane` 不能直接发出业务命令，只能发“请求中断”的输入意图。

### 2. `BottomPaneController`

文件：

- `cli/src/state/bottom_pane_controller.rs`

职责：

1. 持有 `InputPane`。
2. 持有 runtime banner / turn status 等运行时状态。
3. 管理 `/session` 这类异步视图的外部状态，例如 loading generation、防止过期响应回填。
4. 提供面向 `TuiApp` 的 bottom pane 编排接口。

结论：

- 这是输入区的业务编排层。
- `/session` 的“请求、等待、结果回填、过期响应丢弃”必须留在这里，不能塞回 `InputPane`。

### 3. `InputPane`

文件：

- `cli/src/ui/widgets/input_pane.rs`

职责：

1. 持有 `ChatComposer`。
2. 持有 `BottomPaneNavigator`。
3. 负责输入区渲染、视图显示、局部输入路由。

当前问题：

1. 既管 render，又管 layout，又管 view factory，又管 key routing。
2. `Esc` 的最终 fallback 在这里。
3. `/session`、`/gateway`、`/config` 等具体视图创建代码也堆在这里。

结论：

- 它应该是 UI facade，不该继续承担编排层职责。

### 4. `BottomPaneNavigator`

文件：

- `cli/src/ui/bottom_pane_navigation/mod.rs`
- `cli/src/ui/bottom_pane_navigation/result.rs`

职责：

1. 唯一持有 view stack。
2. 决定当前活跃 view。
3. 负责 view 层 key routing 和 pop / replace / child-parent dismiss 规则。

当前问题：

1. 引入 `FallthroughEscFromActionRequiredView`，破坏“活跃 view 优先消费”的基本原则。
2. `Esc` 规则和 view 状态耦合方式不纯。

### 5. `BottomPaneView`

文件：

- `cli/src/ui/widgets/bottom_pane_view.rs`

职责：

1. 局部视图交互。
2. 局部视图渲染。

当前问题：

1. 混入多种业务识别接口。
2. `prefer_esc_to_handle_key_event()` 是一个语义不稳定的权宜接口。
3. `/session` loading generation 这种外部编排信息，不该继续通过 trait 下沉。

## 三、目标架构

重构后的输入层明确分成五层：

1. `TuiApp`
   - 顶层输入映射层。
   - 唯一将“请求中断”转成业务 interrupt command 的地方。

2. `BottomPaneController`
   - 输入区业务编排层。
   - 管理异步视图请求、过期响应丢弃、runtime 状态与 UI 之间的协调。

3. `InputPane`
   - 输入区 UI facade。
   - 只管理 composer、navigator、render coordinator。

4. `BottomPaneNavigator`
   - 唯一 view stack owner。
   - 唯一负责活跃 view 的 key routing。

5. `BottomPaneView`
   - 局部视图实现层。
   - 每个 view 只处理自己的展示和交互。

### 架构硬规则

1. 只要 view stack 非空，`Esc` 就绝不允许进入 interrupt 路径。
2. interrupt 只能在“没有活跃 view、没有 composer popup、composer 未消费 `Esc`”时产生。
3. `/session`、`/gateway`、`server request` 等所有展开视图都必须走统一 navigator 规则，不准写单命令特判。
4. 异步视图编排逻辑只允许放在 `BottomPaneController`，不下沉到 `InputPane` 或单个 view。
5. 不保留旧路由和新路由双轨并存，不引入 migration-only 接口长期挂着不用删。

## 四、重构后的 `Esc` 规则

这是最终行为规范，代码和测试都必须围绕它实现。

### 1. 活跃 view 优先

如果 `BottomPaneNavigator` 有 active view：

1. `Esc` 只发给 active view / navigator。
2. 不允许继续冒泡给 composer。
3. 不允许继续演化为 `InterruptRequested`。

### 2. composer popup 次优先

如果没有 active view，但 composer 有 completion popup：

1. `Esc` 关闭 popup。
2. 不进入 interrupt。

### 3. composer 最后处理

如果没有 active view，也没有 popup：

1. composer 可以处理 `Esc`。
2. 若 composer 返回局部动作，则执行局部动作。
3. 若 composer 不消费 `Esc`，才产生“请求中断”。

### 4. interrupt 业务映射

“请求中断”不是业务命令。

1. `InputPane` 只产生局部输入结果。
2. `TuiApp` 根据当前 `FrontendMode` 决定：
   - idle 时忽略。
   - non-idle 时映射成 `InterruptTurn`。

### 5. `requires_action()` 的新边界

`requires_action()` 只能影响：

1. 标题文案。
2. 状态展示。
3. 渲染风格。

它不再影响：

1. `Esc` 是否穿透。
2. key routing 是否冒泡。
3. interrupt 触发优先级。

## 五、`ServerRequestOverlay` 的明确产品规则

这是本次方案里必须先定死的点，不能模糊。

当前 `ServerRequestOverlay` 是 action-required 视图，不能允许“按一下 `Esc` 什么都没决策就把审批层关掉”。

因此重构后规则定为：

1. `ServerRequestOverlay` 活跃时，`Esc` 不关闭 overlay。
2. 如果 overlay 的 note 输入框有内容且存在局部编辑态，`Esc` 只允许退出局部编辑态或清理局部选择，不允许 dismiss 整个审批层。
3. 如果 overlay 当前没有可退出的局部编辑态，`Esc` 被 consume，但不触发任何业务动作。
4. 用户必须通过显式 approve / approve-for-session / deny / slash command 提交，或由外部事件显式 dismiss 对应 request。

结论：

- `ServerRequestOverlay` 不是普通 picker，不适用“默认 `Esc` 关闭”。
- 它必须显式 override `Esc` 策略。

## 六、按键路由结果模型

不再额外引入第三套长期并存的大枚举，避免 `NavigationKeyResult`、`KeyRouteResult`、`InputPaneAction` 三层并行。

最终只保留两层结果：

1. `NavigationKeyResult`
   - navigator 内部与 `InputPane` 的边界结果。

2. `InputPaneAction`
   - `InputPane` 对 `BottomPaneController` / `TuiApp` 暴露的输入动作。

### 新的 `NavigationKeyResult`

```rust
pub(crate) enum NavigationKeyResult {
    NoActiveView,
    Handled,
    Composer(ComposerIntent),
    LoadMoreSessions { cursor: String },
    ServerRequestSubmit {
        request_id: RequestId,
        decision: ServerRequestDecisionKind,
        reason: String,
    },
}
```

说明：

1. 删除 `FallthroughEscFromActionRequiredView`。
2. 删除 `Consumed`，统一命名为 `Handled`。
3. navigator 只表达“view 层是否已经处理完按键”，不表达“是否应该继续尝试 interrupt fallback”。

### `InputPaneAction` 保持对外边界

`InputPaneAction` 仍然是 `InputPane` 的唯一对外动作结果：

1. `Composer(ComposerIntent)`
2. `LoadMoreSessions`
3. `ServerRequestSubmit`

不新增新的“半业务半路由”中间类型。

## 七、`BottomPaneView` 契约的最终形态

本方案不接受“先保留旧接口，以后慢慢删”的长期兼容思路。

`BottomPaneView` 在本轮重构中一次性切换到更干净的契约。

### 保留

1. `handle_key_event`
2. `handle_paste`
3. `render_lines`
4. `desired_height`
5. `cursor_position`
6. `is_complete`
7. `completion`
8. `dismiss_after_child_accept`
9. `clear_dismiss_after_child_accept`
10. `try_consume_server_request`
11. `dismiss_server_request`
12. `active_server_request_id`
13. `requires_action`
14. `append_session_page`

### 删除

1. `prefer_esc_to_handle_key_event()`
2. `is_model_picker_loading()`
3. `is_session_picker()`
4. `is_session_picker_loading(generation)`

### 新增

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViewKind {
    Help,
    Filter,
    Config,
    Permissions,
    Reasoning,
    ModelPicker,
    SessionPicker,
    GatewayList,
    GatewayEdit,
    WeixinBinding,
    ServerRequest,
}

fn kind(&self) -> ViewKind;
```

说明：

1. `ViewKind` 只表达视图类型，不承担业务状态查询。
2. loading 不单独作为一种 kind，而是作为具体 view 内部状态或具体 view 类型数据。
3. 像 session loading generation 这种外部编排信息，改由 `BottomPaneController` 自己持有并校验，不再通过 trait 反查。

## 八、view stack 与视图状态模型

### 1. view stack 的唯一 owner

仍然是 `BottomPaneNavigator`。

只允许它做：

1. `push`
2. `replace`
3. `replace_active`
4. `replace_parent_after_child`
5. `pop_with_completion`
6. `clear`

### 2. 不新增全局“视图状态中心”

这里不引入一个新的大而全 `ViewStateMachine` 单体，也不把所有 view 状态塞进一个超大 enum。

原因：

1. 当前 view 本质上已经是多态组件。
2. 问题不在“缺一个超级状态机”，而在“路由和编排职责分散”。
3. 强行总线化只会把逻辑从一个大文件迁到另一个大文件。

### 3. 活跃 surface 的判断

`InputPane` 内部保留一个很轻量的 surface 判断方法：

```rust
enum ActiveSurface {
    View,
    ComposerPopup,
    Composer,
}
```

它只用于本地路由，不上升为跨模块共享状态模型。

## 九、`/session`、`/gateway`、`server request` 的统一编排方式

### `/session`

这是“异步请求型视图”，分两层：

1. `BottomPaneController`
   - 发起 request。
   - 记录 pending generation。
   - 丢弃过期响应。
   - 决定何时将 loading view 替换为 loaded view。

2. `InputPane`
   - 只提供：
     - `show_session_picker_loading(...)`
     - `show_session_picker_page(...)`
     - `append_session_picker_page(...)`
     - `close_session_picker()`

重构后不再允许：

1. 通过 `BottomPaneView` trait 反查 loading generation。
2. 让 `InputPane` 自己承担异步 generation 判定。

### `/gateway`

这是“多层导航型视图”：

1. list -> edit -> binding 统一走 stack。
2. 返回规则只由 navigator 的 push / pop / replace 系列 API 决定。
3. 不为某个 gateway 子流程写特殊 `Esc` 分支。

### `server request`

这是“强约束 action-required 视图”：

1. 不允许 `Esc` dismiss。
2. 支持 overlay 自身消化后续 queued request。
3. dismiss 只能来自：
   - 显式提交。
   - 外部 request 被撤销或完成。

## 十、模块与文件规划

本方案按业务职责拆，不机械按函数数量拆。

### A. `cli/src/ui/widgets/input_pane/`

重构后目录：

```text
cli/src/ui/widgets/input_pane/
  mod.rs
  key_routing.rs
  render.rs
  layout.rs
  view_factory.rs
  tests.rs
  key_routing_tests.rs
  render_tests.rs
```

职责：

1. `mod.rs`
   - `InputPane` struct
   - 对外 facade
   - 少量简单委托

2. `key_routing.rs`
   - `handle_key`
   - `route_escape`
   - `active_surface`
   - navigator -> composer 的路由决策

3. `render.rs`
   - `InputPaneSnapshot`
   - `build_snapshot`
   - `render_request_view`
   - `cursor_position`

4. `layout.rs`
   - `InputPaneLayout`
   - `compute_input_layout`
   - 高度计算

5. `view_factory.rs`
   - 所有具体 view 的构造与显示入口
   - 只负责创建和切换 view
   - 不承担异步业务编排

说明：

- 原来的 `input_pane.rs` 被改成目录模块，不再继续维持 1000 行级单文件增长趋势。

### B. `cli/src/ui/bottom_pane_navigation/`

重构后目录：

```text
cli/src/ui/bottom_pane_navigation/
  mod.rs
  result.rs
  route.rs
  stack.rs
  tests.rs
  route_tests.rs
```

职责：

1. `mod.rs`
   - 对外导出 `BottomPaneNavigator`

2. `stack.rs`
   - stack 持有与 push / replace / pop 规则

3. `route.rs`
   - key routing
   - `Esc` 规则
   - `BottomPaneViewAction -> NavigationKeyResult`

4. `result.rs`
   - `NavigationKeyResult`

说明：

- 这里拆分是必要的，因为 navigator 同时承担“stack 管理”和“key route”，已经是两个稳定职责。

### C. `cli/src/ui/widgets/chat_composer/`

重构后目录：

```text
cli/src/ui/widgets/chat_composer/
  mod.rs
  attachments.rs
  history.rs
  render.rs
  key_handling.rs
  completion.rs
  tests.rs
  key_handling_tests.rs
  completion_tests.rs
```

说明：

1. 这轮不强行拆 `slash.rs`。
2. slash command 解析逻辑如果当前已被 `input/` 层清晰承接，就不重复抽象。
3. 优先拆最影响输入路由判断的 `key_handling` 与 `completion`。

### D. `cli/src/ui/widgets/gateway_panel/`

重构后目录：

```text
cli/src/ui/widgets/gateway_panel/
  mod.rs
  model.rs
  key_handling.rs
  render.rs
  tests.rs
```

### E. `cli/src/ui/widgets/history_cell/`

重构后目录：

```text
cli/src/ui/widgets/history_cell/
  mod.rs
  model.rs
  render.rs
  user.rs
  assistant.rs
  system.rs
  markdown.rs
  tool_ui.rs
  tool_aggregation.rs
  tests.rs
```

### F. `cli/src/ui/theme/`

重构后目录：

```text
cli/src/ui/theme/
  mod.rs
  palette.rs
  surface.rs
  input.rs
  picker.rs
  history.rs
  request.rs
  status.rs
  tests.rs
```

职责：

1. `mod.rs`
   - 统一导出样式入口。
   - 对外只暴露语义化 style API，不暴露零散 RGB 常量。

2. `palette.rs`
   - 只放颜色 token 和少量基础常量。
   - 不写业务判断，不读取环境变量，不关心具体 widget。

3. `surface.rs`
   - 通用面板、边框、标题、次级文本、提示文本样式。
   - 作为其它语义样式的基础层。

4. `input.rs`
   - 输入框、completion popup、hint、composer chrome 的样式。

5. `picker.rs`
   - session / model / config / permissions / reasoning / filter / gateway 等列表类视图的样式。
   - 统一选中态、普通态、loading 态、空态、辅助说明态。

6. `history.rs`
   - history 相关的可复用语义样式工厂。
   - 只提供历史消息背景、强调色、辅助 rail、引用色等通用样式，不负责行结构和内容拼装。

7. `request.rs`
   - server request overlay、审批态、note 输入态、action-required 态样式。

8. `status.rs`
   - footer、status line、运行态信息、状态指标样式。

说明：

1. 颜色语义必须统一收口到 `ui/theme`，widgets 只消费，不再重复定义业务颜色规则。
2. `terminal/color_compat.rs` 只保留终端能力检测和 ANSI 适配，不承载 UI 语义。
3. `custom_terminal.rs` 只负责把 `Style` 写成终端输出，不负责决定 widget 该用什么颜色。
4. 这条线和 `Esc` 路由重构并行推进，但不互相依赖，也不要求先完成某一边再开始另一边。
5. 原本散落在 `history_cell/display.rs`、`input_pane/render.rs`、`session_picker.rs`、`server_request_overlay.rs` 等文件里的 RGB 常量，应逐步迁移到这里。

迁移原则：

1. 先抽“可复用语义样式”，再删掉各 widget 里的本地常量。
2. 先迁移高复用、低风险的样式，再迁移带有复杂状态分支的样式。
3. 每次迁移后必须补对应样式测试，避免只把颜色从一个文件挪到另一个文件。

## 十一、测试组织规范

测试从业务实现文件剥离，不在实现文件底部继续塞大段 `#[cfg(test)] mod tests`。

### 组织原则

1. 每个业务模块使用 sibling test 文件。
2. 测试按行为域命名，不搞一个巨大总测试文件。
3. 共享 helper 只有在三个以上测试文件复用时才抽到 `test_support`。

### 必测场景

1. running mode + session picker active + `Esc` 关闭 view，不 interrupt。
2. running mode + config panel active + `Esc` 关闭 view，不 interrupt。
3. running mode + gateway child view active + `Esc` 只 pop 顶层，不 interrupt。
4. running mode + server request overlay active + `Esc` 被 consume，不 dismiss，不 interrupt。
5. running mode + no view + no popup + `Esc` -> interrupt request。
6. idle mode + no view + no popup + `Esc` 不触发业务 interrupt。
7. composer completion popup active + `Esc` 仅关闭 popup。
8. `/session` loading view 被 `Esc` 关闭后，晚到响应被丢弃。
9. child accepted 后，`dismiss_after_child_accept()` 正常清理 parent。
10. non-press key event 不触发关闭、不触发 interrupt。

### 测试层次

1. `bottom_pane_navigation`：
   - 只测 view stack 路由和 pop 规则。

2. `input_pane`：
   - 只测 surface 路由和 composer / navigator 优先级。

3. `bottom_pane_controller`：
   - 只测异步 view 编排，例如 session generation、late response drop。

4. `app`：
   - 只测 input intent 到业务 command 的映射，例如 interrupt。

## 十二、分阶段实施

每个阶段完成后，代码库必须处于“新规则已独立成立”的状态，不接受“先引入一半、再靠旧逻辑兜底”的过渡。

### Phase 1：砍掉 `Esc` 穿透

目标：

1. 删除 `FallthroughEscFromActionRequiredView`。
2. 建立“活跃 view 存在时，`Esc` 绝不进入 composer interrupt 路径”的硬规则。
3. 明确 `ServerRequestOverlay` 的 `Esc` consume 行为。

改动：

1. `cli/src/ui/bottom_pane_navigation/result.rs`
2. `cli/src/ui/bottom_pane_navigation/mod.rs`
3. `cli/src/ui/widgets/server_request_overlay.rs`
4. `cli/src/ui/widgets/input_pane.rs`
5. `cli/src/ui/bottom_pane_navigation/tests.rs`
6. `cli/src/ui/widgets/input_pane_esc_tests.rs`
7. `cli/src/app/tests.rs`

完成标准：

1. active view 下 `Esc` 全部在 navigator 终结。
2. running mode + active view 时，单次 `Esc` 不会产出 interrupt。
3. `ServerRequestOverlay` 不再依赖 `requires_action()` 特判实现防穿透。

### Phase 2：输入区模块化

目标：

1. 将 `InputPane` 从大单体改为目录模块。
2. 把 key routing、layout、render、view factory 职责拆开。
3. 明确 `input_pane.rs` 到 `input_pane/` 的切换点，避免新旧路径长期混用。

改动：

1. 新建 `cli/src/ui/widgets/input_pane/`
2. 将 `input_pane.rs` 改为 `input_pane/mod.rs`
3. 新增：
   - `key_routing.rs`
   - `layout.rs`
   - `render.rs`
   - `view_factory.rs`

完成标准：

1. `InputPane` 对外 public surface 保持稳定。
2. `handle_key` 只保留 facade 语义。
3. render 与 routing 不再和 view factory 混写在一个文件里。
4. Phase 2 完成后，后续所有文档、测试和代码引用统一使用 `cli/src/ui/widgets/input_pane/` 目录路径，不再保留旧文件路径作为默认入口。

### Phase 3：清理 `BottomPaneView` 契约

目标：

1. 一次性删除旧的业务识别洞口。
2. 改成 `ViewKind + controller typed orchestration` 的清晰边界。

改动：

1. `cli/src/ui/widgets/bottom_pane_view.rs`
2. 所有 view implementation
3. `cli/src/state/bottom_pane_controller.rs`
4. `cli/src/ui/widgets/input_pane/view_factory.rs`

完成标准：

1. trait 中不再保留 `is_session_picker_loading(generation)` 这类外部编排查询。
2. `prefer_esc_to_handle_key_event()` 被彻底删除。
3. `/session` generation 逻辑完全回收至 controller。

### Phase 4：拆 navigator

目标：

1. 将 stack 管理和 route 逻辑拆开。
2. 让 view stack 规则更容易单测和推导。

改动：

1. `cli/src/ui/bottom_pane_navigation/mod.rs`
2. 新增：
   - `route.rs`
   - `stack.rs`
   - `route_tests.rs`

完成标准：

1. route 文件只负责按键语义。
2. stack 文件只负责 push / replace / pop / completion。

### Phase 5：大文件治理

优先顺序：

1. `chat_composer.rs`
2. `gateway_panel.rs`
3. `history_cell/mod.rs`
4. `textarea.rs`

完成标准：

1. 每次只拆一个业务域。
2. 拆分后模块职责可以一句话说清。
3. 测试与实现文件分离。

### Phase 6：颜色与样式统一收口

目标：

1. 将散落在各 widget 文件中的颜色语义收口到 `cli/src/ui/theme/`。
2. 让 `widgets` 只消费样式，不再自行定义同一语义的颜色常量。
3. 保留 `terminal/color_compat.rs` 的终端适配职责，不把 UI 语义倒灌回终端层。

改动：

1. 新建 `cli/src/ui/theme/`
2. 迁移优先级：
   - `cli/src/ui/widgets/history_cell/display.rs`
   - `cli/src/ui/widgets/footer.rs`
   - `cli/src/ui/widgets/input_pane/render.rs`
   - `cli/src/ui/widgets/session_picker.rs`
   - `cli/src/ui/widgets/model_picker.rs`
   - `cli/src/ui/widgets/config_panel.rs`
   - `cli/src/ui/widgets/filter_picker.rs`
   - `cli/src/ui/widgets/permissions_picker.rs`
   - `cli/src/ui/widgets/reasoning_picker.rs`
   - `cli/src/ui/widgets/server_request_overlay.rs`
   - `cli/src/ui/widgets/gateway_panel/render.rs`
   - `cli/src/ui/widgets/help_view.rs`
   - `cli/src/ui/widgets/welcome.rs`
   - `cli/src/ui/widgets/weixin_binding_view.rs`
3. 迁移完成后，删除各 widget 内重复出现的本地 RGB 常量和只服务单个 widget 的小型样式工厂。

完成标准：

1. `ui/theme` 成为业务语义样式的唯一默认入口。
2. `history_cell` 只负责历史内容结构、行拼装、缩进与渲染组合，不再承载可复用颜色语义。
3. `custom_terminal` 和 `color_compat` 不再混入 widget 级样式判断。
4. 颜色相关测试按样式语义落到 `ui/theme/tests.rs`，而不是继续散落在各个 widget 的实现文件里。
5. 这一步完成后，颜色和输入路由两条重构线都具备独立完成态。

## 十三、每阶段验收命令

### Phase 1

```powershell
cargo test -p cli esc -- --nocapture
cargo test -p cli bottom_pane_navigation -- --nocapture
cargo test -p cli input_pane -- --nocapture
```

### Phase 2

```powershell
cargo test -p cli input_pane -- --nocapture
cargo test -p cli bottom_pane_controller -- --nocapture
```

### Phase 3

```powershell
cargo test -p cli session_picker -- --nocapture
cargo test -p cli model_picker -- --nocapture
cargo test -p cli bottom_pane_controller -- --nocapture
```

### Phase 4

```powershell
cargo test -p cli bottom_pane_navigation -- --nocapture
```

### Phase 5

```powershell
cargo test -p cli chat_composer -- --nocapture
cargo test -p cli gateway_panel -- --nocapture
cargo test -p cli history_cell -- --nocapture
```

## 十四、最终验收标准

达到以下条件，才算这轮重构完成：

1. `Esc` 行为可以用一张固定规则表解释，没有任何隐藏 fallthrough。
2. 发送消息进入 running 后，打开任何命令展开 view，单次 `Esc` 只会关闭 / 返回该 view，不会中断对话。
3. `ServerRequestOverlay` 的 `Esc` 行为稳定、确定、不可穿透。
4. interrupt 只会在“无 active view、无 popup、composer 未消费 `Esc`”时出现。
5. `/session` 的 loading generation 与 late response drop 逻辑明确留在 `BottomPaneController`。
6. `InputPane` 不再是 view factory、render、layout、routing 混合的大单体。
7. `BottomPaneView` trait 不再承担外部编排查询职责。
8. 测试文件与业务实现文件分离，且覆盖关键输入路由场景。
