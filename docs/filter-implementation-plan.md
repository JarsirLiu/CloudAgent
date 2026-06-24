# CloudAgent filter 具体实施手册

> 目标：把 CloudAgent 的 pre-LLM filter 做成一套**可以直接照着改代码**的实施手册。
>
> 范围：只处理当前真正会进入上下文的 **`StructuredToolResult::CommandExecution`** 历史工具输出。
>
> 不处理：`read_file`、`search_workspace`、`get_metadata`。

---

## 1. 设计目标

CloudAgent 现在已经有基础 filter，但要进一步做到：

- 历史中的工具输出更短
- 仍保留模型决策所需信息
- 实现上高内聚、低耦合
- 命令级策略可独立演进

这份手册不是理念文档，而是**逐文件、逐函数**说明怎么改。

---

## 2. 现有代码入口与职责划分

### 2.1 入口层

**文件**：`crates/agent-core/src/context/facade.rs`

**现有函数**：

- `ContextFacade::apply_pre_llm_filter(...)`

**职责**：

- 作为 filter 的统一入口
- 不承载具体过滤逻辑

**改动要求**：

- 这里不要加新的命令判断逻辑
- 只负责调用 `ContextInputFilterService`
- 保持这一层“薄”

---

### 2.2 策略编排层

**文件**：`crates/agent-core/src/context/input_filter/mod.rs`

**现有职责**：

- 遍历历史消息
- 找到工具输出
- 判断是否是结构化结果
- 选择对应 adapter

**这里是整个 filter 的编排中心**，但不要把具体解析逻辑继续堆进来。

**改动原则**：

- `mod.rs` 只做分发、分类、组合
- 具体输出格式生成放进各个 adapter
- 通用清洗放进 `pipeline.rs`

---

### 2.3 通用清洗层

**文件**：`crates/agent-core/src/context/input_filter/pipeline.rs`

**现有职责**：

- 去 ANSI
- 去空行
- 去 progress
- 长行截断
- 超长输出 head/tail 压缩

**改动原则**：

- 只保留“所有命令都通用”的处理
- 不写命令特化逻辑
- 不写 Git / Cargo / Test / Install 的业务规则

---

### 2.4 命令专用 adapter 层

**目录**：`crates/agent-core/src/context/input_filter/adapters/`

当前已有：

- `git.rs`
- `rust.rs`
- `tests.rs`
- `install.rs`

**职责**：

- 每个文件负责一个命令族
- 输出自己的摘要文本
- 尽量不依赖别的 adapter

**改动原则**：

- 一个 adapter 只处理一个职责域
- 不要在 `git.rs` 里写 Cargo 规则
- 不要把通用清洗再复制一遍

---

## 3. 架构约束：必须高内聚低耦合

为了保证质量，filter 代码必须遵守以下边界：

### 3.1 `mod.rs` 只负责调度

它可以做：

- 解析 `StructuredToolResult::CommandExecution`
- 根据命令分类选择 adapter
- 决定是否 passthrough

它不能做：

- 复杂字符串扫描
- 输出摘要拼接
- 具体命令的逐行规则

### 3.2 adapter 只负责自己领域

例如：

- `git.rs` 只管 git 系列
- `rust.rs` 只管 cargo/rust 系列
- `tests.rs` 只管测试运行器系列
- `install.rs` 只管安装类输出

### 3.3 `pipeline.rs` 只做通用文本降噪

通用规则只能是：

- strip ANSI
- 去空行
- 去 progress
- 截断过长行
- 超长内容做 head/tail 保留

### 3.4 新命令规则优先放 adapter

如果后续要支持 `git log` 或 `cargo check`，优先新增 adapter 内部函数，不要把 `mod.rs` 变成大杂烩。

---

## 4. 具体实施步骤

---

### Step 1：细化命令分发

**文件**：`crates/agent-core/src/context/input_filter/mod.rs`

**目标函数**：

- `CommandInvocation::family()`
- `filter_command_execution_output(...)`

#### 1.1 现状问题

当前 `family()` 只粗分为：

- Git
- Cargo
- TestRunner
- Install
- Generic

这不够，因为后面要对 `git diff`、`git log`、`cargo test`、`cargo build` 做更精细处理。

#### 1.2 要做的改动

建议把 `CommandFamily` 扩充为更细粒度：

- `GitStatus`
- `GitDiff`
- `GitLog`
- `CargoTest`
- `CargoBuild`
- `CargoInstall`
- `TestRunner`
- `Install`
- `Generic`

如果暂时不想完全重构 enum，也至少要在 `mod.rs` 里把 `git diff` 和 `git log` 从 `Git` 中分出来。

#### 1.3 推荐实现方式

在 `CommandInvocation` 上新增轻量判断函数，例如：

- `is_git_diff()`
- `is_git_log()`
- `is_git_status()`
- `is_cargo_test()`
- `is_cargo_build()`
- `is_cargo_install()`

这样 `family()` 只做分类，不写摘要逻辑。

#### 1.4 设计原则

- `family()` 返回尽可能稳定的枚举
- 不在这里拼 summary 字符串
- 不在这里扫描输出内容

---

### Step 2：把 `git.rs` 改成真正的 git 摘要模块

**文件**：`crates/agent-core/src/context/input_filter/adapters/git.rs`

**目标函数**：

- `filter_git_output(...)`

#### 2.1 现状问题

现在 `filter_git_output()` 只做了：

- `git status`：统计 changed files
- `git diff`：统计加减行
- 其余走通用清洗

这还不够细。

#### 2.2 拆分建议

建议在 `git.rs` 内部拆成几个私有函数：

- `filter_git_status_output(...)`
- `filter_git_diff_output(...)`
- `filter_git_log_output(...)`

然后 `filter_git_output(...)` 只做路由：

```text
if status -> filter_git_status_output
if diff   -> filter_git_diff_output
if log    -> filter_git_log_output
else      -> filter_tool_output
```

#### 2.3 `git diff` 具体策略

目标不是保留全量 diff，而是保留“可决策信息”。

推荐输出结构：

```text
[rtk:git]
Git diff summary: files=3, +128 / -47
changed files:
- src/foo.rs
- src/bar.rs
- Cargo.toml
key hunks:
...少量关键片段...
```

建议规则：

- 统计文件名
- 统计 `+` / `-`
- 对超长 diff 只保留前若干个文件块
- 每个文件块只保留少量关键上下文
- 丢弃中间大段重复上下文

#### 2.4 `git status` 具体策略

保留：

- modified
- new file
- deleted
- renamed

输出建议：

```text
[rtk:git]
Git status: 4 changed files
modified: src/main.rs
new file: src/lib.rs
deleted: old.txt
renamed: a.rs -> b.rs
```

#### 2.5 `git log` 具体策略

如果要支持，优先做摘要而不是 patch：

- commit hash
- subject
- author（可选）
- date（可选）

不要保留大段正文，除非是单条关键提交。

#### 2.6 这个文件应该避免的事

- 不要引入 Cargo/test/install 的逻辑
- 不要把通用文本清洗复制进来
- 不要在这里处理 struct 以外的历史去重

---

### Step 3：把 `rust.rs` 拆成 build/test 两类摘要

**文件**：`crates/agent-core/src/context/input_filter/adapters/rust.rs`

**目标函数**：

- `filter_rust_build_test_output(...)`

#### 3.1 现状问题

当前它只是统计：

- `errors`
- `warnings`

然后追加通用 `filter_tool_output()`。

这对省 token 来说还不够。

#### 3.2 建议拆分

建议拆成两个函数：

- `filter_cargo_test_output(...)`
- `filter_cargo_build_output(...)`

如果短期不想改函数签名，也至少内部按命令名再分流。

#### 3.3 `cargo test` 策略

目标：失败优先，成功简化。

保留：

- failed tests
- error blocks
- panic
- note
- failing crate / test name

压缩：

- `running ...`
- `finished ...`
- 大量重复 passed 输出
- 成功测试细节

建议 summary 格式：

```text
Cargo test summary: 1 failed, 42 passed
error: ...
note: ...
```

#### 3.4 `cargo build` 策略

目标：错误优先，warning 聚合。

保留：

- error
- failed
- note
- 关键 crate 名

压缩：

- build 进度
- 重复 warning
- 成功编译日志

建议 summary 格式：

```text
Cargo build summary: 1 error, 12 warnings
error: ...
note: ...
```

#### 3.5 这个文件应该避免的事

- 不要写 git/install/test-runner 的分类逻辑
- 不要重复通用清洗
- 不要直接拼 `generic` 兜底结构

---

### Step 4：把 `tests.rs` 做成测试摘要模块

**文件**：`crates/agent-core/src/context/input_filter/adapters/tests.rs`

**目标函数**：

- `filter_test_output(...)`

#### 4.1 现状问题

当前只是统计 passed/failed，然后追加通用清洗。

#### 4.2 改动方向

建议新增内部 helper：

- `summarize_test_runs(...)`
- `extract_failed_tests(...)`

#### 4.3 输出策略

保留：

- passed / failed 数量
- 失败测试名
- 失败摘要

压缩：

- 全量通过输出
- 运行过程日志
- 重复栈跟踪的非关键部分

#### 4.4 pytest / python -m pytest

你当前代码已经识别了 `pytest` 和 `python -m pytest`，所以测试摘要模块也要兼容这些输出。

#### 4.5 这个文件应该避免的事

- 不要做 install 逻辑
- 不要做 cargo 编译细节
- 不要把 diff 处理搬进来

---

### Step 5：把 `install.rs` 变成安装日志摘要模块

**文件**：`crates/agent-core/src/context/input_filter/adapters/install.rs`

**目标函数**：

- `filter_install_output(...)`

#### 5.1 现状问题

当前逻辑只是保留关键词行：

- error
- warning
- added
- installed
- audited
- finished
- compiling

#### 5.2 改动方向

建议把它升级为更明确的安装 summary：

保留：

- 安装成功/失败结论
- 关键包名
- 错误摘要
- 警告摘要

压缩：

- 下载进度
- 无意义的重复状态行
- 网络 fetch 过程细节

#### 5.3 推荐输出模板

```text
Install summary:
installed: ripgrep
status: success
warnings: 2
```

或者失败时：

```text
Install summary:
status: failed
error: ...
```

#### 5.4 这个文件应该避免的事

- 不要加入 generic 清洗外的共用逻辑
- 不要处理 git/cargo/test 的摘要规则

---

### Step 6：保持 `pipeline.rs` 纯粹

**文件**：`crates/agent-core/src/context/input_filter/pipeline.rs`

**目标函数**：

- `filter_tool_output(...)`
- `truncate_line(...)`
- `looks_like_progress(...)`
- `strip_ansi(...)`

#### 6.1 改动原则

这里只允许做“通用文本净化”。

可做：

- 更稳的 progress 识别
- 更稳的 ANSI 清除
- 超长文本的 head/tail 保留策略微调

不要做：

- 识别 git diff 块
- 识别 cargo warning
- 识别 test failure

#### 6.2 这里的职责边界

如果某个规则开始依赖命令语义，就应该挪回 adapter。

---

### Step 7：调整 `mod_tests.rs` 为主测试入口

**文件**：`crates/agent-core/src/context/input_filter/mod_tests.rs`

**目标**：

- 覆盖 `filter_command_execution_output(...)`
- 覆盖新的分流逻辑
- 覆盖 passthrough 逻辑

#### 7.1 必加测试类别

1. `git diff` 压缩测试
2. `git status` 摘要测试
3. `cargo test` 失败摘要测试
4. `cargo build` warning 聚合测试
5. `install` 成功摘要测试
6. `pytest` 摘要测试
7. `--nocapture` passthrough 测试

#### 7.2 测试风格要求

- 输入要尽量真实
- 断言不要只看“有前缀”
- 要同时验证关键信息保留和冗余信息消失

#### 7.3 推荐补法

先写“压缩前很长，压缩后明显变短”的用例，再细化错误信息保留。

---

## 5. 推荐改造顺序

按收益和风险排序，建议这样做：

### 第一阶段：最小可行改造

1. `mod.rs`
   - 细化命令识别
   - 把 `git diff` / `git log` / `cargo test` / `cargo build` 分开

2. `git.rs`
   - `git diff` 做结构化摘要
   - `git status` 摘要更明确

3. `mod_tests.rs`
   - 补最关键回归测试

### 第二阶段：扩展高收益命令

4. `rust.rs`
   - build/test 分流
   - 错误优先摘要

5. `install.rs`
   - 安装日志进一步压缩

6. `tests.rs`
   - 失败测试定位增强

### 第三阶段：补充命令覆盖

7. `git.rs`
   - `git log`

8. `mod.rs`
   - 支持更多命令族

---

## 6. 新增代码组织建议

如果后面感觉 `adapters/` 继续变大，可以按下面方式演进：

### 方案 A：保持当前结构

- `mod.rs`
- `pipeline.rs`
- `adapters/git.rs`
- `adapters/rust.rs`
- `adapters/tests.rs`
- `adapters/install.rs`

适合当前阶段，改动最小。

### 方案 B：进一步拆模块

未来如果 `git.rs` 太大，可以拆成：

- `adapters/git/status.rs`
- `adapters/git/diff.rs`
- `adapters/git/log.rs`

如果 `rust.rs` 太大，可以拆成：

- `adapters/rust/build.rs`
- `adapters/rust/test.rs`

但这个阶段先不要过度拆，避免文件数量失控。

---

## 7. 验收标准

实施完后，至少满足：

1. `git diff` 输出明显变短
2. `cargo test` / `cargo build` 保留错误但去掉大量噪声
3. `install` 日志不再把过程细节全部塞给模型
4. `pytest`/`python -m pytest` 有稳定摘要
5. `--nocapture` 等详细模式能正确 passthrough
6. filter 代码仍然保持高内聚低耦合

---

## 8. 代码质量要求

### 8.1 单一职责

每个文件只处理自己的命令域。

### 8.2 依赖方向单向

推荐依赖方向：

`mod.rs` -> `adapters/*` -> `pipeline.rs`

不要反向依赖。

### 8.3 不要复制通用逻辑

通用清洗只保留在 `pipeline.rs`。

### 8.4 新规则先加测试

每加一个摘要规则，必须加一条最小回归测试。

---

## 9. 具体开发任务清单

### 任务 1

**文件**：`crates/agent-core/src/context/input_filter/mod.rs`

**改动**：

- 扩展 `CommandFamily`
- 拆分 `git` / `cargo` 识别
- 增加 `git diff`、`git log`、`cargo test`、`cargo build` 的判断函数

### 任务 2

**文件**：`crates/agent-core/src/context/input_filter/adapters/git.rs`

**改动**：

- 拆分 `git status` / `git diff` / `git log`
- 实现 diff 文件级摘要

### 任务 3

**文件**：`crates/agent-core/src/context/input_filter/adapters/rust.rs`

**改动**：

- `cargo test` / `cargo build` 分开处理
- 错误优先输出

### 任务 4

**文件**：`crates/agent-core/src/context/input_filter/adapters/tests.rs`

**改动**：

- 强化失败测试摘要
- 压缩成功输出

### 任务 5

**文件**：`crates/agent-core/src/context/input_filter/adapters/install.rs`

**改动**：

- 安装日志压缩
- 成功/失败结果显式化

### 任务 6

**文件**：`crates/agent-core/src/context/input_filter/mod_tests.rs`

**改动**：

- 为上述每类摘要规则增加测试

---

## 10. 最后建议

如果只做一轮，优先顺序建议是：

1. `git diff`
2. `cargo test`
3. `cargo build`
4. `install`
5. `pytest`

这几类最容易带来 token 下降，也最能体现 RTK 风格的“命令专用摘要”。
