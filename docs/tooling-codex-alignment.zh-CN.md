# CloudAgent 工具系统设计与对齐路线

这份文档定义 `cloudagent` 当前工具系统的设计目标、对标对象、现状判断和落地路线。

它不是阶段性施工 checklist，也不是某一次重构的临时说明，而是一份长期有效的工具系统设计文档。

## 目标

`cloudagent` 的工具系统要同时满足四个目标：

- 让模型更少试探、更少重复调用，就能定位到问题代码
- 让搜索结果、读文件结果、命令结果都能直接指导下一步动作
- 让默认工具面足够小，但又不牺牲复杂场景下的可发现能力
- 让工具运行、权限、并行、输出投影形成稳定的系统，而不是一堆分散的 helper

这意味着工具系统不能只追求“工具很多”，也不能只追求“工具很像某个产品表面长相”。

真正要对齐的是：

- 定位问题时的调用路径
- 工具结果对下一步动作的引导能力
- 默认工具暴露与延迟发现的边界
- 工具运行与调度的系统化程度

## 对标对象

当前主要对标两个现成系统：

- `Codex`
- `Claude Code`

它们并不强在同一个地方。

### Codex 的强项

Codex 更强的是“工具系统作为平台”的完整度：

- 默认工具面、延迟发现、外部工具接入之间的边界清楚
- `tool_search`、`tool_suggest`、dynamic tools、MCP tools 形成统一发现链路
- 工具注册、路由、审批、并行、运行时输出投影是系统化设计
- `apply_patch`、`exec_command`、shell、agent tools 的职责边界更成熟

简单说，Codex 更像一个完整的工具平台。

### Claude Code 的强项

Claude Code 更强的是“代码仓库定位主链”的低摩擦程度：

- `Glob`
- `Grep`
- `FileRead`
- `FileEdit`
- `Bash`

这些原语围绕“快速找文件、快速搜内容、快速精读”形成了很短的调用路径。

简单说，Claude Code 更像一个对代码定位高度优化的工具链。

## 核心判断

对 `cloudagent` 来说，单纯模仿 Codex 或单纯模仿 Claude Code 都不是最优解。

推荐方向是：

- 搜索与读取主链，优先学习 Claude Code
- 工具发现、延迟暴露、运行调度，优先学习 Codex

一句话总结：

> 用 Claude Code 的“仓库定位效率”，加上 Codex 的“工具系统完整度”。

## 当前系统现状

`cloudagent` 已经具备基础工具系统，但整体仍处于“第一代可用版”。

### 已有能力

- 有稳定的默认主链工具：
  - `search_workspace`
  - `read_file`
  - `exec_command`
- 有延迟暴露和 discoverable tools
- 有 `tool_search`
- 有工具批量执行、审批、顺序/并行执行分流
- 有结构化工具结果

### 当前短板

短板主要集中在两层：

1. 搜索与读取主链仍不够低摩擦
2. 工具发现与调度系统还不够成熟

更具体地说：

- `search_workspace` 仍然是单工具承载“文件查找 + 内容搜索”两类职责
- 文本搜索虽已加入排序、文件级聚合、语义模板，但仍偏本地启发式
- 缺少像 Claude Code 那样明确的一等搜索原语分工
- `tool_search` 目前仍是轻量字符串打分，不是成熟检索系统
- discoverable tools 与下一轮工具暴露之间的联动还不够强
- 工具 registry、router、orchestrator、runtime policy 的边界还不够清晰

## 设计原则

后续所有工具系统改动，都应遵守以下原则。

### 1. 默认主链必须短

普通代码定位任务，模型应该优先走短链路：

1. 找文件
2. 搜内容
3. 读文件
4. 编辑或验证

如果一个常见任务必须先决定很多“要不要用 shell / 要不要 discover tool / 要不要换搜索模式”，那就是主链太重了。

### 2. 搜索工具必须让下一步显而易见

搜索不是为了“显示很多文本”，而是为了让模型立刻知道：

- 下一步读哪个文件
- 是否要并行读 2 到 3 个文件
- 是否该继续缩小范围还是已经足够编辑

### 3. 结构化结果才是事实源

展示文本只是投影，事实必须在结构化结果里稳定存在。

这适用于：

- 搜索命中
- 读文件结果
- 命令执行状态
- 工具发现结果

### 4. 常见路径要原语化

高频动作应该用强原语直接表达，而不是让模型通过通用 shell 再拼出来。

典型高频动作包括：

- 文件名匹配
- 内容搜索
- 单文件精读
- 精确编辑

### 5. 低频能力用延迟发现，不挤占默认面

默认可见工具集要小。

但小不代表弱，而是：

- 高频主链工具默认可见
- 低频或上下文相关工具 deferred
- 需要时通过 `tool_search` 拉出来

### 6. 工具系统要像系统，不像工具堆

一个成熟工具系统至少要有清晰分层：

- tool catalog
- exposure pipeline
- discovery
- routing
- execution
- result projection

如果这些责任混在一起，后续会越来越难维护。

## 目标架构

推荐把 `cloudagent` 工具系统收敛为两条链：

### A. 代码定位主链

目标是更接近 Claude Code：

- `find_files`
- `search_text`
- `read_file`
- `edit_file`
- `exec_command`

其中：

- `find_files` 负责文件名/路径模式查找
- `search_text` 负责内容搜索与命中排序
- `read_file` 负责单文件精读
- `edit_file` 负责结构化编辑
- `exec_command` 负责 build/test/git/运行态验证

当前的 `search_workspace` 可以继续存在一段时间，但长期不建议继续让一个工具同时承担：

- 文件查找
- 文本搜索
- 搜索 session 管理
- 结果引导

这会让工具语义过重。

### B. 工具发现与调度主链

目标是更接近 Codex：

- registered tools
- environment-visible tools
- permission-allowed tools
- default visible tools
- deferred discoverable tools
- discovered-and-exposed tools

并形成配套子系统：

- `tool_search`
- tool registry
- exposure resolver
- execution strategy
- approval policy
- result projection

## 重点差距

### 差距 1：仓库搜索原语不够分明

当前 `search_workspace` 已经比早期版本强很多，但仍然不如 Claude Code 的 `Glob + Grep + Read` 这条链低摩擦。

影响：

- 模型仍然容易在一个大工具里来回切模式
- 初期定位调用路径偏长
- 很难把“文件查找”和“文本搜索”分别优化到极致

结论：

- 这是当前最影响“少调用几次就定位问题”的一层

### 差距 2：`tool_search` 仍然太轻

当前 `tool_search` 更像轻量字符串匹配器。

Codex 风格的 `tool_search` 更接近成熟检索层：

- 为工具构造统一 `search_text`
- 将 name、description、namespace、schema 属性名等一并索引
- 做结果 coalesce
- 做 bucket limit
- 做 discover 后的下一轮暴露联动

影响：

- discoverable tools 多起来后，当前搜索质量会下降
- deferred tools 很难稳定被模型找到

### 差距 3：运行与调度层还不够系统化

当前批量执行、审批、并行分流已经存在，但离 Codex 级成熟度还有差距：

- registry / router / orchestrator 边界还不够清晰
- discover 与 runtime 的联动还不够深
- 工具结果对下一轮暴露的影响还不够稳定
- 并行策略还偏静态，不够 runtime-aware

影响：

- 能跑，但不够顺
- 工具多了以后，系统复杂度会上升得很快

## 路线选择

后续路线不应该是“先把所有东西都做得更像 Codex”，而应该分优先级。

### 第一优先级：降低代码定位的调用次数

优先把搜索主链做强。

目标：

- 更少工具调用
- 更少试探性搜索
- 更快拿到候选文件

推荐动作：

1. 将 `search_workspace` 拆分为 `find_files` 与 `search_text`
2. 保留 `read_file` 的单文件精读语义
3. 让 `exec_command` 彻底退出 repo 搜索主路径

### 第二优先级：升级 `tool_search`

目标：

- deferred tools 可发现
- discover 结果更稳定
- tool search 更像真正检索层

推荐动作：

1. 为 discoverable tools 构建统一检索文本
2. 将参数 schema、usage guidance、source/namespace 一并纳入索引
3. 用 BM25 或等价检索替换当前 `contains` 打分
4. 做结果去重与分桶限制

### 第三优先级：整理工具调度系统

目标：

- 让工具系统在复杂场景下仍然稳定

推荐动作：

1. 明确 registry / exposure / router / execution / projection 的边界
2. 强化 discover 与下一轮 exposed tools 的联动
3. 让并行能力从“可并行”升级为“按策略并行”

## 明确不做的事

以下方向不建议继续投入：

- 继续把 repo 搜索主链绑在通用 shell 上
- 继续让单个重工具承担过多职责
- 继续通过 prompt 强行弥补工具原语设计问题
- 单纯复制 Codex 的 tool 表面 schema，而不复制其系统边界

## 当前推荐落地顺序

1. 拆分 `search_workspace`
2. 强化 `search_text` 排序与截断
3. 重写 `tool_search` 检索层
4. 收紧默认工具暴露面
5. 整理调度层边界
6. 最后再配套更新系统提示词

这个顺序的原因很简单：

- 工具主链先变强，模型自然就少走弯路
- 发现与调度层变强，复杂工具系统才不会失控
- 提示词应该放在工具系统之后收尾，而不是先顶上去补洞

## 文档使用方式

这份文档的用途是：

- 评估新增工具是否应该进入系统
- 判断一个工具改造是应该学 Codex 还是学 Claude Code
- 判断一个问题是搜索主链问题，还是发现/调度问题
- 作为后续工具系统重构时的长期设计基准

如果后续实现细节发生变化，只要下面三条没有变，这份文档就仍然有效：

- 默认主链要短
- 搜索结果要让下一步显而易见
- 工具系统要是分层清楚的系统，而不是工具堆
