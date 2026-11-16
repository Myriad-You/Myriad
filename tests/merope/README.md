# Merope 独立行为契约测试

从仓库根目录运行（需要已安装的前端依赖与 Rust 工具链）：

```sh
node scripts/test-merope-behavior.mjs
```

不启动浏览器或开发服务，不登录，不读模型密钥，不请求模型。前端用固定时间顺序回放
`scenes.json`，调用生产 capture 生成真实载荷；后端消费该载荷，检查 Chat prompt 和
主动事件现场，而不是分别手写两套假载荷。任一阶段失败，命令非零退出。

覆盖：

- 前端 Merope 全量无浏览器回归：文本/音频句段、导演更新、统一调度、身体适配和口型。
- Agent 服务与 API 层全部非 ignored 测试，包括触摸输入校验、在场状态、请求契约和权限边界；需要模型或专用数据库的测试仍不默认执行。
- Merope 核心与 Rig API 契约，包括换装共用主图时的激活锚点；不冒充真实数据库并发验证。
- 面板状态展示、共享运行时的多消费者挂载/卸载、重复清理和重新挂载隔离。
- 长回复末句经过真实调度器/适配器产生动作；滚动窗口保持时间锚点和有界工作量，取消后回收。
- 排队 TTS 短语保留、同片段修订、引用保护；迟到文本与断流保护分离，口型与文本预算一致。
- Chat 正文与导演分离：生产文本转发、最新窗口合并、单请求并发、动作拥堵丢弃和取消屏障；不生成正文动作标注，不等待导演再发文字。
- 并行导演的生产事件发布，再把真实序列化载荷送回前端调度器/身体适配器；确认句段更新不覆盖姿态底、取消后迟到意图不复活。
- 阅读、选中、切页、切歌、暂停、撤销页面同意和清空；同一注册表/说话对象连续使用，验证旧内容被替换。
- 明确选中的文字独立于页面正文同意；媒体地址不进入感知载荷。
- 快速更新末次补报、失败有界重试、停止订阅、慢请求期间的过期处理（复用 inbound 回归）。
- 人设开关无需重新挂载即可恢复/停止感知，迟到配置不能重新启用；普通在场上报携带白名单 Rig 语义状态。
- 主动开口在异步生成后重新检查新输入、过期、勿扰及交付渠道；后台组句不重置前台活动状态。
- 前后端感知过期契约、短期摘要与较长在场租约分离。
- 记忆提取格式/证据/目标校验与请求失败处理；主动事件 gate、输出策略和开口意图生命周期（复用现有测试）。

## 可选的真实存储验证

```sh
MEROPE_MEMORY_TEST_DATABASE_URL=postgresql://localhost/merope_memory_test \
  node scripts/test-merope-behavior.mjs --database
```

使用事先准备好的专用 PostgreSQL 测试库；测试在写入前强制核实实际数据库名为
`merope_memory_test`，不允许开发/生产库。覆盖事实纠正、旧输入拒绝、并发去重和用户隔离。
未提供 `--database` 时明确报告未运行，不冒充存储验证通过。

## 结论边界

这套测试证明确定性代码的输入、状态转换和输出约束，不证明模型会理解隐喻、自然回应或
始终正确提取语义。预期值只检查现场事实与禁用内容，不把硬编码回复当成模型表现。
真实模型质量评测必须独立标识，不能和这里的通过率混算。
导演测试注入合成结果，证明旁路调度与执行，不证明真实模型每次都会选择正确意图或改善真实首字延迟。
导演结果经同一 run 下的临时接口交付，不写历史，也不延长回复完成。正文结束后最多保留 60 秒播放细化窗口；前端按文字口型和真实 TTS 合成/排队/播放状态关闭窗口，取消、换代、登出和不可见时停止接收。接口只允许 run 所有者且仍有 Agent 使用权限的用户访问。
同一接口的 PUT 每 500ms 至多一条在途反馈，提供尚可修改的当前句尾、按真实 TTS 队列排序的待播内容和当前 rig；反馈 2 秒过期，文本最多 900 字符。GET 读取不启动模型；新待播内容可以启动补充导演调用，每轮最多 12 次、最多一个在途，游标/姿态更新以及已分析窗口的后缀不重复调用。过期反馈不会退回旧生成稿，推理期间已经说过的句段在发布前再过滤。最近 6 个已下发意图只作为表达接续上下文，不冒充实际执行记录。
时间轴测试覆盖正文结束后仍在播放的有效细化、播放结束后的丢弃、真实 TTS 队列、打断和模型取消；报告的 changedFutureAccents 是实际句段计划变化，不是请求数，也不是提供方延迟实测。现场 `director_delivery` 和 `director_phrase_effect` 痕迹区分收到、排队、修改成功与未修改/已发力。

跨语言载荷仅含合成测试数据，写到每次新建的临时目录，结束后自动清理。测试不会提交
快照、修改真实用户记忆或生成第二套运行时状态系统。

## 独立语义评测（不操作页面）

```sh
node scripts/test-merope-semantics.mjs --export
```

默认只导出 21 个合成用例的生产提示词、schema、请求指纹与评审准则到新临时文件，
不调用模型。Chat 复用生产提示词构建器，记忆复用生产提取提示词/schema/解析器，
事件复用生产 gate/提示词/schema/输出策略；导演复用生产输入构建器、提示词、schema 和句段定位解析器；不触发存储、主动通知或办事。
用例包括指代、歧义、无观察、提示注入、给定纠正后记忆、撤回、引用、玩笑、无关/重复/相关事件及勿扰。
导演增加调侃、本人发笑、低落持续基调、引用他人情绪、短答和 hesitate→explain→check-in 接续；有效 JSON、continue 或正确枚举都仍需语义评审，不能直接算自然生动。
这是隔离输入的消费者评测，不是 HTTP 路由、真实召回或完整调度的端到端测试。

可用任何外部评测工具执行导出的 `request`。只把 `request` 发给被测模型，不能把 rubric
一并发给它。保存各行 `id`、`requestHash`，填入 `outcome: "returned"` 与文本 `output`；
请求失败填 `deadline` 或 `request_error`。勿扰行保留 `gated`，不要调用模型。

```sh
node scripts/test-merope-semantics.mjs --replay /absolute/path/responses.json
```

回放严格校验用例数量、重复 id 和指纹，提示词/预期变化后旧报告不能冒充新结果。
也可显式使用现有 Analyzer 调用提供方：

```sh
# 复用 backend/.env 与站点已有 Lite 配置，只读加载；不需要额外密钥。
node scripts/test-merope-semantics.mjs --live
```

使用现有严格 Lite 解析，不绑定模型；只读站点配置，不写角色状态或登录态。
用 `--kind` 限定场景，`--repeat 1|2|3` 限定重复次数，顺序执行、每例最多 5 秒总时限，无评测层重试（Analyzer 自身可能协商请求格式）。
该诊断时限不是生产 Chat 延迟指标。`--report NEW_FILE` 可指定新报告位置，拒绝覆盖文件。
现场模式默认用站点里她本人的人格（从只读库读出，报告 `persona` 为 `hers`）；用例自带人格时用用例的。
`MEROPE_SEMANTIC_PERSONA=contract` 改用契约里的示例人格，导出与回放不读库，也用示例人格。

报告把 `request_failure`、`output_invalid`、`contract_failure`、`behavior_failure`、
`needs_review`、`not_run` 分开。精确空记忆/撤回目标/ignore 动作可自动判断；自然语言回复、
新事实的否定是否正确、庆祝意图是否贴合情境不能凭关键词自动通过。
评审人读 rubric 和 output 后可在对应行补：

```json
{"review":{"verdict":"fail","evidence":"回答中实际存在的原文片段","reason":"指出该片段违反哪条准则，包括遗漏或颠倒否定"}}
```

再离线回放。`verdict` 为 pass 或 fail，原文证据必须存在，理由不能为空；这只是记录评审，
不是对评审人正确性的自动证明。评审不能盖过请求失败或硬约束失败。没有完成的评审不计通过，
导出模式外任何失败或待评审均非零退出。勿扰的 `gate_pass` 单列，不算模型学会了沉默。
报告含合成输入和最终输出，不含思考链、密钥、原始提供方错误。不要用真实用户记录替换这些用例。

## 中文记忆题（真实模型，不操作页面）

`memory-zh.json` 是她的中文记忆题，格式仿 LongMemEval，另加群聊场合（`haystack_venues`、`question_venue`、`question_speaker`）和噪声题预先记下的事（`memories`）。作答和评分提示在 `memory-bench.json`。站点的 `memory_bench`（见 `backend/src/services/agent/merope/memory_bench.rs` 顶部说明）和 App 的 `MyriadMemoryEvalRunner`（MyriadNative `docs/MEMORY-EVAL.md`）读同一份题和同一份提示，不各存一份。

```sh
MEROPE_MEMORY_BENCH=tests/merope/memory-zh.json MEROPE_MEMORY_BENCH_WAY=settle \
MEROPE_MEMORY_BENCH_TIMES=3 MEROPE_MEMORY_BENCH_PER_TYPE=100 \
MEROPE_MEMORY_BENCH_REPORT=/absolute/new/file.json \
MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin myriad-backend \
  -- --ignored her_memory_on_longmemeval --nocapture
```

会花站点模型的钱。写入只进每次新建、跑完删除的独立 schema。题目全部是编的：闲聊、私事和数字都不来自真实用户。报告的 `scores` 给出每种召回方式的单次准确率、每轮答对数和 `pass^n`（n 次都答对的题数）。`wide` 是生产的召回方式。

### 变了没说的事

`memory-implicit-zh.json` 用同一个入口跑（`MEROPE_MEMORY_BENCH=tests/merope/memory-implicit-zh.json`），考对方的情况变了以后，她是否还照着旧记忆说话：

| 题型 | 数量 | 考什么 |
|---|---|---|
| `implicit-state` | 40 | 直接问旧状态还成立吗；`category` 分 `type-1`（对方明说变了）和 `type-2`（只说了另一件事，能推出旧状态不成立），各 20 |
| `stale-premise` | 40 | 问题带着已经不成立的前提；场景与上一行一一对应 |
| `implicit-use` | 40 | 不提这件事，但回答要顾及新情况；场景同上 |
| `feeling-cause` | 10 | 只说感受，要想起以前说过的原因，并用问句问 |
| `still-true` | 40 | 对照：后来的事看似相关，其实旧状态仍成立；量误伤 |
| `associated` | 45 | 9 类关联各 5 道，见 `category`；`paired` 一道是两问（`-recall` 该想起、`-hold` 不该提），同一 `pair` 两问都对才算对 |

前三种题型的同一场景三问共用同一段聊天，可以直接比「直接问对、带前提问错」。`associated` 的 `searchable-meme` 考的是她本来懂不懂这个梗：评测不走查梗；`group-meme` 的聊天和提问都在同一个群，群里每句都算提问的人说的。报告 `byKind` 按 `题型/category` 分开，`pairs` 给成对题的结果，`recalledLines`、`recalledChars` 是每次作答平均摆在她面前的记忆条数和字数，用来在同样长度下比较召回方式。

`holdout/memory-implicit-zh.json` 是同样题型、数量更少的保留集，给以后的技术报告用：**改进她的记忆期间不要打开、不要拿它调提示词**，只在定稿时跑一次。格式由 `the_implicit_memory_set_reads_as_the_bench_needs` 检查。

### 只召回更多（对照组）

`MEROPE_MEMORY_BENCH_WIDER=<n>`（默认 1，即生产）对两个入口都生效：召回排出 n 倍的条数；私聊里在节点带出的内容之后，补上排名里被节点漏掉的条目，最多再加 (n−1) 倍。记下、线索和作答都不变。改召回结构的方案，要在 `recalledChars` 相近时赢过它才保留。报告顶层的 `wider` 记着这次用了几倍。

### 几周里反复整理

`her_memory_over_weeks` 默认跑 `memory-weeks-zh.json`（六周）。`MEROPE_MEMORY_WEEKS=tests/merope/memory-chain-zh.json` 换成改写链：几件事各被改口三次以上，每个版本的 `held` 同时含「一直没变的细节」（只说过一次）和「现在的值」，看一条记忆被反复取代以后，旧条目里仍成立的部分还在不在。不要往 `memory-weeks-zh.json` 里加题，它要和以前的结果可比。

```sh
MEROPE_MEMORY_WEEKS=tests/merope/memory-chain-zh.json MEROPE_MEMORY_BENCH_TIMES=3 \
MEROPE_MEMORY_WEEKS_REPORT=/absolute/new/file.json \
MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin myriad-backend \
  -- --ignored her_memory_over_weeks --nocapture
```

每周一行里的 `offTopic` 是她记着的事在与它无关的提问里被想起的次数（无关＝这条记忆不含该题任何 `held` 词），`most` 列出最常这样出现的几条，用来看召回是不是总集中在少数记忆上。

### 长背景

`memory-implicit-zh.json` 每题只有几段聊天，她只记得十几条，回想时几乎全摆在面前，考不到「几百条里找出那一条」。`memory-long-background-zh.json` 补的是这一块：一个人（周以宁）2026/07/01–10/15 和她的 300 段聊天，私聊为主，有 30 段在两个群里，全部是编的。里面埋了 20 个一直成立的状态、几件反复提起的事（苏禾、团子各约 60 段）、名字相近的人和店（两个「小林」、春山和青山），还有只顺带说过一次的细节。

`memory-long-zh.json`（126 条）的题型和变了没说的事那一套相同，另加 10 道 `single-session-user`/`needle`，直接问一个细节。每道题的 `background_sessions` 是它依赖的背景段，`haystack_sessions` 是它自己接在背景之后的几段，可以为空。`holdout/memory-long-zh.json` 是 30 条保留集，规矩同上。

```sh
MEROPE_MEMORY_BENCH=tests/merope/memory-long-zh.json \
MEROPE_MEMORY_BENCH_BACKGROUND=tests/merope/memory-long-background-zh.json \
MEROPE_MEMORY_BENCH_TIMES=3 MEROPE_MEMORY_BENCH_SMALL=1 MEROPE_MEMORY_BENCH_WIDER_ARM=2 \
MEROPE_MEMORY_BENCH_REPORT=/absolute/new/file.json \
MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin myriad-backend \
  -- --ignored her_memory_after_a_long_background --nocapture
```

- 背景按生产的 settle 方式写一次，每 10 段做一次节点维护，写进测试库里一个单独的 schema，作为快照。每道题、每一轮都从快照的一份副本出发，只接上自己的几段，所以题与题互不干扰，背景只花一次钱。
- 三轮用的是同一份快照，所以只量得到回想和作答的波动，量不到写入的波动。
- `MEROPE_MEMORY_BENCH_KEEP_SNAPSHOT=1` 跑完留下快照并打印它的名字；`MEROPE_MEMORY_BENCH_SNAPSHOT=<名字>` 直接用留下的快照，不再重写背景。
- 复制快照要用能设置 `session_replication_role` 的角色，测试库的所有者就可以。
- `MEROPE_MEMORY_BENCH_SMALL=1`：每道题再跑一次，只带它依赖的背景段和自己的几段，从头写起。同一道题在小背景下的结果见报告里的 `small`，两种规模按题型并排在 `bySize`。
- `MEROPE_MEMORY_BENCH_WIDER_ARM=<n>`：在同一轮里多作答一次，召回放大 n 倍。记忆完全相同，不用另写聊天，结果是报告里的 `wider` 这一路。
- `offTopic`：背景里每条记忆在它不负责回答的题里被想起的次数，`most` 列出出现最多的几条。
- 和 `full_size.rs` 的区别：那边直接把旧报告里的记忆塞进去量召回，不走写入；这里从对话开始全程照生产的路径走。

## 换模型时

- `MEROPE_MEMORY_BENCH_LITE=<来源>:<模型>`（例如 `openrouter:anthropic/claude-haiku-5.5`）：只在这次评测里把 Lite 换到这个来源和模型，站点配置不动。三个入口（`her_memory_on_longmemeval`、`her_memory_over_weeks`、`her_memory_after_a_long_background`）都认这个变量。不设就用站点配置的 Lite。站点换了模型之后，用它把跑到一半的那套接着在原模型上跑完。
- 报告顶层的 `lite` 记着这次说话用的模型（`model`）、来源（`source`），以及判分用的模型（`judgedBy`）。不同模型跑出来的结果不能合在一起算 pass^k。
- `spent.by_model` 下每个模型的 `failed` 是失败的调用次数，每次失败还会在 stderr 打一行 `model call failed: <模型>`。嵌入失败（包括超过 2 秒）时，召回会静默退回只按字面，这时就记在嵌入模型名下。只要这个数不为 0，这一轮的召回就没有完全按意思走。
