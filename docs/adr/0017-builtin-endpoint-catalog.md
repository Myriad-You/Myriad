# builtin 端点权限判定收敛为声明式端点目录

Status: accepted

builtin 端点的权限绑定现在是两处硬编码 match——安装时校验一处（`tapp_validation.rs`）、调用时判定一处（`declared_api.rs`）。两处不对称，新增一个 builtin 端点要同时改两处，漏一处就会出现「装得进去、调不通」或「装不进去、却能调」的错位。

决定：把 builtin 端点的权限要求收敛成一个声明式**端点目录**（单一事实源）。安装校验与调用判定都从目录读，不再各自 match。新增 builtin 端点 = 目录里加一行声明，两处判定自动生效。

## 首批目录条目

| 端点 | 所需权限 |
|-|-|
| `geo` | 无 |
| `ai:chat` | `ai:chat` |
| `ai:generate` | `ai:generate` |

## Consequences

这是飞书「能力位图 → 端点目录」主线在 builtin 侧的结构化落地，但**不新增任何端点**，也不为入站端点（G3）提前实现任何东西——G3 仍在范围外。

目录只覆盖平台提供的 builtin 端点。http 出站端点的 access 分级是 manifest 自带字段（应用自己声明 Public/Protected/Manager），判定路径不同，不在本目录的收敛范围内——这点是刻意区分，不是遗漏。
