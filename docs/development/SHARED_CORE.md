# 共用核心（myriad-core-ffi）

站点以外的平台（目前是 macOS 原生 App，仓库 MyriadNative）要和站点上的她说同样的话、按同样的规则变心情，所以不各自抄一份规则，而是链接同一份 Rust 代码。`crates/myriad-core-ffi` 把纯规则 crate 包成一个静态库，对外只有一个 JSON 进、JSON 出的 C 入口。

站点后端不经过它，照常直接依赖 `myriad-merope`、`myriad-agent-rules`。

## 边界

- **只收纯 crate。** 现在是 `myriad-merope` 和它依赖的 `myriad-agent-rules`。不引入 tokio、reqwest、sea-orm，也不读写文件或网络。
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

1. 在 `src/calls.rs` 写调用函数，按名字的字典序加进 `CALLS`。函数只做 JSON 与 crate 类型之间的转换，规则本身留在原 crate。
2. 在 `src/tests.rs` 写一组往返测试：经 C 入口的输出要和直接调 crate 函数逐字节（浮点逐位）相同。再把名字登记进 `every_call_is_tested_and_version_lists_them`，漏登记会失败。
3. 改了已有调用的输入或输出形状、或删掉调用，要加 `ABI_VERSION`。只是新增调用或可选字段则不必。
4. `cargo test -p myriad-core-ffi`，`cargo clippy -p myriad-core-ffi --all-targets -- -D warnings`。

## 别的平台怎么链接

不从工作区构建：工作区里常有别人还没提交的改动。按一个提交构建：

1. 取这个提交的 `crates/myriad-core-ffi` 与它的全部 path 依赖、`shared/`（`myriad-merope/build.rs` 在编译期读里面的契约）、`Cargo.lock`、`rust-toolchain.toml`，`git archive` 到一个隔离目录。
2. 在那里写一个只列这些 crate 的工作区 `Cargo.toml`，沿用根 `Cargo.toml` 的 `[profile.release]`。Cargo 只会从锁文件里去掉没用到的包，用到的每个包都要与上游 `Cargo.lock` 同版本。
3. 设 `MYRIAD_CORE_UPSTREAM_COMMIT=<提交>`（取归档自己记录的提交号，`git get-tar-commit-id`），`cargo build --release --target <triple> -p myriad-core-ffi`。
4. 链接 `libmyriad_core_ffi.a` 和头文件；启动或测试时断言 `meta.version` 的 `upstreamCommit` 就是钉住的提交。

macOS 原生的做法：MyriadNative 的 `CORE_PIN`、`scripts/build-core.sh` 与 `docs/CORE.md`（只有 Command Line Tools 时手工拼 xcframework 目录）。冷构建约 4 分钟，静态库约 50 MB（带着整个 std 与 LTO 后的依赖），链接后只留下用到的部分。
