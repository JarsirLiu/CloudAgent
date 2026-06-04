# 开发说明

## 当前形态

CloudAgent 现在有四个明确的运行角色：

- `cloudagent`：产品入口
- `cli`：终端 surface
- `node`：本机常驻 host
- `agentd`：source worker 进程

仓库顶层结构：

```text
apps/      可执行程序入口
cli/       可复用终端 surface crate 与 cli 二进制
crates/    可复用 Rust crates
configs/   配置示例
docs/      当前架构文档
packaging/ 打包资源
scripts/   安装、升级、校验脚本
tests/     工作区级测试
web/       未来的 Web 工作区
```

## 运行边界

当前目标链路是：

```text
surface (cli / future web / IM)
  -> remote app-server client
  -> node
  -> worker(agentd)
  -> core
```

核心约束：

- surface 不直接调用 `agent-core`
- `node` 负责常驻生命周期、worker 调度、平台 runtime 管理与 transport host
- `agentd` 负责某个 source domain 的多会话控制与执行编排

## 目标形态

当前实现仍带有“按连接或按目录推导 worker scope”的历史痕迹，但目标形态不是“一会话一 worker”或“一目录一 worker”，而是以下模型：

- `node` 是全局常驻 supervisor
- `agentd` 是某个入口来源的长驻 worker controller
- `cli`、`web`、`IM` 分别作为独立的 source domain 接入
- 目录、权限、会话不再作为 worker 身份本身，而是执行上下文的一部分

目标链路仍然保持：

```text
surface (cli / future web / IM)
  -> remote app-server client
  -> node
  -> worker(agentd)
  -> core
```

但语义更新为：

- `surface`
  只负责入口交互，不负责 worker 身份判定
- `node`
  负责 source domain registry、session registry、workspace registry、worker pool 调度、故障恢复
- `agentd`
  负责其所属 source domain 下的多会话管理、turn 编排、tool runner 分发与事件上报
- `agent-core`
  负责核心推理与工具执行语义，不拥有 node/worker 拓扑策略

## 核心抽象

### `source domain`

入口来源标识，例如：

- `cli`
- `web`
- `wecom`
- `weixin`

worker 的一级归属按照 `source domain` 划分，而不是按照目录或单个会话划分。

### `worker instance`

某个 `source domain` 下的一个长驻 `agentd` 实例。

约束：

- 一个 `source domain` 可以只有一个 worker instance
- 也可以扩展为一个 worker pool
- session 绑定到 worker instance 由 `node` 调度决定

### `workspace`

逻辑工作区，不等于 worker 身份。

至少包含：

- `workspace_id`
- `root_path`
- 可选 repo 指纹
- 可选 data root / config root

### `session`

某个 surface 建立的交互会话。

至少包含：

- `session_id`
- `source domain`
- 默认 `workspace_id`
- 当前 conversation 指针
- 默认 permission profile

### `execution context`

某个 turn 实际执行时使用的上下文快照。

至少包含：

- `session_id`
- `conversation_id`
- `workspace_id`
- `cwd`
- permission profile
- env overlay
- model profile
- tool availability

规则：

- `cwd` 属于 execution context，不属于 worker 身份
- permission 变化在 turn 边界生效，不做运行中热迁移
- worker 可以同时承载多个 workspace、多个 session

## 可执行程序职责

### `apps/cloudagent`

产品级命令入口。

负责：

- `start/status/stop`
- 平台管理命令
- 拉起 CLI surface
- 发行版命令体验

不负责：

- 终端渲染细节
- worker 协议实现

### `cli`

终端 surface crate 与 `cli` 二进制。

负责：

- console 渲染
- 终端交互
- 本地 console bootstrap 辅助逻辑

不负责：

- 产品级生命周期命令路由
- 打包逻辑

### `apps/node`

本机常驻 host。

负责：

- remote app-server host
- 平台 runtime 生命周期
- source domain registry
- session 状态与 registry
- workspace registry
- worker pool 拉起、调度、复用、空闲回收
- worker 健康检查、熔断与重启
- turn 边界上的执行绑定策略

### `apps/agentd`

worker-oriented 二进制。

负责：

- source worker controller
- 所属 source domain 下的多 session / 多 conversation 管理
- turn queue 与执行编排
- tool runner 分发与生命周期管理
- 事件上报与故障上报
- stdio worker host
- 仅在明确需要时提供嵌入式开发 console 模式

## crates 边界

### `agent-core`

负责核心会话、turn、context、tool execution、approval 与 orchestration 语义。

额外约束：

- 不通过进程级 `current_dir` 推导运行中的 conversation 上下文
- 明确消费来自 node/worker 的 execution context

### `agent-app-server`

负责 app-server 的 command routing、projection、session state 与 server-request 协调。

### `agent-app-server-client`

负责共享的 in-process / remote app-server client 访问层。

所有 surface 都应该复用它，不要各自发明并行 client。

### `agent-gateway`

负责 IM adapter 逻辑与 gateway 侧抽象。

当前规则：

- IM 平台代码统一放在 `crates/agent-gateway/src/adapter/`
- 平台 adapter 一律通过 `AppServerClient::Remote -> node` 回到系统主链路

### `agent-model-provider`

负责模型 provider 适配。

### `agent-memory`

负责 agent 使用的 memory 抽象与支持逻辑。

### `agent-scheduler`

负责调度相关抽象与后续周期执行支持。

### `agent-tools`

负责工具定义、注册和工作区/系统工具实现。

### `config`

负责工作区和运行配置加载。

### `infra-*`

只负责底层基础设施接入：

- `infra-http`
- `infra-shell`
- `infra-ssh`
- `infra-store`

### `shared`

负责轻量公共类型与共享辅助逻辑。

## 清洁度规则

1. 产品入口、surface、resident node、worker 必须分层。
2. surface 不得绕过 `agent-app-server-client`。
3. IM adapter 不得定义并行的 node 协议。
4. 平台特定代码统一保留在 `agent-gateway/src/adapter/<platform>/`。
5. 新增二进制前，优先把重复 bootstrap/entry 逻辑提取成共享库模块。
6. 文档属于架构表面的一部分；过时迁移稿应删除，不要长期半维护。
7. 不允许把 `cwd`、permission profile 或 connection id 直接作为 worker 身份。
8. 不允许把“一会话一 worker”作为默认实现前提。
9. worker 内的会话与目录复用策略必须显式建模，不能依赖进程继承状态。

`node-worker` 改造文档统一收敛为：

- [`docs/node-worker-rebuild-plan.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-rebuild-plan.zh-CN.md)
  架构总览、职责边界、长期运行约束
- [`docs/node-worker-current-status.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-current-status.zh-CN.md)
  当前代码落地状态、自动回归结果、当前明确边界
- [`docs/node-worker-phase1-checklist.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-phase1-checklist.zh-CN.md)
  第一阶段实施清单与迁移顺序
- [`docs/node-worker-protocol-draft.zh-CN.md`](D:/learn/gifti/cloudagent/docs/node-worker-protocol-draft.zh-CN.md)
  协议、Rust 骨架与接口草案

## 本地校验

主要检查：

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --no-fail-fast
```

辅助脚本：

- `scripts/ci-check.sh`
- `scripts/ci-check.ps1`

在 Windows 上如果 `bash` 或 WSL 环境拿不到 `cargo`，优先使用 PowerShell 脚本。
