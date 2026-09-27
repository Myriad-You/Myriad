# 签名发布升级演练

用于在没有已安装 Myriad 的本地 Docker daemon 上，验证当前 updater/Guard 对真实
已发布业务镜像的升级与恢复。需要 Docker Compose、Python 3、Bash、jq，以及访问
GitHub、Docker Hub、Fulcio/Rekor 的网络。脚本拒绝在已有 Myriad 固定容器名的 daemon
上运行；不要把生产 daemon 当作测试环境。

## 执行

在仓库根目录构建本次源码的测试镜像，再运行演练。首次构建和拉取业务镜像需要时间。

```bash
docker build --build-arg CARGO_PROFILE=dev --build-arg MYRIAD_VERSION=v0.5.8 \
  -t myriad-updater-dev:storage-rehearsal -f updater/Dockerfile .
python3 scripts/extra/test-signed-release.py --out /tmp/myriad-release-evidence
```

默认用 v0.5.6 作为基线、v0.5.7 作为目标。`--from-version`、`--to-version` 可选择
其他正式版本，但应先核查它们的部署契约和数据库兼容性。`--out` 必须是尚不存在的目录。
此命令不发布新版本、标签或镜像。

演练执行以下检查：

1. 使用正式 Compose 挂载声明和宿主准备函数，一次 `compose up` 启动整栈。
2. 向真实 updater HTTP API 请求正式发布升级，保持 `COSIGN_VERIFY=strict`。
   updater 从 GitHub 下载原始发布清单、签名和证书，调用镜像内的 Cosign v2.4.1，
   拉取正式镜像并核对清单中的摘要。
3. 升级后核对实际容器健康、数据库与媒体校验值，以及 worker 的挂载来源和读写模式。
   本地初始模板用直接 bind，v0.5.7 的目标模板使用 named volumes；更新后必须继续
   使用原目录，不能注册新的 backend 卷。
4. 通过 updater `/rollback` 恢复真实 PostgreSQL 文件快照，核对旧版本重新就绪。
   数据库回到快照值，媒体保留升级后的新写入——媒体不属于 updater 的数据库快照。
5. 再次请求升级，在切换标签和 Compose 后，由测试代理拒绝一次初始化容器创建请求。
   检查 updater 自动恢复旧版本、数据库与媒体，并清除维护状态。

异常时保留脱敏日志与 job 结果；正常和异常退出都会尝试清理本次项目的容器、网络和
临时数据。`results.json` 的 `cleanup_ok` 表示 Compose 清理是否成功。镜像和构建缓存
会保留，以便复跑。测试凭据随机生成，证据文件不包含它们。

## 验签负例

本机 PATH 有与 updater 镜像相同的 Cosign v2.4.1 时，可单独执行：

```bash
cargo test --manifest-path updater/Cargo.toml --test signed_release -- --ignored --nocapture
```

此测试直接下载并验证 v0.5.7 的真实发布物，再分别修改 JSON 字节、指定错误仓库身份、
删除签名文件，确认严格模式全部拒绝。该测试默认忽略，避免普通单元测试依赖外网。
不要用 `--insecure-ignore-tlog`、关闭验签或伪造 Cosign 成功输出来消除失败。

## 边界

业务镜像、签名、摘要、Docker 操作、Guard 挂载校验、PostgreSQL 快照和恢复均为真实
路径。唯一的传输故障由 `fixtures/guard-fault-proxy.py` 注入，它将其他请求原样转发给
Guard，不替代 Guard 的权限判断。

本地尚未发布的 updater/Guard 使用项目已有的 debug 镜像身份入口。此演练**不覆盖**
新 updater 自身的正式摘要身份引导、TCB 自更新交接，也不会为未发布代码生成 GitHub
OIDC 签名。要验证这些环节，仍需正式签名制品及独立的控制平面升级演练。
