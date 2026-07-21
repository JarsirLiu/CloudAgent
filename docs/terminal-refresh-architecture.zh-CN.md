# CloudAgent CLI 终端刷新与滚动架构诊断

## 1. 文档目的

本文记录 CloudAgent CLI 当前终端刷新、流式消息展示和历史滚动问题，并给出参考 `D:\codespace\codex` 的修复方向。

本文关注三个用户可见问题：

1. 流式回复期间终端容易闪烁。
2. 输入 `/` 打开命令补全时，历史消息较多，画面会重新滚动或位置跳动。
3. 历史消息、活动消息、输入面板高度变化互相影响，导致问题反复修补但无法稳定收口。

本文是架构诊断和实施依据，不等同于具体代码补丁。实现时应保持每一步可单独测试和回滚。

## 2. 结论摘要

当前最主要的根因不是 frame 请求太频繁，而是把两种完全不同的变化混成了同一个动作：

```text
活动 viewport 高度变化
    被判断为
历史 scrollback 必须 FullReplay
```

具体路径是：

```text
流式消息变化 / 打开 /
    -> 活动区或输入区高度变化
    -> desired_viewport_height() 变化
    -> TerminalProjectionController 判定 FullReplay
    -> clear_for_history_replay()
    -> Clear(All) + Clear(Purge)
    -> 重放全部历史
    -> 重新绘制当前 viewport
```

因此，历史越多，重放成本越高，闪烁和重新滚动越明显。

必须建立的核心规则是：

> `viewport height change` 不得自动触发 `history replay`。

只有历史内容本身发生不可追加的变化，或者历史渲染宽度改变导致换行结构失效时，才允许进入全量历史重放路径。

## 3. 当前刷新链路

### 3.1 事件到 frame

当前主循环位于：

- `cli/src/app/runtime/loop.rs`
- `cli/src/app/runtime/controller.rs`
- `cli/src/terminal/events/frame_requester.rs`

主要流程如下：

```text
AppServerEvent / Key / Paste / Resize / Tick
    -> RuntimeController 处理事件
    -> FrameRequester::schedule_frame()
    -> UiEvent::Draw
    -> draw_with_terminal_projection()
    -> TerminalProjectionController::draw_frame()
    -> TerminalGuard::draw_projection()
    -> Terminal::draw()
```

当前 `FrameRequester` 使用 `draw_pending` 合并重复的即时 frame 请求。这部分不是主要闪烁根因。即使请求被正确合并，只要一次 frame 内执行了清屏和历史重放，用户仍然会看到跳动。

### 3.2 Projection 到终端

当前代码位于：

- `cli/src/app/runtime/terminal_projection.rs`
- `cli/src/terminal/draw_coordinator.rs`
- `cli/src/terminal/custom_terminal.rs`

每帧大致执行：

```text
计算 viewport_height
计算历史 render metrics
计算 scrollback diff
准备 history update
调整 terminal viewport
必要时插入历史行
使用双 buffer diff 绘制当前 viewport
```

问题出在 `prepare_history_update()` 的 replay 判定：

```rust
let full_replay = self.last_scrollback_metrics != Some(render_metrics)
    || self.last_viewport_height != Some(viewport_height)
    || matches!(update, ScrollbackDiff::Replay);
```

其中第二个条件是错误的：viewport 高度是终端布局状态，不是历史内容状态。

### 3.3 FullReplay 的实际副作用

`DrawCoordinator` 在 `FullReplay` 时调用：

```rust
self.terminal.clear_for_history_replay()?;
```

`clear_for_history_replay()` 会执行：

```text
Clear(All)
Clear(Purge)
MoveTo(0, 0)
清空两个 terminal buffer
flush
```

这不是普通 repaint，而是破坏并重新建立终端可见状态和 scrollback 的操作。它只能用于真正的历史重建，不能用于活动消息每增加一行、输入 popup 出现或输入框换行。

## 4. 问题一：流式消息闪烁

### 4.1 触发条件

流式输出会不断改变活动消息的渲染行数：

- Markdown 换行数量增加；
- 当前段落从一行变成多行；
- 流式稳定行被提交到 scrollback，活动 tail 变短；
- 工具状态、reasoning、进度文本改变活动区高度。

这些变化会影响 `ChatSurface::desired_viewport_height()`。

### 4.2 错误路径

```text
活动消息高度变化
    -> viewport_height 变化
    -> full_replay = true
    -> clear_for_history_replay()
    -> 清空终端
    -> 历史重新写入
    -> 当前活动消息重新绘制
```

这解释了为什么“只改刷新频率”效果不稳定：刷新频率只决定清屏发生得快还是慢，没有消除清屏动作本身。

### 4.3 额外放大因素

当前系统同时使用两种输出区域：

```text
已提交历史      -> 直接写入终端 scrollback
活动消息和输入区 -> ratatui-like viewport 双 buffer
```

两者之间没有一个统一的、可验证的屏幕状态模型。历史直接通过 ANSI 写入终端，活动区再通过 buffer diff 写入终端；只要 viewport 的 y/height、光标位置、滚动区或 buffer area 有一帧不一致，就会产生跳动。

这套设计并非不能使用，但必须严格区分：

- 历史追加；
- 历史重放；
- viewport 移动；
- 当前 frame diff。

当前实现的 replay 判定破坏了这个边界。

## 5. 问题二：输入 `/` 时历史重新滚动

这里的“执行 `/`”通常首先发生的是输入 `/` 并打开命令补全 popup，而不是已经执行某个 slash command。

### 5.1 输入面板高度变化

输入区布局位于：

- `cli/src/ui/bottom_pane/input_pane/layout.rs`
- `cli/src/ui/bottom_pane/input_pane/render.rs`

输入区本体包含状态行、composer、提示行和边框。completion、配置、会话、模型等非全屏 view 的 popup 不属于输入区本体，而是锚定在输入框上方的 overlay；它只改变绘制层，不改变 bottom pane 的 desired height。

因此：

```text
输入 /
    -> completion_lines 非空
    -> 计算输入框上方 popup_area
    -> 清理并绘制 overlay
    -> bottom pane desired_height 不变
    -> viewport_height 不变
    -> 历史 scrollback 不参与这次 UI 变化
```

### 5.2 为什么历史越多越明显

历史消息多不会直接改变 slash popup 的逻辑；在旧布局中，popup 高度变化会放大终端滚动副作用：

- 需要重新渲染和写入的历史行更多；
- scrollback 被清空、重建的范围更大；
- 终端滚动区域调整距离更大；
- 重新绘制期间，活动 viewport 的起始位置更容易发生可见跳动。

所以 `/` 问题和流式闪烁共享终端 projection 的边界问题。popup 作为 overlay 后，打开命令列表不再是 viewport transition，也不再需要用 history replay 兜底。

## 6. 问题三：两套滚动模型职责冲突

当前状态大致分为：

```text
CommittedTranscriptStore
    -> scrollback_snapshot()
    -> 终端历史区

ActiveCellController
    -> viewport_snapshot()
    -> 活动消息区

TranscriptScroll
    -> 活动 viewport 内的 top row

Terminal::viewport_area
    -> 终端物理 viewport 的 Rect
```

这几个对象解决的是不同问题，但当前它们之间的关系没有完全收口。

### 6.1 TranscriptScroll 不拥有完整历史滚动

`TranscriptScroll` 只根据活动 viewport 的内容行数计算 `top_row`。已经写入终端 scrollback 的历史并不在它的 `content_rows` 中。

这意味着：

- 活动消息的逻辑滚动由 `TranscriptScroll` 管理；
- 历史消息的物理滚动由终端 ANSI scroll region 管理；
- 底部输入区高度变化由 `Terminal::ensure_viewport_height()` 调整；
- 历史 replay 又会重置终端 scrollback。

如果任何一层把其他层的变化误判成自己的内容变化，就会出现位置跳动。

### 6.2 历史 snapshot 和 terminal screen state 没有单一提交点

`TranscriptOwner` 的历史状态和 `Terminal` 的实际 scrollback 状态分别更新。中间通过 `TerminalProjectionController` 推断 append 或 replay。

这种推断必须建立在强不变量上：

```text
历史 snapshot append-only
    -> 只能 append history lines

历史已有前缀变化
    -> 必须 full replay

只有 viewport 高度变化
    -> 不能改变 history update
```

目前第三条没有成立。

## 7. 与 Codex 架构的对应关系

参考实现目录：

```text
D:\codespace\codex\codex-rs\tui\src\tui.rs
D:\codespace\codex\codex-rs\tui\src\ui\frame_requester.rs
D:\codespace\codex\codex-rs\tui\src\insert_history.rs
D:\codespace\codex\codex-rs\tui\src\streaming\controller.rs
```

### 7.1 Codex 的 frame 调度

Codex 的 `FrameRequester` 把请求发送给独立 scheduler：

```text
多个 schedule_frame()
    -> scheduler 合并 deadline
    -> rate limiter 限制最大帧率
    -> 广播一个 Draw 通知
```

CloudAgent 当前的 `draw_pending` 合并逻辑已经具备类似目标，因此不需要先重写整个事件系统。

可以后续补充 rate limiter，但这不是解决闪烁的第一优先级。

### 7.2 Codex 把历史行先放入 pending queue

Codex 的 `insert_history_lines_with_wrap_policy()` 不直接在业务事件处理时写终端，而是先放入 `pending_history_lines`，然后请求下一帧。

绘制时统一执行：

```text
调整 viewport
    -> flush pending history lines
    -> 绘制当前 widget
```

这保证历史插入和当前 viewport 绘制处于同一个 draw transaction 中。

CloudAgent 当前虽然已经有 `HistoryReplayBatch` 和 `DrawCoordinator`，但 history projection 在进入 terminal 前就把 viewport height 混入了 history replay 判定，导致交易边界被破坏。

### 7.3 Codex 的 viewport 高度变化

Codex 在 `Tui::draw(height, ...)` 中：

1. 根据新的 height 计算 viewport area；
2. 如果活动区变高，使用 scroll region 把上方区域向上滚动；
3. 如果 viewport Rect 变化，清理必要的旧区域；
4. flush pending history lines；
5. 绘制当前 frame。

关键点是：

```text
viewport area 变化 != history source 变化
```

Codex 只有在 transcript 需要 resize reflow、历史前缀发生变化或明确需要重建时，才走完整 repaint/replay。

## 8. 目标架构

目标不是照搬 Codex 的所有模块，而是复制它的职责边界。

### 8.1 四种变化必须分开

| 变化类型 | 例子 | 应执行的动作 |
| --- | --- | --- |
| Frame 请求 | server delta、输入字符、状态变化 | 合并后绘制一帧 |
| Viewport 变化 | 活动消息变高、输入框换行 | 调整 viewport，局部清理和 diff |
| Overlay 变化 | popup、配置面板、会话/模型选择器出现 | 清理 overlay Rect 后在当前 frame 最后绘制，不改变 viewport |
| History append | 新稳定历史行提交 | append 到终端 scrollback |
| History replay | 宽度变化、历史前缀替换、恢复会话 | 清理并重放历史 |

禁止以下隐式关系：

```text
viewport height change -> history replay
frame request          -> clear terminal
popup open             -> render overlay only
active tail change     -> rebuild all history
```

### 8.2 Projection 状态建议

`TerminalProjectionController` 应只跟踪影响历史投影的状态：

```rust
struct TerminalProjectionController {
    last_scrollback_revision: Option<u64>,
    last_scrollback_metrics: Option<TranscriptRenderMetrics>,
    last_scrollback_cells: Vec<HistoryCell>,
}
```

`last_viewport_height` 不应参与 history diff 的 early return 或 `full_replay` 判定。viewport height 应由 `Terminal` 自己根据本帧 projection 处理。

建议逻辑：

```rust
let update = scrollback_diff(&previous_cells, &current_cells);
let metrics_changed = last_metrics != Some(render_metrics);
let full_replay = metrics_changed || matches!(update, ScrollbackDiff::Replay);
```

如果 revision、metrics、历史 cells 都没有变化，即使 viewport height 变化，也不生成 history update：

```rust
if revision_unchanged && metrics_unchanged && cells_unchanged {
    history_update = None;
}
```

但仍然要把新的 viewport height 传入 terminal，让 terminal 调整活动区域。

### 8.3 Terminal 的职责

`Terminal` 负责：

- 屏幕尺寸；
- 当前 viewport Rect；
- 上方历史 scroll region；
- 当前 buffer 和 previous buffer；
- cursor 位置；
- ANSI 输出和 flush。

`Terminal` 不应知道业务历史 cell，也不应决定什么是历史 replay。

`TerminalProjectionController` 负责：

- 从业务 snapshot 计算历史是否 append/replay；
- 把历史 cell 转为终端行；
- 生成 `PreparedHistoryProjection`。

`DrawCoordinator` 负责：

- 按固定顺序执行 viewport 调整、历史插入和 frame 绘制；
- 不再根据 viewport height 自己推导 FullReplay。

## 9. 建议实施顺序

### 阶段一：切断错误 replay 触发器

修改：

- `cli/src/app/runtime/terminal_projection.rs`
- `cli/src/app/runtime/terminal_projection_tests.rs`

要求：

1. 从 `full_replay` 条件中移除 `last_viewport_height != Some(viewport_height)`。
2. 从 history update 的“无变化”判断中移除 viewport height。
3. 保留 viewport height 作为 `PreparedHistoryProjection` 的布局输入。
4. 将现有测试“viewport 高度变化触发 FullReplay”改为“viewport 高度变化不产生 history update”。

这是最小修复，也是最应该先落地的一步。

### 阶段二：将非全屏 popup 从布局高度中分离

修改：

- `cli/src/ui/bottom_pane/input_pane/layout.rs`
- `cli/src/ui/bottom_pane/input_pane/render.rs`
- `cli/src/ui/bottom_pane/input_pane/esc_tests.rs`

要求：

1. `desired_height` 只返回输入框本体高度。
2. 非全屏 popup 使用输入框上方的 `popup_area`，并在绘制前清理该矩形。
3. popup 绘制顺序晚于 transcript，因此流式输出可以继续更新；被 popup 覆盖的区域属于正常 overlay 视觉层，不参与 transcript 状态。
4. popup 关闭后由下一帧重新绘制 transcript，不修改 `TranscriptScroll` 的跟随状态。

### 阶段三：补 viewport transition 测试

修改：

- `cli/src/terminal/custom_terminal_tests.rs`
- 必要时新增 `cli/src/terminal/viewport_transition_tests.rs`

覆盖：

- 活动 viewport 变高时，上方区域滚动，历史不被清空；
- 活动 viewport 变矮时，底部保持对齐；
- popup 出现和消失只改变 overlay，不改变 viewport，也不触发 history replay；
- viewport 从无历史区域扩展到有历史区域时，不写穿 viewport；
- viewport 变化后下一个 buffer diff 仍然以正确 Rect 为基准。

### 阶段四：把历史提交改成 pending queue

如果阶段一后仍有终端跳动，继续向 Codex 靠拢：

1. `TerminalProjectionController` 只产生 append/replay batch。
2. `TerminalGuard` 或 `DrawCoordinator` 暂存 append history lines。
3. 在一次 draw transaction 内统一执行：

```text
ensure viewport
-> 必要时局部清理
-> flush pending history lines
-> render current frame
-> buffer diff
-> flush
```

不要让业务事件处理函数直接产生终端输出。

### 阶段四：收口历史 replay

保留 FullReplay，但限制其来源：

- 终端宽度变化导致历史换行重新计算；
- 历史前缀被替换或删除；
- 会话恢复、重新加载历史；
- 明确的终端状态恢复。

不允许以下事件直接触发 FullReplay：

- 普通 server delta；
- 普通输入字符；
- `/` 补全 popup 打开/关闭；
- 活动消息高度变化；
- spinner 或状态栏动画。

### 阶段五：统一可观测性

建议为 projection 和 draw 增加 debug tracing，至少输出：

```text
frame_id
event_kind
scrollback_revision
scrollback_diff
metrics_changed
viewport_height_before
viewport_height_after
history_update_mode
history_line_count
terminal_viewport_before
terminal_viewport_after
```

正常输入 `/` 的日志应类似：

```text
event=key('/'), viewport_height=12->18,
history_diff=none, history_update=none
```

采用 overlay 布局后，正常日志中的 viewport 高度也应保持不变；上面的 `12->18` 仅用于说明旧实现的错误路径。

错误行为会显示为：

```text
event=key('/'), viewport_height=12->18,
history_update=full_replay, history_line_count=800
```

## 10. 必须建立的不变量

### 10.1 History append-only 不变量

如果当前历史 snapshot 只是旧 snapshot 的追加：

```text
previous[0..n] == current[0..n]
```

则只能使用 `Append`，不能使用 `FullReplay`。

### 10.2 Viewport 独立性不变量

只改变 viewport height 时：

```text
history_update == None
```

除非同时发生历史宽度变化或历史内容变化。

### 10.3 Draw 完整性不变量

每次 `Terminal::draw()` 的 render callback 必须完整绘制当前 viewport，而不是只绘制变化部分。buffer diff 负责减少 ANSI 输出，不负责修复缺失区域。

### 10.4 终端输出单提交点不变量

一帧中的 viewport 调整、历史插入、当前 buffer 绘制必须通过同一个 draw transaction 提交。业务状态更新不应直接向 stdout 写 ANSI。

### 10.5 光标不变量

历史插入完成后，cursor 必须恢复到当前 viewport 需要的位置；viewport 变化不能把 cursor 留在历史写入区。

## 11. 测试矩阵

### 11.1 Projection 单元测试

| 场景 | 预期 |
| --- | --- |
| 初次空历史 | 无 history update |
| 历史 append | `Append` |
| 历史前缀变化 | `FullReplay` |
| render width 变化 | `FullReplay` |
| 只有 viewport height 变化 | 无 history update |
| `/` popup 出现 | 无 viewport 变化、无 history update |
| popup 关闭 | 无 viewport 变化、无 history update |

### 11.2 Terminal 单元测试

| 场景 | 预期 |
| --- | --- |
| viewport 变高 | 上方区域局部滚动，不清空 scrollback |
| viewport 变矮 | viewport 保持底部对齐 |
| history append | 只写入新增历史行 |
| history replay | 清理并重建历史 |
| 空 history 插入 | 不改变当前 viewport |
| 当前 buffer 变短 | 正确清理旧行尾部 |

### 11.3 端到端终端测试

建议使用真实 ANSI/VT100 buffer 验证，而不是只检查 Rust 状态：

1. 预先写入大量历史消息。
2. 输入 `/`，确认 popup 出现。
3. 检查历史行内容和顺序没有变化。
4. 检查没有出现 `Clear(All)` 或 `Clear(Purge)`。
5. 关闭 popup，重复检查。
6. 流式输出长文本，逐步增加活动消息高度。
7. 检查历史只追加、不重复、不回滚。

Codex 已经有类似 VT100 测试思路，可参考：

```text
D:\codespace\codex\codex-rs\tui\tests\suite\vt100_history.rs
D:\codespace\codex\codex-rs\tui\tests\suite\resize_reflow.rs
```

## 12. 验收标准

修复完成后必须满足：

1. 输入 `/` 打开补全菜单时，已有历史消息不重新滚动、不重复、不消失。
2. 输入 `/` 不触发 `Clear(All)` 或 `Clear(Purge)`。
3. 流式回复增长时，不再因为每次高度变化清空整个终端。
4. 历史较多时，普通 frame 的输出量与新增内容规模相关，而不是与全部历史规模相关。
5. 历史前缀真正变化时，FullReplay 仍然可用。
6. 终端 resize 导致换行变化时，历史可以正确 reflow。
7. 手动 PageUp/Home 后，输入 `/`、关闭 popup、流式更新不会强制回到底部。
8. 光标始终位于输入区或预期的活动 viewport 位置。
9. projection、terminal、真实 VT100 测试全部通过。

## 13. 不建议的修复方式

以下方案只能缓解症状，不能作为最终架构：

### 13.1 单纯降低刷新频率

这会降低闪烁频率，但仍然会清屏和 replay。还会使流式输出变得迟钝。

### 13.2 在 clear 前后增加 sleep

这只会让闪烁更明显，也无法修复状态不一致。

### 13.3 每次只重绘最后几行

当前终端同时存在 scrollback 和 viewport，盲目局部重绘容易留下旧字符、错误背景和错误光标位置。必须先明确 viewport/scrollback 所有权。

### 13.4 把所有内容放回 ratatui buffer

这可以简化物理输出，但会改变 inline scrollback 的交互模型，属于更大范围的架构选择，不应作为第一步修复。

### 13.5 继续增加特殊条件

例如“slash popup 时不要 replay”“streaming 时延迟 replay”“某些历史数量以下允许 replay”。这些条件会继续扩大状态组合，不能替代正确的变化分类。

## 14. 最终设计判断

CloudAgent 不需要完全复制 Codex 的代码，但需要复制 Codex 的三个核心边界：

1. frame scheduler 只负责合并刷新请求，不决定历史重放。
2. history projection 只根据历史内容和渲染 metrics 决定 append/replay。
3. terminal viewport 高度变化只调整 viewport，不重建历史。

最终目标可以概括为：

```text
业务状态变化
    -> 生成明确的 projection

projection
    -> 区分 viewport update / history append / history replay

terminal draw transaction
    -> 调整 viewport
    -> 插入必要历史
    -> 绘制当前 frame
    -> 一次提交
```

只要这条边界稳定，流式消息闪烁、输入 `/` 时历史重新滚动、历史数量越多越明显这几个问题会同时消失，而不是分别添加补丁。

## 15. 开发态与发行态配置边界

终端刷新问题之外，配置读取也必须保持单一规则。此前 CLI 启动、`/model`、local-node、agentd 和 gateway 各自选择配置来源，导致启动实际模型与 `/model` 显示模型可能不同，IM 平台状态也可能落到另一套数据目录。

当前统一入口是 `config::AgentConfig::load_runtime()`：

| 模式 | 配置来源 | 数据/平台目录 |
| --- | --- | --- |
| 开发态（`cargo run`） | 用户配置作为基础，项目 `.cloudagent/config.toml` 覆盖它，项目 `configs/config.toml` 最高 | 项目 `.cloudagent/data` 和 `.cloudagent/platform` |
| 发行态 | 仅用户 `~/.cloudagent/config.toml` | 用户 `~/.cloudagent/data` 和 `~/.cloudagent/platform` |

开发态的 `/model` 会更新项目 `configs/config.toml`，并保留同文件中的 gateway、Feishu 等其他 TOML 配置；发行态才更新用户配置。local-node 和 agentd 使用同一个模式入口，gateway 也按相同的项目优先顺序选择配置文件。

IM 平台的运行凭据不是 LLM TOML 配置的一部分，而是 node 管理的 platform state。开发态 CLI 将项目数据目录传给 local-node，因此 `/gateway` 写入的是项目 `.cloudagent/platform`；发行态则写入用户平台目录。两者不能混用，否则会出现“模型来自项目、平台凭据来自用户”这类半隔离状态。

这套边界仍允许 `CLOUDAGENT_RELEASE_MODE=1` 强制当前进程使用发行态规则，便于验证发行行为；发布构建默认由编译模式进入发行态。后续若新增配置消费者，必须调用 `load_runtime()` 或接收已经解析好的 `AgentConfig`，不得重新实现一套 `load_user_only()`/路径搜索逻辑。

如果设置了 `CLOUDAGENT_CONFIG`，它是显式配置路径，所有组件都使用它作为唯一配置文件；这属于人为覆盖，不再参与项目/用户多文件合并。
