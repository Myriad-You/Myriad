# 常驻三档映射到 backgroundRequirements 六类

Status: accepted

`backgroundRequirements` 声明机制已经存在——manifest 声明（安装时注册）与运行时 `Tapp.background.require()` 两路合并。缺的不是声明，是 ADR 0006 的三档判定没有落到这六类上：`TappBackgroundRunner` 无差别地为所有声明后台需求的应用拉起 headless core，无三档、无名额。

决定：把三档映射定死，并在收集后台应用时按名额准入判定。

## 映射表

| background 类型 | 档位 | 占名额 |
|-|-|-|
| `media` | 事实维持 | 否 |
| `sync` | 名额准入 | 是 |
| `notification` | 名额准入 | 是 |
| `event-listener` | 名额准入 | 是 |
| `realtime` | 名额准入 | 是 |
| `scheduler` | 不授予（平台代管触发） | — |

`realtime` 归名额准入：实时数据更新发生在沙箱内部、平台只看黑盒，与 sync、event-listener 同性质，无法走「事实维持」的资格验证，也没有平台代管它的触发。

`scheduler` 改为不授予：ADR 0006 已定「定时任务平台代管触发，沙箱无需活着」。现状 `TappBackgroundRunner` 仍为声明 scheduler 的应用拉起 headless core，与本档矛盾，改为不再因 scheduler 声明而常驻。

## Consequences

名额准入判定发生在收集阶段（`getBackgroundTapps`），按 ADR 0015 的配置项（每应用 1 个 / 全站 3 个）在申请那一刻拒绝，而不是拉起之后回收——与 ADR 0006「运行实例用准入控制」一致。

事实维持（media）不占名额，但工作经平台通道、资格随时可验证；工作结束自动降为普通隐藏，这一点已在 ADR 0006 定下，不因本映射改变。
