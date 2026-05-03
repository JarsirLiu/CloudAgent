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

<a id="中文"></a>
## 中文

### 项目简介
CloudAgent 是一款面向远程操控的 Agent。

第一阶段，它要解决的是：我不用登录服务器，也能完成整套远程工作流，包括项目部署、服务器监控、突发事件上报与自动处理。
CloudAgent 内置了 [rtk](https://github.com/rtk-ai/rtk) 的 token 压缩思路，通过 `/filter` 命令启动压缩，在长会话里显著降低 token 消耗，同时提升缓存命中率。
同时，CloudAgent 提供了完整的上下文编排、工具执行与审批机制，保证本地编码和自动化任务执行得更稳、更准。

它适合的人群很直接：懒到极致、希望拿着手机就能远程指挥多端 Agent 干活的人。
CloudAgent 不止于本地。
### 技术栈
- Agent Runtime: Rust（workspace）+ TypeScript（Web/Frontend）
- LLM 接入: OpenAI-compatible APIs
- Memory: 短期上下文压缩 + 缓存策略
- Infra: MCP（规划中）/ Skills（规划中）/ 远程互连（规划中）

### 当前亮点
- 对话历史压缩（已完成）
- 说明：采用类似 RTK 的思路，对上下文进行压缩与结构化裁剪
- 收益：显著降低 token 消耗，减少冗余上下文噪音
- 高缓存命中率（已完成）
- 说明：针对重复任务和相似上下文进行缓存复用
- 收益：在连续会话中有明显的成本与延迟收益

### 开发进度（Roadmap）
- [x] 压缩对话历史，节省大量 token
- [x] 高缓存命中率优化
- [ ] 长期记忆（Long-term Memory）
- [ ] MCP 集成
- [ ] Skill 机制
- [ ] Agent 自动唤醒机制
- 范围：用于服务器/监控场景
- 能力：触发告警后自动唤醒 Agent
- 目标：向用户报告状态并执行处置流程
- [ ] 远程互连机制
- 示例：隔空投送等跨端/跨设备能力

### 愿景
从“能对话的 Agent”进化到“可持续值守、可自动响应、可跨端协同”的智能系统。

---

<a id="english"></a>
## English

### Overview
CloudAgent is an agent built for remote control.

In phase one, it solves a simple but real problem: without logging into servers directly, I can still complete the full remote workflow, including project deployment, server monitoring, incident reporting, and automated handling.
CloudAgent embeds the token-compression strategy from [rtk](https://github.com/rtk-ai/rtk), activated via the `/filter` command, to significantly reduce token usage in long sessions while improving cache hit rates.
It also provides robust context orchestration, tool execution, and approval mechanisms to keep local coding and automation tasks accurate and reliable.

Its target users are straightforward: people who are extremely lazy and want to command multiple agents from a phone.
CloudAgent goes beyond local.
### Tech Stack
- Agent Runtime: Rust (workspace) + TypeScript (Web/Frontend)
- LLM Access: OpenAI-compatible APIs
- Memory: Context compression + cache strategy
- Infra: MCP (planned) / Skills (planned) / Remote interconnect (planned)

### Highlights
- Conversation History Compression (Done)
- Notes: RTK-style context compression and structural trimming
- Impact: Significantly reduces token usage and noisy context
- High Cache Hit Rate (Done)
- Notes: Reuses cached outputs for repeated tasks and similar contexts
- Impact: Improves both cost efficiency and latency in ongoing sessions

### Roadmap
- [x] Conversation history compression with major token savings
- [x] High cache-hit optimization
- [ ] Long-term memory
- [ ] MCP integration
- [ ] Skill mechanism
- [ ] Agent auto-wakeup mechanism
- Scope: For server/monitoring scenarios
- Capability: Auto-wakes the agent on alerts
- Goal: Reports status and handles issues proactively
- [ ] Remote interconnect mechanism
- Example: Cross-device features such as remote drop/transfer workflows

### Vision
Evolve from a chat-capable agent into a persistent, self-responsive, cross-device collaborative agent system.


