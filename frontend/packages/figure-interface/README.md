# @myriad/figure-interface

站点从 `@myriad/figure` 用到的东西：哪些入口、入口里的哪些名字、它们的类型，只有声明，没有运行时代码。站点能拿来构建的每一个形象实现都提供这些。

- `src/<入口>.d.ts` 对着 `@myriad/figure` 的入口（`src/index.d.ts` 是根入口），只列站点用到的名字；声明都在 `src/declarations.d.ts`。行为类型（播放器、运行时、协调器、动作源、端口）只留站点用到的成员；数据类型（表演指令、事件、清单、口型、时间线）是完整的。渲染层的类型直接引 `@myriad-you/figure-player`，和 `@myriad/figure` 钉的是同一个提交。
- `contract/` 是运行时契约：每个实现的测试都用自己的入口调一次 `figureContract`，只经这里声明的名字，测站点和服务端依赖的行为（表演指令和她的状态的校验、通道上的轮次和状态、说话事件和口型时间线、页面什么时候放 2.5D 形象、错误码、rig 校验）。它是测试用的，生产代码不引。

## 怎么检查

站点 `frontend/` 的 `pnpm typecheck` 检查三件事：

1. `tsconfig.figure-interface.json`：把 `@myriad/figure` 换成这里的声明，站点的生产代码照样通过类型检查。站点用了这里没有的名字或成员，这一步报错。
2. `src/features/merope/figureInterface.ts`：`@myriad/figure` 提供这里声明的：每个值在同一个入口里，类型合得上；数据类型两边可以互换；行为类型 `@myriad/figure` 的可以当这里的用。
3. 本包自己（`tsc -p packages/figure-interface`）。

`pnpm test:unit` 跑 `src/features/merope/figureContract.test.ts`，即对 `@myriad/figure` 跑 `contract/`。

## 改它

站点要用 `@myriad/figure` 的新名字、新成员，或者一个名字的类型变了，先改这里，再改站点；第 1 步会指出缺了什么，第 2 步会指出 `@myriad/figure` 哪里不合。每个实现都要提供改过的样子。

## 许可

Apache-2.0（`LICENSE`，全文在仓库根目录的 `LICENSES/Apache-2.0.txt`），和 Tapp 契约一样：不公开的实现也要照着它写。
