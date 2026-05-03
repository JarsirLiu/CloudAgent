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
CloudAgent is an agent built for remote control.

In phase one, it solves a simple but real problem: without logging into servers directly, I can still complete the full remote workflow, including project deployment, server monitoring, incident reporting, and automated handling.
CloudAgent embeds the token-compression strategy from [rtk](https://github.com/rtk-ai/rtk), activated via the `/filter` command, to significantly reduce token usage in long sessions while improving cache hit rates.
It also provides robust context orchestration, tool execution, and approval mechanisms to keep local coding and automation tasks accurate and reliable.

Its target users are straightforward: people who are extremely lazy and want to command multiple agents from a phone.
CloudAgent goes beyond local.

### Roadmap
In progress:
- [x] OpenAI-compatible model support
- [x] Tooling system
- [x] CLI (under active development)

Planned:
- [ ] MCP
- [ ] Skill
- [ ] Long-term memory
- [ ] Self-scheduling
- [ ] Multi-end interconnect
- [ ] Web console
- [ ] Multilingual support

### Quick Release Download
- GitHub Releases: [https://github.com/JarsirLiu/CloudAgent/releases](https://github.com/JarsirLiu/CloudAgent/releases)
- One-line install (Linux/macOS): `curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh | sh`
- One-line upgrade (Linux/macOS): `curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/upgrade.sh | sh`
- One-line uninstall (Linux/macOS): `curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh`

### Release Usage Commands
```bash
# start full CLI (default)
cloudagent start

# upgrade to latest release
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/upgrade.sh | sh

# uninstall
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh
```

### Configure API Key
CloudAgent reads config from default paths in this order:
- `~/.cloudagent/config.toml`
- `<workspace>/.cloudagent/config.toml`
- `<workspace>/configs/config.toml`

Recommended (global default):
```bash
mkdir -p ~/.cloudagent
cp configs/config.toml.example ~/.cloudagent/config.toml
# edit ~/.cloudagent/config.toml and set llm.api_key
# (only [llm] is required; other settings use defaults)
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
| `/help` | Show local command help |
| `/copy` | Copy the latest assistant reply |
| `/interrupt` | Interrupt the running turn (shortcut: `Ctrl+C`) |
| `/compact` | Compact older context into a summary |
| `/session <id>` | List sessions or switch to a session |
| `/new [session-id]` | Create and switch to a new session. Session ID is optional; press Enter after `/new` to open picker and use Up/Down to select |
| `/title <text>` | Set current session title |
| `/archive <id>` | Archive a conversation |
| `/delete <id>` | Hard delete a conversation |
| `/filter` | Configure pre-LLM input filter |
| `/permissions` | Set session permission mode |
| `/clear` | Clear this conversation |
| `/exit` | Exit CloudAgent |

---

<a id="中文"></a>

## CloudAgent

### 项目简介

CloudAgent 是一款面向远程操控的 Agent。

第一阶段，它要解决的是：我不用登录服务器，也能完成整套远程工作流，包括项目部署、服务器监控、突发事件上报与自动处理。
CloudAgent 内置了 [rtk](https://github.com/rtk-ai/rtk) 的 token 压缩思路，通过 `/filter` 命令启动压缩，在长会话里显著降低 token 消耗，同时提升缓存命中率。
同时，CloudAgent 提供了完整的上下文编排、工具执行与审批机制，保证本地编码和自动化任务执行得更稳、更准。

它适合的人群很直接：懒到极致、希望拿着手机就能远程指挥多端 Agent 干活的人。
CloudAgent 不止于本地。

### 开发进度（Roadmap）
已开发：
- [x] OpenAI 兼容模型
- [x] 工具系统
- [x] CLI（开发中）

未开发：
- [ ] MCP
- [ ] Skill
- [ ] 长期记忆
- [ ] 自我调度
- [ ] 多端互连
- [ ] Web 端
- [ ] 多语言支持

### Release 快速下载
- GitHub Releases: [https://github.com/JarsirLiu/CloudAgent/releases](https://github.com/JarsirLiu/CloudAgent/releases)
- 一键安装（Linux/macOS）: `curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh | sh`
- 一键升级（Linux/macOS）: `curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/upgrade.sh | sh`
- 一键卸载（Linux/macOS）: `curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh`

### 发行版使用命令
```bash
# 启动完整 CLI（默认）
cloudagent start

# 更新到最新发行版
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/upgrade.sh | sh

# 卸载
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh
```

### 配置 API Key
CloudAgent 默认按以下顺序读取配置：
- `~/.cloudagent/config.toml`
- `<workspace>/.cloudagent/config.toml`
- `<workspace>/configs/config.toml`

推荐方式（全局默认）：
```bash
mkdir -p ~/.cloudagent
cp configs/config.toml.example ~/.cloudagent/config.toml
# 编辑 ~/.cloudagent/config.toml，设置 llm.api_key
# （只需要 [llm]，其它配置使用默认值）
```

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
| `/help` | 显示本地命令帮助 |
| `/copy` | 复制最新一条 assistant 回复 |
| `/interrupt` | 中断当前运行中的 turn（快捷键：`Ctrl+C`） |
| `/compact` | 将旧上下文压缩为摘要 |
| `/session <id>` | 查看会话列表或切换到指定会话 |
| `/new [session-id]` | 新建并切换到会话。`session-id` 可省略；输入 `/new` 回车后可进入选择器，并可用上下方向键选择 |
| `/title <text>` | 设置当前会话标题 |
| `/archive <id>` | 归档会话 |
| `/delete <id>` | 永久删除会话 |
| `/filter` | 设置 pre-LLM 输入过滤 |
| `/permissions` | 设置会话权限模式 |
| `/clear` | 清空当前会话 |
| `/exit` | 退出 CloudAgent |

