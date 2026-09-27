<!--
Thanks for contributing! PRs target `preview`. Fill in what applies and delete the rest.
感谢贡献！PR 请发到 `preview` 分支。按实际情况填写，不适用的部分可删去。
-->

## What & why · 改了什么、为什么

<!-- The problem this solves and the approach you took. Link the issue if there is one. -->
<!-- 解决什么问题、采用了什么做法。有对应 issue 请关联。 -->

Closes #

## Type · 类型

- [ ] `fix` Bug fix · 修复
- [ ] `feat` New feature · 新功能
- [ ] `perf` Performance · 性能
- [ ] `refactor` No behavior change · 重构（行为不变）
- [ ] `docs` / `test` / `ci` / `chore`
- [ ] ☢️ Breaking change (config, API, Tapp contract, or data migration users must act on) · 破坏性变更（配置、API、Tapp 契约或需要用户处理的数据迁移）

## Area · 涉及范围

- [ ] `frontend`
- [ ] `backend` / `crates/*`
- [ ] `backend/migrations` (database schema · 数据库结构)
- [ ] Tapp contract / `tools/tapp-cli`
- [ ] `proxy` / `updater`
- [ ] Docker / CI / docs

## How it was tested · 如何验证

<!-- Commands you ran and what you checked by hand. For UI changes, add before/after screenshots. -->
<!-- 列出运行过的命令和手动检查过的内容。界面改动请附改动前后截图。 -->

```
cd frontend && pnpm test
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Checklist · 自查

- [ ] Commits follow [Conventional Commits](https://www.conventionalcommits.org/), one reviewable change per commit. · 提交遵循 Conventional Commits，一次提交一个可评审单元。
- [ ] No drafts, scratch specs, or agent notes committed. · 没有提交草稿、临时规格或代理笔记。
- [ ] New or changed UI copy is added in all locales (`zh-CN` `zh-TW` `en-US` `ja-JP` `ko-KR` `fr-FR` `de-DE`). · 新增或修改的 UI 文案已补齐所有语言。
- [ ] Terms match [`CONTEXT.md`](https://github.com/Myriad-You/Myriad/blob/preview/CONTEXT.md); new project-specific terms are added there. · 术语与 `CONTEXT.md` 一致；新出现的项目术语已补进去。

### If you touched permissions, secrets, or config · 如涉及权限、密钥或配置

- [ ] Permission changes name the layer they affect: **declared**, **approved**, or **granted**. Only granted permissions decide behavior. · 权限改动说明了动的是哪一层：声明、批准还是授予。只有授予权限决定行为。
- [ ] Host secrets and installation credentials never appear in anything returned to a Tapp — error messages included. · 宿主密钥与应用凭据不出现在任何返回给 Tapp 的载荷里，错误信息也算。
- [ ] A new platform config default also has its branch for parsing the value back from the database. · 新增平台配置默认值时，同时补上了从数据库读回的解析分支。
- [ ] Tapp sandbox lifecycle: when the authorization premise goes away (uninstall, code change, sign-in change, permission change) the sandbox is **destroyed**; when only the user's attention moves away it may be kept alive, never the reverse. · Tapp 沙箱生命周期：授权前提消失（卸载、代码变更、登录态变更、权限变更）必须销毁；只是用户注意力转移才可以保留，不能反过来。
