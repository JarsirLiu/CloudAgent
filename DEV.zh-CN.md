# 开发说明

## 当前形态

CloudAgent 现在有四个明确的运行角色：

- `cloudagent`：产品入口
- `cli`：终端 surface
- `node`：本机常驻 host
- `agentd`：worker 进程

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
- `node` 负责常驻生命周期、worker 复用、平台 runtime 管理与 transport host
- `agentd` 负责单个 worker 会话的执行

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
- 会话状态与 registry
- worker 拉起、复用、空闲回收

### `apps/agentd`

worker-oriented 二进制。

负责：

- stdio worker host
- 仅在明确需要时提供嵌入式开发 console 模式

## crates 边界

### `agent-core`

负责核心会话、turn、context、tool execution、approval 与 orchestration 语义。

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
