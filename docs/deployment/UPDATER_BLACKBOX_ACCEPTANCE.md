# Updater mock 黑盒验收

验收日期：2026-09-21。对应 PR：[#562](https://github.com/Myriad-You/Myriad/pull/562)。

**最终结论：自动恢复验收未通过。** 正常业务升级、切换版本前中断恢复、proxy 替换后重启收尾已通过；业务更新在切换 tag 后、启动新服务后中断，重启仍进入 `needs_manual` 并保持维护模式。这两处需要人工介入，不符合本次自动化要求。

本轮测试命令返回成功：14 项进程级测试通过，严格 Clippy 通过。其中一项是已知缺陷的复现测试，断言当前确实进入 `needs_manual`，因此不能把“14 项测试通过”写成“断电恢复验收通过”。

## 目的与方法

验证用户发起升级后，更新器遇到突然中断，能否自动恢复到可服务且状态一致的结果。成功的标准包括：部署镜像与记录一致、维护模式解除、没有遗留进行中的任务、不要求用户补做操作；已完成的替换不应重复执行。

测试启动编译出的真实 updater，通过 HTTP 发起更新。选版、任务落盘、版本切换、启动恢复和结果写入均由生产代码执行；Docker API、Compose 命令、仓库和应用健康响应由测试替身提供。helper 测试另外启动真实自更新执行器。

新增中断演练从 HTTP 请求开始，让真实更新流程运行到指定外部操作。测试随后向 updater 及其 Compose 子进程所在的进程组发送 `SIGKILL`，不执行正常退出处理；保留同一份 `.env`、任务和维护状态，再启动 updater。故障注入只改变测试替身，不在生产代码增加测试开关，也不预先编造恢复任务。

这模拟更新执行器突然消失、已完成的外部操作仍然存在的故障。它不是物理主机断电测试，不模拟文件系统缓存丢失、磁盘损坏，也没有验证真实 Docker 的主机重启行为。

## 如何运行

在 Linux 上，使用仓库支持的 Rust 工具链，并具备 `/bin/sh`、`sed` 等基本命令。从仓库根目录运行：

```sh
cd updater
cargo test --test component_update_api --test self_update_handoff -- --nocapture
cargo clippy --all-targets -- -D warnings
```

这两个黑盒测试目标不需要真实 Docker daemon、镜像仓库凭据或正在运行的 Myriad 部署；测试会创建临时目录、回环 HTTP 服务和替代的 Docker 命令。它们也是现有 `cargo test` 的一部分。

只运行本轮新增的升级和中断用例：

```sh
cargo test --test component_update_api business -- --nocapture
cargo test --test component_update_api power_cut -- --nocapture
```

第二条会再次运行名称含 `power_cut` 的业务用例。已知缺陷复现会输出：

```text
ACCEPTANCE GAP: cut=swapped, restart requires manual recovery
ACCEPTANCE GAP: cut=started, restart requires manual recovery
```

看到这些输出表示自动恢复验收未通过，即使测试进程退出码为 0。修复恢复策略时，应将对应测试改为断言自动恢复后的版本、服务、任务和维护状态，不能只删除输出或改写报告。

## 本轮结果

| 演练 | 观察到的结果 | 验收 |
| --- | --- | --- |
| 从 `v0.5.3` 标记的 mock 部署发起业务升级 | HTTP 受理后完成更新，任务成功，维护模式解除 | 通过 |
| 成功落盘后强杀、重启 | 保持目标版本，不再次启动服务，不重新进入维护 | 通过 |
| 停止旧服务后、切换 tag 前强杀 | 重启自动启动旧服务，清除维护和进行中任务 | 通过 |
| `.env` 已切换目标 tag，卷初始化操作期间强杀 | 重启进入 `needs_manual`，维护模式不解除 | **未通过** |
| 新服务已启动、Compose 尚未返回时强杀 | 重启进入 `needs_manual`，维护模式不解除 | **未通过** |
| 自更新请求已受理、查询仓库期间强杀 | 重启自动重新查询；模拟仓库错误后记录失败，没有误交接给 Guard | 通过；仅覆盖交接前恢复 |
| proxy 已替换、Compose 尚未返回时强杀 | 重启识别健康的目标镜像并记录成功，累计只拉取、替换一次 | 通过 |

原有 9 项进程测试也重新执行通过，覆盖旧 Guard 响应丢失、受理记录兼容、proxy 失败回退及 helper 切换/恢复等行为。其中一些通过预置旧协议状态验证恢复入口，不能算作从升级开始的中断演练。

源码入口：

- [业务升级与中断演练](../../updater/tests/support/business_update.rs)：正常升级、停止后恢复、两处 `needs_manual` 缺陷复现。
- [组件 API 黑盒](../../updater/tests/component_update_api.rs)：真实 daemon、HTTP 请求、进程组强杀、自更新查询中断、proxy 替换中断。
- [helper 黑盒](../../updater/tests/self_update_handoff.rs)：真实 helper，模拟 Compose 和容器状态。

本轮没有覆盖内置 PostgreSQL 快照/恢复过程中的中断、数据库迁移、真实联邦 worker 启动，也没有覆盖 Guard/helper 三组件整体自更新期间的断电。业务测试使用外部数据库模式和简化的后端/前端编排，`v0.5.3` 是 fixture 的起始版本标记，不能代替完整 v0.5.3 发行部署的升级证明。

## 两处自动恢复失败的原因

[`worker/recovery.rs:126`](../../updater/src/worker/recovery.rs#L126) 在 `SwapTag` 阶段发现 `.env` 已指向目标版本时直接选择 `NeedsManual`；[`worker/recovery.rs:166`](../../updater/src/worker/recovery.rs#L166) 对后续启动、健康检查和回滚阶段也执行同一策略。测试复现的是既定恢复逻辑，不是测试环境缺少 Docker。

后续修复应复用现有更新/回滚执行路径，使任务在重启后有明确的自动完成或恢复结果，并增加相应中断断言。不能直接清掉维护标记，也不能在数据库状态不明时只改回版本号就宣称恢复成功。本轮只补测试和分析，尚未修改这项生产恢复策略。

## 截图中的挂载拦截

截图报错来自 [`worker/preflight_env.rs:208`](../../updater/src/worker/preflight_env.rs#L208) 的 `check_federation_http_storage`。它读取宿主 Compose 的解析结果，要求只读数据根目录加四个固定可写挂载；缺少 `backend_data/media → /app/data/media`，同时挂载总数为 4 而非 5，于是拒绝进入更新。预检发生在维护模式之前，见 [`worker/update.rs:85`](../../updater/src/worker/update.rs#L85)。

这里要区分实际需求与检查方式：

- **`media` 可写有真实用途。** 联邦上传入口 [`backend/src/api/federation/social.rs:500`](../../backend/src/api/federation/social.rs#L500) 调用平台媒体存储；[`media/mod.rs:80`](../../backend/src/services/media/mod.rs#L80) 使用 `DataPaths.media`，它来自 [`data_paths.rs:55`](../../backend/src/services/data_paths.rs#L55) 的 `root.join("media")`。根目录只读、又没有这个可写子挂载，实际上传会无法写入。只删除拦截并不能修好更新后的服务。
- **“挂载必须恰好为 5 个”不是业务能力要求。** [`preflight_env.rs:270`](../../updater/src/worker/preflight_env.rs#L270) 把模板数量当成了升级条件。新增挂载是否允许应由权限边界判断；总数本身不能说明上传能否工作。
- **路径事实存在重复维护。** updater 的必需挂载清单与 [`docker/guard/validate.rs:690`](../../updater/src/docker/guard/validate.rs#L690) 的允许挂载清单各写一份目录映射。两者职责分别是运行条件和权限上限，不能简单删掉权限边界，但目录事实不应漂移成两套。
- **真正增加用户成本的是缺少部署迁移。** 发布模板已经包含 `media` 挂载，并不意味着已有宿主 Compose 会跟着变化。当前检查要求用户先手改 Compose，再重新升级，把发布引入的编排变化交给了用户。兼容应覆盖现有部署的自动迁移，不能只证明新模板可用。

因此，该错误发现的缺写入路径需要解决；固定数量检查和要求用户手动同步发布模板的流程没有必要保留。应由负责部署编排的现有路径补齐必要变更，再执行升级，并用旧编排作为黑盒起点验证。不能通过扩大 worker 的整个数据根目录写权限来省略迁移。

截图只证明当次解析到的挂载缺少 `media`；未读取实际部署文件，不能据此判断它具体来自哪个旧版本。本轮没有自动迁移 Compose，也没有把该问题标为已修复。
