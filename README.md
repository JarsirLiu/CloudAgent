<p align="center">
  <img src="https://img.shields.io/badge/Status-Active-success" alt="status">
  <img src="https://img.shields.io/badge/Language-Rust%20%7C%20TypeScript-blue" alt="language">
  <img src="https://img.shields.io/badge/Architecture-Agent%20System-orange" alt="architecture">
</p>

<p align="center">
  <a href="https://github.com/JarsirLiu/CloudAgent/stargazers"><img src="https://img.shields.io/github/stars/JarsirLiu/CloudAgent?style=social" alt="GitHub stars"></a>
  <a href="https://github.com/JarsirLiu/CloudAgent/network/members"><img src="https://img.shields.io/github/forks/JarsirLiu/CloudAgent?style=social" alt="GitHub forks"></a>
  <a href="https://github.com/JarsirLiu/CloudAgent/issues"><img src="https://img.shields.io/github/issues/JarsirLiu/CloudAgent" alt="GitHub issues"></a>
  <a href="https://github.com/JarsirLiu/CloudAgent/blob/main/LICENSE"><img src="https://img.shields.io/github/license/JarsirLiu/CloudAgent" alt="License"></a>
</p>

<p align="center">
  <a href="#english">English</a> •
  <a href="#中文">中文</a>
</p>

---

<a id="english"></a>

## CloudAgent

### Overview
CloudAgent is an agent designed for remote control, multi-device collaboration, and mobile-first operation. In the future, it will support logging in from any device, connecting to remote devices for continuous interaction, and creating scheduled or wake-up tasks that can be handled automatically. The long-term goal is for CloudAgent to become your "internet employee" — coordinating across devices for project development, deployment, monitoring, and more day-to-day remote workflows, all controllable from a single phone.

Today, CloudAgent already supports remote access through Feishu and personal WeChat. It uses a `node-worker` architecture with a lightweight resident process, on-demand worker startup, and idle recycling to reduce resource usage. The current version provides a CLI interface, supports any OpenAI-compatible model, accepts image input, and can be used directly for coding tasks. Its default context window is `258k`, and when usage approaches the threshold (`90%`), CloudAgent automatically compacts context to keep long conversations stable.

### Roadmap
In progress:
- [x] OpenAI-compatible model support
- [x] CLI interaction
- [x] Image input
- [x] Feishu remote access
- [x] Personal WeChat remote access
- [x] Automatic context compaction

Planned:
- [ ] MCP
- [x] Skills (workspace/user local skill packages, initial local skill support)
- [ ] Long-term memory
- [ ] Web access

### Install
GitHub Releases: [https://github.com/JarsirLiu/CloudAgent/releases](https://github.com/JarsirLiu/CloudAgent/releases)

Release process: [docs/release-process.md](docs/release-process.md)

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh | sh
```

Windows:

```bash
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.ps1 | iex
```

Common runtime commands:

```bash
# show the top-level help
cloudagent

# start the local node service
cloudagent start

# open the CLI
cloudagent cli

# show local node status
cloudagent status

# stop the local node service
cloudagent stop

# upgrade CloudAgent to the latest release
# if the local node is running, upgrade will stop it first and restart it after install
cloudagent upgrade

# uninstall CloudAgent but keep user data
cloudagent uninstall

# uninstall CloudAgent and delete user data
cloudagent uninstall --purge
```

`cloudagent` with no arguments shows the top-level help. `cloudagent cli` opens the interactive CLI surface. Unknown commands such as `cloudagent xxx` now fail fast with an "unknown command" error instead of falling back to the CLI.

For the full release, install, upgrade, rollback, beta, and uninstall standard, see [docs/release-process.md](docs/release-process.md).

### Permissions
CloudAgent currently supports three session permission modes:

| Mode | Description |
|---|---|
| `ReadOnly` | Read operations run directly; writes and other changes require approval |
| `WorkspaceWrite` | Workspace writes run directly; outside-workspace actions, network commands, and dangerous commands require approval |
| `FullAccess` | Workspace and outside-workspace actions usually run directly; dangerous commands still require approval |

Default mode: `WorkspaceWrite`

### Configure API Key
CloudAgent reads config from default paths in this order:
- `~/.cloudagent/config.toml`
- `<workspace>/.cloudagent/config.toml`
- `<workspace>/configs/config.toml`

CloudAgent skills are discovered from:
- `<workspace>/.cloudagent/skills/`
- `~/.cloudagent/skills/`

Skill packages use a `SKILL.md` entry file with YAML frontmatter. The metadata shape is:

```md
---
name: repo-reader
description: Use this skill when the user needs repository structure analysis.
policy:
  allow_implicit_invocation: true
dependencies:
  tools: [rg, git]
---
```

`policy.allow_implicit_invocation` defaults to enabled when omitted. Set it to `false` for explicit-only skills.

Recommended:
```bash
# 1) start node
cloudagent start

# 2) open CLI
cloudagent cli

# 3) inside CLI, run:
/config
```

`/config` is the preferred setup path for `api_key`, `base_url`, and `model`.

If you need to edit `~/.cloudagent/config.toml` manually, use a config like:

```toml
[llm]
base_url = "https://api.openai.com/v1"
api_key = "replace-with-your-api-key"
model = "gpt-4.1-mini"
temperature = 0.2
```

### Local Development Startup
```bash
# 1) Clone
git clone https://github.com/JarsirLiu/CloudAgent.git
cd CloudAgent

# 2) Start CLI (dev mode)
cargo run -p cli
```

### CLI Quick Commands
| Command | Description |
|---|---|
| `/config` | Configure OpenAI-compatible `api_key`, `base_url`, and `model` |
| `/skill <name>` | Insert a discovered skill into the composer |
| `/skills` | List discovered skills |
| `/help` | Show local command help |
| `/copy` | Copy the latest assistant reply |
| `/interrupt` | Interrupt the running turn |
| `/compact` | Compact older context into a summary |
| `/session [id]` | List sessions or switch to a session. If `id` is omitted, you can choose from the session list |
| `/new [session-id]` | Create and switch to a new session. Session ID is optional |
| `/title <text>` | Set current session title |
| `/archive <id>` | Archive the specified conversation |
| `/delete [id]` | Hard delete a conversation. If `id` is omitted, you can choose from the session list |
| `/filter` | Set the pre-LLM input filter |
| `/permissions` | Set the model execution permission mode |
| `/gateway` | Configure and connect IM platforms |
| `/clear` | Clear this conversation |
| `/exit` | Exit CloudAgent |

---

<a id="中文"></a>

## CloudAgent

### 项目简介

CloudAgent 是一款面向远程操控的 Agent，目标是服务于多端互连、远程协同和移动控制场景。未来，它将支持用户在任意设备登录后，与其他远端设备建立连接并持续交互；同时也将支持任务创建、定时唤醒与自动处理机制。最终，CloudAgent 将成为您的“互联网员工”，通过多端协同完成项目开发、部署、监控，以及更多日常远程操作与自动化工作，而这一切只需要一部手机即可完成。

目前，CloudAgent 已支持飞书、个人微信远程接入，并采用 `node-worker` 架构：通过轻量常驻进程承载基础能力，在需要时按会话拉起 worker，空闲后自动回收，以尽可能节省系统资源。当前版本已提供 CLI 交互，支持任意 OpenAI 兼容模型接入与图片输入，可直接用于代码编写与日常任务处理。默认上下文窗口为 `258k`，当上下文使用接近阈值（`90%`）时，会自动执行压缩，以提升长对话场景下的稳定性。

### 开发进度（Roadmap）
已开发：
- [x] OpenAI 兼容模型
- [x] CLI 交互
- [x] 图片输入
- [x] 飞书远程接入
- [x] 个人微信远程接入
- [x] 自动上下文压缩

未开发：
- [ ] MCP
- [x] Skill（已支持 workspace / user 本地 skill 包，当前为首版本地 skill 能力）
- [ ] 长期记忆
- [ ] Web 访问

### 安装
GitHub Releases: [https://github.com/JarsirLiu/CloudAgent/releases](https://github.com/JarsirLiu/CloudAgent/releases)

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh | sh
```

Windows:

```bash
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.ps1 | iex
```

常用运行命令：

```bash
# 显示顶层帮助
cloudagent

# 启动本机 node 服务
cloudagent start

# 打开 CLI
cloudagent cli

# 查看本机 node 状态
cloudagent status

# 停止本机 node 服务
cloudagent stop

# 将 CloudAgent 升级到最新版本
# 如果本机 node 正在运行，upgrade 会先停止它，安装后再自动拉起
cloudagent upgrade

# 卸载 CloudAgent，但保留用户数据
cloudagent uninstall

# 卸载 CloudAgent，并删除用户数据
cloudagent uninstall --purge
```

`cloudagent` 在没有参数时会显示顶层帮助。`cloudagent cli` 才会打开交互式 CLI。像 `cloudagent xxx` 这样的未知命令现在会直接报错并提示可用命令，不再回退到 CLI。

`cloudagent uninstall` 默认会保留用户数据目录，并移除安装器创建的 CloudAgent PATH 项。使用 `cloudagent uninstall --purge` 可以同时删除用户数据。

### 权限
CloudAgent 当前支持三种会话权限模式：

| 模式 | 说明 |
|---|---|
| `ReadOnly` | 读操作可直接执行；写入和其他变更需要审批 |
| `WorkspaceWrite` | 工作区内写操作可直接执行；工作区外操作、网络命令和危险命令需要审批 |
| `FullAccess` | 工作区内外操作通常可直接执行；危险命令仍需要审批 |

默认模式：`WorkspaceWrite`

### 配置 API Key
CloudAgent 默认按以下顺序读取配置：
- `~/.cloudagent/config.toml`
- `<workspace>/.cloudagent/config.toml`
- `<workspace>/configs/config.toml`

CloudAgent skill 默认从以下目录发现：
- `<workspace>/.cloudagent/skills/`
- `~/.cloudagent/skills/`

Skill 包使用 `SKILL.md` 作为入口文件，并在文件顶部使用 YAML frontmatter。元数据结构如下：

```md
---
name: repo-reader
description: 当用户需要分析仓库结构时使用这个 skill。
policy:
  allow_implicit_invocation: true
dependencies:
  tools: [rg, git]
---
```

如果省略 `policy.allow_implicit_invocation`，默认会允许隐式调用；只有显式触发的 skill 才建议把它设为 `false`。

推荐方式：
```bash
# 1) 启动 node
cloudagent start

# 2) 打开 CLI
cloudagent cli

# 3) 在 CLI 中执行：
/config
```

`/config` 是配置 `api_key`、`base_url` 和 `model` 的首选方式。

如果需要手工编辑 `~/.cloudagent/config.toml`，可以使用下面这个最小示例：

```toml
[llm]
base_url = "https://api.openai.com/v1"
api_key = "replace-with-your-api-key"
model = "gpt-4.1-mini"
temperature = 0.2
```

### 安装位置
Linux / macOS：

- 数据目录：`~/.cloudagent`
- 已安装二进制：`~/.local/lib/cloudagent/current`
- 启动器：`~/.local/bin`

Windows：

- 数据目录：`%USERPROFILE%\\.cloudagent`
- 已安装二进制：`%LOCALAPPDATA%\\CloudAgent\\current`
- 启动器：`%USERPROFILE%\\.local\\bin\\cloudagent.cmd`

### 本地开发启动
```bash
# 1) 克隆仓库
git clone https://github.com/JarsirLiu/CloudAgent.git
cd CloudAgent

# 2) 启动 CLI（开发模式）
cargo run -p cli
```

### CLI 快捷命令表
| 命令 | 说明 |
|---|---|
| `/config` | 配置 OpenAI 兼容模型的 `api_key`、`base_url` 和 `model` |
| `/skill <name>` | 将已发现的 skill 插入输入框 |
| `/skills` | 列出当前发现到的 skill |
| `/help` | 显示本地命令帮助 |
| `/copy` | 复制最新一条 assistant 回复 |
| `/interrupt` | 中断当前运行中的 turn |
| `/compact` | 将旧上下文压缩为摘要 |
| `/session [id]` | 查看会话列表或切换到指定会话；省略 `id` 时可在列表中选择 |
| `/new [session-id]` | 新建并切换到会话，`session-id` 可省略 |
| `/title <text>` | 设置当前会话标题 |
| `/archive <id>` | 归档指定会话 |
| `/delete [id]` | 永久删除会话；省略 `id` 时可在列表中选择 |
| `/filter` | 设置 pre-LLM 输入过滤 |
| `/permissions` | 设置会话权限模式 |
| `/gateway` | 配置并连接 IM 平台 |
| `/clear` | 清空当前会话 |
| `/exit` | 退出 CloudAgent |
