# 共用核心（myriad-core-ffi）

站点以外的平台（目前是 macOS 原生 App，仓库 MyriadNative）要和站点上的她说同样的话、按同样的规则变心情，所以不各自抄一份规则，而是链接同一份 Rust 代码。`crates/myriad-core-ffi` 把纯规则 crate 包成一个静态库，对外只有一个 JSON 进、JSON 出的 C 入口。

站点后端不经过它，照常直接依赖 `myriad-merope`、`myriad-agent-rules`。

## 边界

- **只收纯 crate。** 现在是 `myriad-merope` 和它依赖的 `myriad-agent-rules`，以及同步她的数据用的 `myriad-persona-sync`（格式、规范字节、时钟、比对）。不引入 tokio、reqwest、sea-orm，也不读写文件或网络。
- **每个调用都是输入的纯函数。** 现在几点、她的时区、心情这些状态都在输入里；库不读进程的时钟、时区和环境。crate 里的规则因此不用 `chrono::Local`：她的时区由调用方传入，后端传 `merope::clock::zone()`，现在就是进程时区（容器里 `TZ=Asia/Shanghai`）。
- **一个二进制里只能有一个 Rust 静态库。** 两个 Rust staticlib 会在 std 符号上冲突。别的平台将来要用的纯 crate，都并进这一个库，不另建。
- **密钥不进来。** 规则拿不到任何宿主密钥或凭据，错误信息里也没有。

## ABI

头文件：`crates/myriad-core-ffi/include/myriad_core.h`。

```c
MyriadCoreBuf myriad_core_call(const char *name, const uint8_t *input, size_t len);
void myriad_core_buf_free(MyriadCoreBuf buf);
```

| `status` | 含义 | 缓冲区内容 |
| --- | --- | --- |
| 0 | 成功 | 调用的输出 JSON |
| 1 | 没有这个调用 | `{"error":{"kind":"unknown_call","message":…}}` |
| 2 | 输入不对（不是 JSON、缺字段、多字段、类型不对、名字为空或不是 UTF-8） | `{"error":{"kind":"bad_input",…}}` |
| 3 | 调用里 panic（库内捕获，不跨 FFI 展开） | `{"error":{"kind":"panic",…}}` |

- 返回的缓冲区归调用方，用完交给 `myriad_core_buf_free` 一次。`ptr` 为空的缓冲区可以安全地交回。
- `input` 为空指针且 `len` 为 0 时按 `{}` 读。
- 字段名 camelCase；输入拒绝未知字段，拼错不会被静默忽略。
- 时间是 Unix 微秒（`i64`）。她的时区是 IANA 名（`Asia/Shanghai`）或固定偏移（`+08:00`）。用名字，是因为夏令时会挪动一天的边界，而且各平台要拿同一个名字。
- 浮点按位一致：`serde_json` 开了 `float_roundtrip`，平台侧用最短往返表示编码即可。

`meta.version` 返回 `abi`（`ABI_VERSION`）、`upstreamCommit`（构建时的 `MYRIAD_CORE_UPSTREAM_COMMIT`，没设为 `unknown`）和全部调用名。调用表在 `src/calls.rs` 的 `CALLS`。

## 加一个调用

0. 规则还在后端（`backend/`）的，先搬进纯 crate（只挪不改，后端改为 re-export 或薄包装，现有测试证明行为不变），再登记调用。回想的算术（`myriad_agent_rules::memory`）、聊天日记的门槛（`myriad_merope::diary`）、她的作息（`timing::*_at`）、聊天记录的文本（`chat_history`）就是这样进来的。
1. 在 `src/calls/` 里它所属领域的模块（心情、说话、她自己的生活、作息、体征、说话习惯、海龟汤、连载、先开口、换装……）写调用函数，再按名字的字典序登记进 `src/calls.rs` 的 `CALLS`。函数只做 JSON 与 crate 类型之间的转换，规则本身留在原 crate。
2. 在 `src/tests.rs` 或 `src/tests/` 的领域模块里写一组往返测试：经 C 入口的输出要和直接调 crate 函数逐字节（浮点逐位）相同。再把名字登记进该模块的 `TESTED`（或 `every_call_is_tested_and_version_lists_them` 的列表），漏登记会失败。
   - 规则本身的输出要确定：同一输入在任何平台、任何一次运行都得出同一结果。按 `HashMap` 的遍历顺序决定先后的地方要补一个确定的次序（例如 `contrast::overused` 的同分按片段本身排）。
3. 改了已有调用的输入或输出形状、或删掉调用，要加 `ABI_VERSION`。只是新增调用或可选字段则不必。
4. `cargo test -p myriad-core-ffi`，`cargo clippy -p myriad-core-ffi --all-targets -- -D warnings`。

## 同步（`sync.*`）

她的数据跨端时的格式只有 `myriad-persona-sync` 里的 Rust 类型这一份定义。平台把自己的记录拼成粗实体交给 `sync.canonicalize`，拿回实体和它的规范字节；存的、发的、比对的都是这份字节。`crates/myriad-persona-sync/fixtures/` 是黄金文件：每类实体一份、一份完整的信封、一组比对（`compare/local.json`、`remote.json` → `comparison.json`）、一个 rig 包（atlas 字节是 `atlas-pixels`）。别的平台按钉住的提交复制这些文件，经 C 入口逐字节比对。故意改格式时用 `MYRIAD_SYNC_BLESS=1 cargo test -p myriad-persona-sync` 重写它们。

| 调用 | 输入 → 输出 |
| --- | --- |
| `sync.affect_fold` | `{anchor, events:[变化], upto}` → `{anchor}`：把 `upto`（含）以前的变化折进锚点，锚点停在最后一条之后并记下它（`throughEvent`）；只有站点写锚点 |
| `sync.affect_replay` | `{anchor, events:[变化], now, baselineNow}` → `{affect}`：她对站长现在的心情 |
| `sync.archive_key` | `{passphrase, kdf}` → `{key}`：按文件头里的 argon2id 代价从口令（NFC）派生密钥 |
| `sync.archive_open` | `{start}`（文件开头，base64）→ `{header, offset}`；或 `{key, header, index, last, data}`（一块密文）→ `{data}`：除最后一块外每块恰好 `chunkBytes` + 16 字节 |
| `sync.archive_part` | `{part}` → `{head}`；或 `{bytes}` → `{part, headBytes}`（不够一个头时 `{part: null}`） |
| `sync.archive_seal` | `{salt, noncePrefix, fields}`（16、19 字节随机数，base64）→ `{header, line}`；或 `{key, header, index, last, data}`（一块明文）→ `{sealed}` |
| `sync.canonicalize` | `{entities:[粗实体]}` → `{entities:[{entity, canonical}]}`；`digest` 可留空，写了就必须对 |
| `sync.check_rev` | `{rev, now}` → `{accepted, aheadMs, maxAheadMs}`：`now` 是站点的时间，`rev` 超前它 5 分钟以上就不收 |
| `sync.compare` | `{local:[实体], remote:[实体]}` → 比对结果（各字段、各类的相同／只在一端／两端不同，`sameName`、`needsSubject`，`unreadablePictures` 两端各自读不到的图） |
| `sync.is_empty` | `{persona, memories, diary, affectEvents}` → `{empty}` |
| `sync.hlc_next` | `{last?, now, node}` → `{rev}` |
| `sync.hlc_observe` | `{last?, seen, now, node}` → `{last}` |
| `sync.legacy_rev` | `{updatedAt, origin?}` → `{rev}`（没有修订的旧行现算） |
| `sync.manifest` | `{envelopes}` → `{manifest}`：每类实体、向量、文件的条数和 digest，与读的顺序无关 |
| `sync.persona_diff` | `{before, after, revs, legacyRev, rev, origin, reason?}` → `{changed, revs, history}`：人设一次写入逐字段比（字段没了算 `null`），变了的字段记成这次的 `{rev, origin}`，被换掉的旧值连同它原来的 rev 进 `history`（原来就没值的不进）；没有 rev 的旧字段按 `legacyRev`、`site` 算；`reason` 默认 `edit` |
| `sync.rig_package_id` | `{atlas: base64, manifest}` → `{id}` |

格式里几条平台要照做的规矩：

- **第 2 版的几类只在搬家和导出时出现。** `conversation`（站长的一场私聊连同每一句，含「没听完」标记和句子里的站点图片）、`proactive_line`、`intention`、`owner_state`、`pace`、`present`、`wearing`、`field_history` 是第 2 版加的。首次比对的 pull 只发前 13 类（`Kind::COMPARED`），`sync.compare` 也只读它们。
- **导出文件 `myriad.her-archive`。** 第一行是明文头（规范 JSON），之后是分块的 XChaCha20-Poly1305：nonce 为头里的 19 字节前缀 + 块序号（u32 大端）+ 最后一块标记，头这一行是每块的附加数据。截断、换块、改头都打不开，口令错和文件坏说的是同一句话。流里依次是信封页、它们引用的文件（rig 是 `manifest.json` 加 `atlas.png`）、清单。导出文件总是加密。盐和 nonce 前缀由调用方从系统随机源取来传入，核心不读随机数。黄金文件在 `fixtures/archive/`。

- **不透明的人设键。** 人设里本端不认识的键（站点的 `displayName`、`draftSource`，以及以后别的端加的键）是 `opaque` 类：原样存下，原样传回（值和 `rev` 都不动），按单值合并。比对不列它们：不用这个键的一端谈不上和别人不同。
- **读不到的图。** 立绘、贴纸、贴纸头像的位置上要么是图片资产，要么是 `{"unreadable": true}`：这一端有这张图，但读不出来（外链失效、文件没了）。没有可拉的文件，界面照比对里的 `unreadablePictures` 提示「站点上有张图读不到」。站点上外链的图（`http`/`https`）在第一次被设备读到时，经出站守卫下载、存进站点的媒体库，她的人设、衣柜立绘、贴纸头像和贴纸改指向这份副本；之后外链失效也不影响。下载失败的地址 10 分钟后再试，这期间带标记。rig 以某个地址为母图时，那个地址保留不改，图从副本读。图按字节的 sha256 跨端，换地址不改变跨端的任何内容。
- **人设的字段修订与历史。** 每个人设字段各有自己的 `{rev, origin}`：站点存在 `agent_persona.field_revs`，每次经站点编辑人设都用 `sync.persona_diff` 比一遍，变了的字段拿到这次的 rev，被换掉的值进 `agent_persona_field_history`（每个字段最多留 50 条），站长可以在「人格」各项、名字、时区与外观设定（`visual.*`）旁边的「历史」里看并换回，换回也只是一次新的编辑。pull 的 persona 里每个字段的 `rev` 是它自己的（没编辑过的按行的 `updated_at` 现算），被清空的字段以 `null` 带着它的 rev 下行，整个 persona 的 `rev` 是其中最新的。贴纸头像按图片字节算，换地址不算编辑，仍用行的 rev、不进历史。
- **她的时区。** 人设字段 `zone` 是 IANA 名（`Asia/Shanghai`），或空（`null`）：没设时各端用自己的时区（站点是进程的 `TZ`）。规范化时去掉首尾空白，空串记为 `null`。站点存在 `agent_persona.zone`，核心调用里的 `zone` 参数就该传它（没设时传本机时区）。
- **心情按变化重放。** 每次心情变化记一条 `affect_event`：`at`（这次写入盖的时刻）、`baseline`（这次之前她往哪里回落）、`impulse`（`{"appraisal":{emotion, arousal, scale}}`、`{"music":{listenedSeconds}}` 或 `"none"`：只是又过了一会儿）。重放：从锚点出发，按 `(at, id)` 逐条先按整秒数回落到这一条的 `at`，再施加冲量，四个值夹到 0–100；最后回落到 `now`。锚点及以前的变化已经在锚点里，不再施加。变化可以只给 `{id, at, baseline, impulse}`，事件 body 加上 id 也行（别的字段不读）。站点记下的变化与站点自己读出的心情逐位相同。站点每天折一次：站长的变化留 14 天、别人的 30 天，更早的折进锚点后删掉；这是站点的维护，她住在设备上（主机是设备）时也照常进行。收到比锚点还早的变化（在它的时刻被折进锚点之后才到）时，收的一端把 `at` 改成锚点的时刻并标 `late`，再保存。
- **取代边。** 记忆被别的记忆取代（`superseded`，以及 `changed_mind`、`forgiven` 等换成新行的退役）时，旧行的 `supersededBy` 写新行的 id，与退役同一次写入；多条旧行可以指向同一条新行，新内容已经有别的行在说时指向那一行，只是收回、没有新行时为空。`faded` 等没有新行的退役不写边。
- **没有停用时间的旧记忆（R9）。** 一端有些记忆只记了「已停用」、没记什么时候：上线时按退役处理，`invalidAt` 取这条记录最后一次写入的时间，`invalidReason` 可空。格式照常接受（`retired` 当且仅当有 `invalidAt`，不要求晚于 `validFrom` 或 `createdAt`）；这条换算由那一端在拼粗实体时做，核心不猜。
- **在穿哪一套各端各自。** `visual.activeOutfitId` 与 `visual.activeFullBodyOutfitId` 不是人设字段（Q12、R6）：站点不发，别的端也不要发；衣柜本身照常同步。
- **资产按内容 id 比对。** `sync.compare` 只看 rig、立绘、贴纸、贴纸头像的内容 id（rig 的包 id、图片字节的 sha256）；两端写的 `bytes`、`mediaType` 不同不算不同（R8）。
- **体征不带群。** `vitals_day` 的 `groups` 只有每个群的计数，规范化时去掉群的标识并按计数排序。
- **时钟超前。** 站点不收超前它 5 分钟以上的 `rev`，回 409 `clock_ahead`（带 `serverTime`、`aheadMs`、`maxAheadMs`）。设备的请求头 `X-Sync-Clock` 带「下一次写会拿到的 rev」（`sync.hlc_next` 算出、不落盘），`hello` 与 `role` 必带；以后的 push 里每个实体和每个人设字段的 `rev` 都按同一规则查，有一个超前就整批不收，什么都不写、也不吸收进站点的时钟。设备收到 `clock_ahead`：停止同步并告诉用户时钟不对，outbox 留着（暂存）；时钟改对后，把时钟退回到最近一次从站点看到的 rev，按 outbox 顺序用 `sync.hlc_next` 给每条待推的写重新打 rev，再推。推之前可以用 `sync.check_rev` 按 `hello` 给的 `serverTime` 加上经过的时间自查。
- **站点自己的时钟。** 站点量自己的时钟偏了多少（启动时和之后每 15 分钟；SNTP，UDP 123 不通时用 HTTPS 的 `Date`），`hello`、`clock_ahead` 和站长的设备列表都带 `serverClock`：`{checkedAt, offsetMs, source, trusted, thresholdMs, wrong, corrected}`，检查关掉或还没跑过时是 `null`。`offsetMs` 是站点的时钟减去真实时间（毫秒，负数是站点慢），什么都没答上时为 `null`（无法检查，不算偏差）；`checkedAt` 是站点自己的时钟、自 epoch 的毫秒；`source` 是 `sntp`、`https` 或 `null`；`trusted` 是 HTTPS（证书验过）或至少两个 SNTP 服务器相差不到 1 秒；`wrong` 是偏差超过 `thresholdMs`；`corrected` 是站点现在拿这次测量把设备的时钟和真实时间比（可信、不超过 45 分钟、偏差不超过 3 小时）。`corrected` 时，上面的超前检查、`hello` 和 `clock_ahead` 的 `serverTime` 都是真实时间（本机时间减去 `offsetMs`），否则照旧是本机时间；`serverTimeSource` 说是哪一种：`clock`（本机）、`sntp` 或 `https`。站点别处都仍用本机时间，上限 5 分钟不变，真超前的设备照样被拒。设备据此分清是哪一端不准：`wrong` 而且没有 `corrected` 时是站点的时钟不对，设备不要去改自己的。

## 别的平台怎么链接

不从工作区构建：工作区里常有别人还没提交的改动。按一个提交构建：

1. 取这个提交的 `crates/myriad-core-ffi` 与它的全部 path 依赖、`shared/`（`myriad-merope/build.rs` 在编译期读里面的契约）、`Cargo.lock`、`rust-toolchain.toml`，`git archive` 到一个隔离目录。
2. 在那里写一个只列这些 crate 的工作区 `Cargo.toml`，沿用根 `Cargo.toml` 的 `[profile.release]`。Cargo 只会从锁文件里去掉没用到的包，用到的每个包都要与上游 `Cargo.lock` 同版本。
3. 设 `MYRIAD_CORE_UPSTREAM_COMMIT=<提交>`（取归档自己记录的提交号，`git get-tar-commit-id`），`cargo build --release --target <triple> -p myriad-core-ffi`。
4. 链接 `libmyriad_core_ffi.a` 和头文件；启动或测试时断言 `meta.version` 的 `upstreamCommit` 就是钉住的提交。

macOS 原生的做法：MyriadNative 的 `CORE_PIN`、`scripts/build-core.sh` 与 `docs/CORE.md`（只有 Command Line Tools 时手工拼 xcframework 目录）。冷构建约 4 分钟，静态库约 50 MB（带着整个 std 与 LTO 后的依赖），链接后只留下用到的部分。
