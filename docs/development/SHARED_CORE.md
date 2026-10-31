# 共用核心

共用核心（她的规则：纯 Rust crate，加一个 JSON C ABI 静态库）在单独的仓库 [Myriad-You/myriad-core](https://github.com/Myriad-You/myriad-core)。规则、边界、C 入口、一致性对照和别的平台怎么链接，见那边的 [docs/SHARED_CORE.md](../../core/docs/SHARED_CORE.md)。

## 在这个仓库里

它是 git 子模块 `core/`。后端和前端读的是同一个提交，所以只有这一个 pin：

- 后端的 path 依赖：`core/crates/` 下的 `myriad-merope`、`myriad-agent-rules`、`myriad-persona-sync`、`myriad-persona-log`、`myriad-her-link`、`myriad-nodes`。根 `Cargo.toml` 把 `core` 列在 `workspace.exclude` 里：核心有自己的 workspace，这几个 crate 在这里只当依赖，不是成员，版本由本仓库的 `Cargo.lock` 定。
- 契约在 `core/shared/`：后端编译时读 `her_link_contract.json`，测试读 `her_link_fixtures.json`。
- 形象（子模块 `figure/`）不依赖核心：它在 `figure/contracts/` 带 `merope_rig_contract.json` 和 `merope_performance_contract.json` 的逐字节副本。CI 用 `scripts/extra/figure-contracts.sh check` 检查副本是 `core/` 历史上出现过的某一版；形象的常量与核心 Rust 源码的对照在 `frontend/src/features/merope/figureCore.test.ts`。
- 后端测试还直接读 `core/crates/conformance/` 和 `core/crates/myriad-persona-sync/fixtures/`。
- 本仓库的 `cargo test --workspace` 和 clippy 不包含核心自己的测试，它们在 myriad-core 的持续集成里跑。

克隆之后，以及拉取后子模块指针变了时：

```bash
git submodule update --init
```

## 改核心

1. 先在 myriad-core 里改，合进它的 `main`。
2. 再在这里把子模块挪到那个提交：

   ```bash
   git -C core fetch origin
   git -C core checkout <提交>
   git add core
   ```

   跑后端和前端的测试（契约变了，两边都受影响），和用到新规则的改动放在同一个提交里。
3. 改的是形象契约（`merope_rig_contract.json` 或 `merope_performance_contract.json`）时，在 `figure/` 里跑 `scripts/extra/figure-contracts.sh sync` 拷过去，在 myriad-figure 里提交，再把 `figure/` 挪到那个提交。形象没跟上之前，CI 只提示副本落后，不拦。
4. 原生 App 按它自己的 pin 跟进。

不要在 `core/` 里直接改了提交到这里：子模块指向的提交必须已经在 myriad-core 上。
