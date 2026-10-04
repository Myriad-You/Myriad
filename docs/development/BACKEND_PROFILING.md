# 后端占用分析

使用 [hotpath-rs](https://github.com/pawurb/hotpath-rs) 分开观察函数耗时、累计分配和进程 RSS。
默认构建不启用它，也不编译 profiler 依赖。`hotpath-alloc` 包装当前分配器：默认仍是
mimalloc，`--no-default-features` 时为系统分配器。不要用更换分配器后的结果比较业务优化。

## 功能与边界

- `hotpath`：函数耗时，以及 Web / 合并进程、人设进程、联邦进程的业务路由统计。
- `hotpath-alloc`：额外记录函数与请求的累计分配字节和次数，自动启用 `hotpath`。
- `hotpath-cpu`：额外启用 CPU 采样归因，自动启用 `hotpath`；仅支持 macOS / Linux。
- `profiling` Cargo profile：优化编译且保留符号，供 CPU 归因使用。

起始插桩覆盖平台快照和画像投影读取、资料库分页和来源统计、原始缓存合并和条目补全、
TAPP API 执行与响应缓存、定期内存回收。
后台 worker 共用进程入口的 profiling guard，正常关闭时输出报告。姿态推理子进程独立于
服务器负载，当前不启动 profiling guard。

函数耗时包含等待；不能把异步函数的等待时间当成 CPU 时间。分配统计是累计申请量，
不能当作存活堆大小或 RSS。当前默认按函数自身分配统计；若设置
`HOTPATH_ALLOC_CUMULATIVE=true` 则包含嵌套调用，此时不能逐行相加。路由统计止于响应头完成，
不包含 SSE / 下载流的整个生命周期。默认插桩不记录参数、返回值、SQL、请求正文或凭据。

## 可重复的平台补全负载

在仓库根目录运行；负载纯内存构造，不读取平台缓存、不连接数据库、不调用外部模型：

```sh
HOTPATH_METRICS_SERVER_OFF=true \
HOTPATH_OUTPUT_FORMAT=json-pretty \
HOTPATH_OUTPUT_PATH=/tmp/myriad-enrichment.json \
cargo test -p myriad-backend --locked --features hotpath-alloc \
  profile_raw_enrichment -- --ignored --nocapture --test-threads=1
```

每个平台构造 1,000 条原始记录，每条有 2 KiB 不参与补全的字段，补全 100 条输出，
重复 50 次。构造样本与复制输出位于被测函数之外。对比报告中 GitHub、Xbox、Bangumi、
MAL、网易云五个 `enrich_*_items` 的平均耗时和每次分配；保存修改前后独立的报告。
这是 debug 构建下的定向负载，不能由此推算生产请求延迟或全后端节省百分比。

原始缓存合并可将上述命令的测试名换为 `profile_platform_cache_assembly`，并更换报告路径。
该负载合并四个平台，各 1,000 条记录，每条含 2 KiB 附加字段，重复 20 次。
输入准备在被测函数外；报告只统计合并本身，不含磁盘读取或 JSON 解析。

缓存命中耗时可独立测量，不启用 profiler；4096 个 `(模型, 文本摘要)` 键，
每批 20,000 次查询，五批取中位数，不包含调用方的向量复制或互斥锁等待：

```sh
cargo test -p myriad-backend --locked profile_retained_cache_hits \
  -- --ignored --nocapture --test-threads=1
```

需要只测缓存实现的优化编译结果时，该模块只依赖标准库，可以单独编译同一个测试：

```sh
rustc --edition=2024 -O --test backend/src/services/retained_cache.rs \
  -o /tmp/myriad-cache-bench
/tmp/myriad-cache-bench profile_retained_cache_hits \
  --ignored --nocapture --test-threads=1
```

数据库元数据合并的定向负载保留旧版复制实现作为对照，构造 1,000 首歌、十个分片，
每首歌含 2 KiB 附加字段；两种实现各测 30 次，输入准备不计入耗时。它不含查询和 JSON 解码：

```sh
cargo test -p myriad-backend --locked profile_metadata_merge \
  -- --ignored --nocapture --test-threads=1
```

也可加 `--features hotpath-alloc` 查看 `cloned_metadata` / `merge_latest_metadata` 的分配。
资料库缓存的并发、刷新、取消和空闲回收契约可运行 `library_items::cache_tests`。
真实服务并发冷读须等待最后一次资料库请求后 31 秒，再同时发起请求；空闲回收由
60 秒维护任务处理，观察期至少 90 秒，不能将「到 TTL 不再命中」当成「已释放存活数据」。

资料库缓存命中后的过滤和分页可独立测量：构造 20,000 个条目，请求偏移 75、每页 50 条，
交替测旧版和当前实现各 30 次。保留旧版来源复制和全量匹配引用数组作对照；不含数据库、
可用来源统计和响应序列化，不能据此推算整个接口的延迟或 RSS。别名、缺失配置、显式空来源、
超大偏移和分页边界的等价性回归可运行 `library_items::paging_tests`。

```sh
cargo test -p myriad-backend --locked profile_warm_library_paging \
  -- --ignored --nocapture --test-threads=1
```

也可加 `--features hotpath-alloc` 查看 `previous_pagination` / `paginate_library_items` 的累计分配。

可用来源统计随资料库条目一起保留，刷新失效和 TTL 回收同时丢弃二者；偏好仍按请求读取。
将测试名换为 `profile_warm_library_sources`，可交替比较每次重新扫描与复用统计的成本，
两种路径均包含将来源统计转换为响应 JSON Value 的开销。样本同样有 20,000 个条目。
启用 `hotpath-alloc` 时，可设置 `HOTPATH_ALLOC_CUMULATIVE=true` 查看
`previous_source_response` / `cached_source_response` 包含子函数的分配；此时不要再与子函数相加。

资料接口只从数据库读取平台画像字段，避免为了头像和简介解析整份歌单、视频和仓库数据。
SQL 投影兼容 PostgreSQL 的 `json` 和 `jsonb` 列，保留字段空值、`user` / `user_info` 优先级、
最新记录和数据库存在时的磁盘回退规则；真实列类型也必须核验，不能只用 JSONB 测试样本。
首页头像和文案复用单次请求内的画像读取；独立调用、后续请求和其他用户不复用该数据。
设置专用 PostgreSQL 测试库的 `AVATAR_PROFILE_TEST_DATABASE_URL` 后，可运行实际 SQL 回归：

```sh
cargo test -p myriad-backend --locked \
  profile_projection_preserves_shapes_precedence_and_database_presence \
  -- --ignored --nocapture --test-threads=1
```

该测试用 CTE 样本遮蔽真实表，不写入持久数据；未配置测试数据库时默认忽略。
同一环境变量下，`profile_request_shares_profiles_without_cross_request_or_user_staleness`
使用单连接的临时表验证请求内复用、后续请求更新、非站长隔离和当前归属重查。

TAPP 响应缓存淘汰可用独立策略负载测量。构造 2,048 条逻辑大小为 40 KiB 的缓存记录，
交替运行旧版与当前实现各 30 次，覆盖预算不变、单条淘汰、切换节省内存模式，以及仅缩小
字节预算的情形。相同的截止时间和缓存年龄用于核对保留结果；输入复制不计时，样本保留
共享载荷以排除 JSON 解码与载荷释放。该结果只反映缓存策略 CPU 成本，不代表完整接口延迟
或 RSS。临时排序数组增加少量工作内存，不改变常驻缓存预算。

```sh
cargo test -p myriad-backend --locked profile_response_cache_trim \
  -- --ignored --nocapture --test-threads=1
```

也可加 `--features hotpath-alloc` 比较 `previous_trim_response_cache` /
`trim_response_cache` 的累计分配。`tapp_api_service::cache_trim_tests` 验证 TTL、
双预算和最旧优先淘汰，以及缓存命中允许并发读、过期删除释放载荷。

## 实际服务负载

使用已有的本地开发配置和空闲端口，分别记录同角色、同数据、同负载的修改前后结果。
`MYRIAD_PROCESS_ROLE` 应与待优化的进程一致：`web`、`persona-worker`、
`federation-worker` 或本地 `all`。启动方式和数据目录见 [BUILD.md](BUILD.md)。

```sh
HOTPATH_METRICS_SERVER_OFF=true \
HOTPATH_OUTPUT_FORMAT=json-pretty \
HOTPATH_OUTPUT_PATH=/tmp/myriad-backend-profile.json \
cargo run -p myriad-backend --locked --profile profiling --features hotpath-alloc
```

预热后执行相同数量 / 并发的业务请求，记录吞吐和延迟，同时按秒采样进程 CPU、RSS。
负载结束后观察 RSS 是否回落，再通过 Ctrl-C 正常关闭进程以输出报告。SIGKILL 无法
刷新报告。测常驻占用时还要跑不启用 profiler 的同一负载，排除 profiler 自身的影响。

需要实时读取时去掉 `HOTPATH_METRICS_SERVER_OFF=true`。上游 metrics 服务绑定
`127.0.0.1:6770`；多进程各设不同的 `HOTPATH_METRICS_PORT`。
输出及采样选项见[上游配置](https://hotpath.rs/configuration)。
CPU 采样还需要上游的 samply 工具和操作系统支持，见[CPU profiling](https://hotpath.rs/cpu_profiling)。
