# @myriad-you/figure-interface

站点从它的 2.5D 形象（`@myriad/figure`）用到的东西：哪些入口、入口里的哪些名字、它们的类型，只有声明，没有运行时代码；加上每个实现都要通过的运行时契约。站点能拿来构建的每一个形象实现都提供这些。

- `src/<入口>.d.ts` 对着 `@myriad/figure` 的入口（`src/index.d.ts` 是根入口），只列站点用到的名字；声明都在 `src/declarations.d.ts`。
  - 类（播放器、协调器、动作源）和标了 `@behaviour` 的类型（运行时、触摸的动作源）只留站点用到的成员。
  - 数据类型（表演指令、事件、清单、口型、时间线）是完整的。
- 渲染层的类型直接引 `@myriad-you/figure-player`。本包不钉渲染层：用它的一方装了哪个渲染层，它就读哪个。在站点里，`pnpm-workspace.yaml` 的 `publicHoistPattern` 把 `@myriad/figure` 钉的那一个放到 `frontend/node_modules/@myriad-you/figure-player`，检查会确认两边读的是同一份。
- `contract/` 是运行时契约。每个实现的测试都用自己的入口调一次 `figureContract`，只经这里声明的名字，测站点和服务端依赖的行为：
  - 表演指令和她的状态的校验、rig 校验（正反两面，`contract/fixtures/` 有一份合法的半身清单）；
  - 通道上的轮次和状态、说话事件和口型时间线、音乐特征；
  - 运行时的持有和释放、租约计数、活的脸、订阅；
  - 触摸的绑定和解绑、评估和相遇；
  - 播放器的换装（失败时留着上一套）、`tick`、`dispose`，在 `contract/headless.ts` 的无头页面和假 WebGL2 上跑；
  - 工作台的表情程度和姿势修正的界限。

  它只给测试用，生产代码不引。跑它要 `node:test` 和能直接跑 TypeScript 的加载器（比如 `tsx`）。

## 怎么检查

站点 `frontend/` 的 `pnpm typecheck` 跑 `scripts/figure-interface/check.mjs`，它查三件事：

1. **站点对着接口。** 把 `@myriad/figure` 换成这里的声明，站点的生产代码照样通过类型检查，而且没有一个文件碰到实现的文件：接口没声明的入口（比如 `@myriad/figure/testing`）、指向 `figure/` 的相对路径，都算碰到。测试和浏览器夹具也查：它们换成接口后不许多出真 figure 下没有的错误。`check.mjs` 里列出的几个是专测某一个实现内部的（它的 `testing` 入口、运行时内部、rig 导入器），不查，以后跟着那个实现的测试走。
2. **实现对着接口。** 每个入口声明的每个名字，从接口里读出来，不是手写的清单，都要在 `@myriad/figure` 的同一个入口里：
   - 值要合得上；
   - 数据类型两边可以互换；
   - 类和 `@behaviour` 类型是 figure 的可以当这里的用。
3. **同一个渲染层。** 接口和 figure 读到的 `@myriad-you/figure-player` 是同一份。

`tsc -p packages/figure-interface` 查本包自己，包括 `contract/`。`pnpm test:unit` 跑 `src/features/merope/figureContract.test.ts`，即对 `@myriad/figure` 跑契约。

### 参数为什么两边比，为什么两步都要过

第 2 步比较函数时，参数按方法的规则两边比（双变）：只要接口的参数类型和实现的参数类型有一边能当另一边用就算合。这是有意的。接口只描述站点用到的成员，像协调器、运行时这样的参数，接口里的类型比实现的少很多成员；严格的逆变比较会把每个这样的函数都判成不合。

代价是：只看第 2 步，一个实现可以要求比接口多的参数成员，也能过。所以另一个实现必须**两步都过**：

- 第 1 步保证站点只用接口里有的东西。它传进去的值都是照接口的类型构造的。
- 第 2 步保证实现提供了这些名字，返回的东西接口用得上。

两步合起来，站点换成另一个实现也照样编得过。运行时的行为由契约的测试守着。

## 别的仓库怎么用

建议：和渲染层一样，按 `@myriad-you/figure-interface` 发布到 npm（Apache-2.0，`publishConfig.access` 是 `public`，`files` 只带 `src`、`contract`、README 和 LICENSE），不公开的实现钉一个版本。

- 第 2 步在那边照做：一个文件把它自己的入口和这里的入口逐个比，像 `check.mjs` 生成的那样。
- 测试里调 `figureContract`。
- 本包的版本跟站点的需要走；站点改了接口，就发一个新版本，实现跟上之后站点再换。

发布之前，用 pnpm 的 git 子目录依赖钉站点的一个提交：

```json
"@myriad-you/figure-interface": "github:Myriad-You/Myriad#<提交>&path:/frontend/packages/figure-interface"
```

这要下载整个站点仓库在那个提交的压缩包，只适合过渡。

## 改它

站点要用 `@myriad/figure` 的新名字、新成员，或者一个名字的类型变了，先改这里，再改站点：第 1 步会指出缺了什么，第 2 步会指出 `@myriad/figure` 哪里不合。每个实现都要提供改过的样子。

## 许可

Apache-2.0（`LICENSE`），和 Tapp 契约一样：不公开的实现也要照着它写。
