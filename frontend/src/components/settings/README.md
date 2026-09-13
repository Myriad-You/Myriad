# 通用设置组件架构

设置页统一使用本目录下的组件，避免各区块手写原生 `select` / `checkbox` / toggle DOM。

## 目录

```
settings/
├── types.ts                 # 类型定义
├── SettingItem.tsx          # 按 type 分发的工厂组件
├── SettingGroup.tsx         # 分组容器（子分类标题 + 可选 switch）
├── SettingGroupGrid.tsx     # 多子分类网格（2 列自适应嵌套）
├── SettingSection.tsx       # 带标题/图标的区块
├── SectionSwitch.tsx        # 一级分类切换（退场 + 高度）
├── AutoHeight.tsx           # contentKey 变化时高度过渡（平台列表↔二级页）
├── CollapseRegion.tsx       # 组折叠 0fr→1fr
├── CompactSettingGroup.tsx
├── InfoCard.tsx
├── items/
│   ├── ToggleSwitch.tsx     # 统一开关控件（无 label 壳）
│   ├── SwitchItem.tsx       # 开关设置项
│   ├── InputItem.tsx        # 文本输入
│   ├── NumberItem.tsx       # 数字输入
│   ├── SliderItem.tsx       # 滑动条
│   ├── SelectItem.tsx       # 下拉（内部用 FieldSelect）
│   ├── FieldSelect.tsx      # 自定义 listbox（替代原生 select）
│   ├── CheckboxItem.tsx
│   ├── CheckboxCard.tsx     # 单张开关卡（权限下放同款）
│   ├── CheckboxGroupItem.tsx
│   ├── NumberGroupItem.tsx
│   ├── ProviderItem.tsx
│   ├── ButtonItem.tsx
│   ├── SettingsButton.tsx   # 统一操作按钮
│   ├── ChoiceControls.tsx   # SegmentedControl（统一选择）
│   └── SettingItem.css      # 含 toggle-switch / checkbox-group-card
├── SetupFlow.tsx            # 流程图式步骤引导
├── ManagedList.tsx          # 可操作列表
├── InfoActionCard.tsx       # 信息卡 + 操作按钮
├── presets/
│   ├── PermissionGroup.tsx
│   └── QuotaGroup.tsx
└── index.ts
```

## 何时用哪个

| 场景                                                  | 组件                                                                                                                                     |
| ----------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| **操作按钮**（工具栏 / 弹层 / 行内 / 浮动保存）       | **`SettingsButton`**                                                                                                                     |
| 带标签的按钮设置行                                    | `ButtonItem`（内部用 `SettingsButton`）                                                                                                  |
| **单选 / 多选分段**（预设、可见性、筛选、来源、频率） | **`SegmentedControl`**                                                                                                                   |
| 带标签的开关设置行                                    | `SwitchItem`（可选 `preview`）                                                                                                           |
| 卡片头/紧凑行内开关（无整行 label 壳）                | `ToggleSwitch`（可选 `preview`：悬停预告开启/关掉以后会怎样）                                                                            |
| 文本 / 密码 / URL / email                             | `InputItem`                                                                                                                              |
| · `variant="clickToEdit"`                             | 只读 → 点编辑 → 框内保存（`onCommit`）                                                                                                   |
| · `variant="imageUpload"`                             | 左侧预览 + URL / 本地 data URL 上传（网站图标等）                                                                                        |
| 数字 + 单位                                           | `NumberItem`                                                                                                                             |
| **滑动条**                                            | **`SliderItem`**（`min` / `max` / `step`；两端数值 + 可选 `startLabel` / `endLabel`；刻度强制最多 10；可选 `recommendedValue` 推荐标记） |
| 下拉（必须用自定义 listbox，勿用原生 `<select>`）     | `FieldSelect` 或 `SelectItem`                                                                                                            |
| **卡片式开关 / list chrome**                          | **`CheckboxCard`**（圆点 + 可选图标 + 标题 + 介绍）；`variant="switch"` 翻转；`variant="action"` 仅点击行为不同；`icon` 可选             |
| 卡片式多选（带说明文案）                              | `CheckboxGroupItem`（内部 `CheckboxCard` switch）                                                                                        |
| 可操作列表（统计 + 工具栏 + 行操作 + 可选添加表单）   | `ManagedList`（chrome 用 `CheckboxCard` action；危险/主行动作用 `SettingsButton`；`form` 插槽）                                          |
| **信息卡 + 操作按钮**（身份摘要 / 状态说明后行动）    | **`InfoActionCard`**（`fields` + `actions`，按钮用 `SettingsButton`）                                                                    |
| **列表统计行开关**（数值后面；list 小尺寸密度）       | `CheckboxCard` `size="sm"` / `ManagedList` `stats` 的 `kind: 'switch'`（渲染顺序：metric → switch）                                      |
| 一级区块                                              | `SettingSection`                                                                                                                         |
| **同壳内大块内容换页**（高度平滑）                    | **`AutoHeight`**（`contentKey`；平台列表↔详情）                                                                                          |
| **一级分类切换**                                      | **`SectionSwitch`**（退场/进场；高度委托 `AutoHeight`）                                                                                  |
| 子分组                                                | `SettingGroup`（标题；可选 `titleExtra` / **`switch`** / **`detail`**）                                                                  |
| **多子分类网格**（嵌套多个 SettingGroup，2 列自适应） | **`SettingGroupGrid`**（`columns` / `minColumnWidth` / `variant` / **`align`**）                                                         |
| **标题旁标签**（跳转 / 轻提示）                       | **`SettingTitleTag`**（经 `SettingGroup` 的 `titleExtra`；可选 `detail`；`variant="danger"` 报错；`onDismiss` 可关闭）                   |
| **选项字段报错**（贴在 label 旁）                     | **`SettingFieldErrorTag`** / 设置项 `error` prop（内部用 `SettingTitleTag` danger；更新器区块除外）                                      |
| **默认值已更新**（可关闭 / 可一点应用）               | **`SettingDefaultChangeTag`**（直接使用时传 `fieldKey`，设置项通过 `itemKey` 关联；可选 `onApply`；点标签写入新默认并关闭；× 仅关闭）    |

设置项的 `itemKey` 是逻辑字段键，用于表单名称与提示元数据；DOM id 按组件实例生成，重复字段键也不会相互抢占标签。导览定位使用 `guidePath`。

### 默认值变更提示

页面通过 `SettingsDefaultsProvider` 注入提示源；未注入时通用控件不显示产品提示。设置页的 `ConfigDefaultsProvider` 负责默认值目录与持久化。产品默认变更时，在对应选项标题旁显示「默认值有更新」标签（可 × 关闭，状态存 `localStorage`）。

1. 修改代码默认时，同步更新 `config/settingDefaultChanges.ts` 里的 **`SETTING_PRODUCT_DEFAULTS`**（与 `defaultFieldValues` / 后端默认对齐）。
2. 用户点标签「默认值有更新」→ 写入新默认并关闭提示；点 × → 只关闭、不改值。
3. 关闭后不再显示该次 `from→to` 变更（已写入 draft 的仍需用户保存配置）。
4. 调试可在控制台：`localStorage.removeItem('myriad_setting_default_notices_v1')` 后刷新。

### 未保存内容阻断

需要保护草稿的页面统一调用 `hooks/useUnsavedChangesGuard(when, message)`。当前它用 `beforeunload` 处理刷新、关闭标签页和地址栏跳转；浏览器自行显示原生提示，不能依赖自定义文本。项目使用声明式 `BrowserRouter`，SPA 离开需要通过兼容该路由的统一导航门处理，不能直接调用 data router 的 `useBlocker`。
| **标题详细说明**（默认隐藏，hover ⓘ 显示） | **`SettingTitleHelp`** / `detail` prop（Group / Section / Tag / 设置项）；开关 `preview` 走同一套 hover tooltip |
| **子分类标题开关**（模块启用） | **`SettingGroup` 的 `switch`**（右侧 `ToggleSwitch`；关闭时组内容弱化） |
| **流程图式配置步骤**（去哪里 / 做什么 / 外链或站内动作） | **`SetupFlow`**（数据平台二级页；步骤数据见 `config/platformSetupGuides.ts`） |
| ~~信息卡长说明~~ | **弃用 `InfoCard` 堆说明**；改走标题 `detail` tooltip |

### 信息卡 + 按钮

```tsx
import { InfoActionCard } from '../settings'

;<InfoActionCard
  fields={[
    { key: 'handle', label: 'Handle', value: '@me@example.com' },
    { key: 'actor', label: 'Actor', value: 'https://…', mono: true },
  ]}
  emptyText="暂无身份"
  empty={!identity}
  actions={[
    {
      key: 'rotate',
      label: '轮换密钥',
      onClick: rotate,
      loading: busy,
      variant: 'secondary',
      confirm: '确定轮换？',
    },
  ]}
/>
```

- `fields`：标签 / 值行（`mono` 用于 URL / key id）
- **复制**：字段默认可复制（字符串/`copyText`）；`copyable={false}` 或字段 `copyable={false}` 关闭
- `empty` + `emptyText`：空态
- `children`：自由正文（与 `fields` 二选一，`children` 优先）
- `actions`：底部按钮组（统一 `SettingsButton`）
- `tone`：`default` | `info` | `success` | `warn` | `danger` | `muted`
- `embedded`：嵌在列表展开区等宿主表面时去掉卡片描边/底，避免双层背景

### 统一间距

`.config-section` 定义令牌，**用 gap 排距，不要再叠 margin**：

| 令牌                    | 默认       | 用途                                 |
| ----------------------- | ---------- | ------------------------------------ |
| `--settings-gap-item`   | `0.625rem` | 组内设置项 / 控件堆叠                |
| `--settings-gap-group`  | `1.25rem`  | 子分类与内容块之间（`.config-form`） |
| `--settings-gap-header` | `0.75rem`  | 区块标题 → 内容                      |

组内非 `SettingItem` 的自定义堆叠请包一层 `settings-stack`。

### 标题旁标签

```tsx
<SettingGroup
  title={t.config.siteUrlConfig}
  titleExtra={
    <SettingTitleTag
      icon={<FaExchangeAlt />}
      title="更换域名、同步 CORS 请到高级配置"
      onClick={() => goAdvancedDomain()}
    >
      前往更换域名
    </SettingTitleTag>
  }
>
  …
</SettingGroup>
```

- 有 `onClick` → 可点 chip（会 `stopPropagation`，不干扰折叠标题）
- 无 `onClick` → 静态标签；`variant="muted"` 为中性样式

### 标题详细说明（tooltip）

```tsx
;<SettingGroup
  title="网络"
  description="配置出站代理与 API 镜像" // 默认仅 ⓘ hover 显示
  // detail="更长的说明…"              // 可选，优先于 description
  // descriptionVisible                 // 需要时再常显在标题下
>
  …
</SettingGroup>

{
  /* 设置项 */
}
;<SwitchItem
  label="启用代理"
  detail="较长帮助文案…"
  description="短说明仍可常显在标签下"
  /* ...其余属性 */
/>
```

- 所有标题组件统一：`SettingSection` / `SettingGroup` / `SettingTitleTag` / 设置项 `detail`
- 默认**不占版面**，标题旁 ⓘ，hover / focus 出 tooltip（支持链接等富文本）
- `detailTone="warning"` 用于阻断性提示（如 OAuth 未设站点 URL）
- **每个设置页右上角**：
  - **重置本页**（右上角：标题 + 介绍 + 危险按钮）：仅恢复当前页默认值并立即保存；`SettingsPageActionsProvider` 注入
  - **显示说明**（`SettingsHelpToggle` + `.glass`）：开 = 本页说明常显；关 = 仅 ⓘ 悬停

### 子分类标题开关

```tsx
<SettingGroup
  title="Agent 任务"
  description="任务进度、结果、取消与澄清请求"
  icon={<NotificationSourceIcon source="agent" />}
  switch={{
    checked: preferences.sources.agent,
    onChange: (v) => setSourceEnabled('agent', v),
    disabled: !masterEnabled,
    ariaLabel: '允许此来源',
    preview: {
      on: '开启后会推送此来源的通知',
      off: '关闭后不再推送此来源',
    },
  }}
>
  {/* 显示位置 / 事件等细项 */}
</SettingGroup>
```

- `switch` 渲染在标题行右侧（`ToggleSwitch`），点击不会触发展开/折叠
- `preview`：悬停/聚焦开关时预演开启样子；浮层与 ⓘ 共用 `useSettingHoverTooltip`（`on` / `off` / `disabled`）
- 关闭时组根节点带 `is-switch-off`，内容区弱化；子控件仍需自行传 `disabled`

### 多子分类网格（嵌套）

```tsx
{
  /* 默认 stretch：同一行两张卡外框同高、底边齐 */
}
;<SettingGroupGrid columns={2} variant="card" minColumnWidth="18rem">
  <SettingGroup title="Agent 任务" switch={{/* ... */}}>
    …
  </SettingGroup>
  <SettingGroup title="Heartbeat" switch={{/* ... */}}>
    …
  </SettingGroup>
</SettingGroupGrid>
```

- 嵌套多个 `SettingGroup` 子分类
- 宽屏最多 `columns` 列（默认 2）；按 `minColumnWidth` 自适应减列，≤640px 单列
- **`align="stretch"`（默认）**：同排卡片 **外框同高、顶底对齐**（grid stretch + 卡片 height:100%）
- `align="rows"`：同高 + CSS subgrid 内部区块跨列对齐（各卡结构行数需一致）
- `align="start"`：高度随内容

```tsx
{
  /* 单选 · 等分 4 列 */
}
;<SegmentedControl
  size="md"
  columns={4}
  value={freq}
  options={[/* ... */]}
  onChange={setFreq}
/>

{
  /* 多选 · 自动换行 */
}
;<SegmentedControl
  mode="multi"
  value={sources}
  options={[/* ... */]}
  onChange={setSources}
/>
```

**禁止**在设置页手写 `<button className="btn-base btn-…">`。统一：

```tsx
import { SettingsButton } from '../settings'

;<SettingsButton variant="primary" size="sm" loading={busy} onClick={onSave}>
  保存
</SettingsButton>
```

`variant`: `primary` | `secondary` | `danger` | `ghost` | `icon`

`size` 规格：

| size         | 用途                          | 形态                          |
| ------------ | ----------------------------- | ----------------------------- |
| **`sm`**     | 列表行、卡片头、密集工具栏    | 更矮、圆角矩形、约 1.65rem 高 |
| `md`（默认） | 设置行 `ButtonItem`、弹层确认 | 标准 `btn-base`               |
| `lg`         | Hero / 浮动主操作             | 更高、更大字号                |

```tsx
{
  /* 列表 / 密集区 */
}
;<SettingsButton size="sm" variant="danger">
  封禁
</SettingsButton>

{
  /* 常规设置操作 */
}
;<SettingsButton variant="primary">保存</SettingsButton>
```

`ManagedList` 只负责结构与样式；数据与乐观更新由调用方维护。可选 `stats` / `toolbar` / `search` / `filters`（`SegmentedControl`）/ **`form`（添加表单）** / `footer`。

**搜索与筛选（通用 chrome）**：有 `search` / `filters` / `filterGroups` 时由 `ManagedList` 统一渲染。`queryCollapsible={false}` 时始终展开、无切换/收起。`queryChrome="plain"` 无标题栏。工具栏顺序：`搜索与筛选`（可折叠时）→ 业务 `toolbar` → `添加表单`。`managed-list-top` 包住 stats + toolbar。

**行展开**：`item.expandContent` + `expanded` + `onToggleExpand`；主区域可点，详情区可嵌 `InfoActionCard` 等。

```tsx
{
  /* 列表 + 内嵌添加表单（默认折叠，点按钮展开） */
}
;<ManagedList
  stats={[{ key: 'total', label: '全部', value: n }]}
  items={rows}
  emptyText="暂无规则"
  maxHeight={null}
  formTitle="添加规则"
  formCollapseLabel="取消"
  formOpen={open}
  onFormOpenChange={setOpen}
  form={
    <>
      <InputItem /* ... */ />
      <SelectItem /* ... */ />
      <div className="managed-list-form-actions">
        <SettingsButton variant="primary" size="sm" onClick={add}>
          添加
        </SettingsButton>
      </div>
    </>
  }
/>
```

- `form`：调用方渲染字段与提交；`ManagedList` 只包一层统一表单 chrome
- **默认折叠**（`formDefaultOpen={false}`）；展开按钮文案用 `formTitle`
- `formOpen` / `onFormOpenChange`：可选受控（添加成功后自行收起）
- `formCollapseLabel`：展开态的收起按钮（默认 `Cancel`）
- `formPlacement`：`'before'`（默认）| `'after'`
- 表单内提交用 **`SettingsButton`**，动作行可包 `managed-list-form-actions`

**已接入的设置页列表**

| 区块                     | 文件                                                     | 能力                                                                               |
| ------------------------ | -------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| 联邦 · 出站投递队列      | `FederationDeliveryQueue.tsx`                            | stats + toolbar + 折叠 query                                                       |
| 联邦 · 已知实例          | `FederationConfigSection.tsx`                            | stats + 折叠 query                                                                 |
| 联邦 · 内容过滤          | `FederationConfigSection.tsx`                            | stats + 折叠 query + 折叠 form                                                     |
| 联邦 · 身份与密钥        | `FederationConfigSection.tsx`                            | **InfoActionCard**                                                                 |
| **用户管理**             | `UsersConfigSection.tsx`                                 | top：数值后统一 chip 区（公开注册 / 刷新 / 新建…）；plain query；折叠 form；行展开 |
| **权限 · elevated 下放** | `PermissionGroup` → `CheckboxGroupItem` → `CheckboxCard` |
| 关于 · 更新备份快照      | `UpdaterConfigSection.tsx`                               | stats + toolbar                                                                    |

**不适合迁入的设置页列表（保留专用 UI）**

| 区块            | 原因                                     |
| --------------- | ---------------------------------------- |
| 用户管理        | 展开详情 + 多段表单 / 身份解绑           |
| OAuth providers | 可展开编辑的凭证卡片                     |
| 通知来源网格    | 每源一整组 Switch/Checkbox，非行操作队列 |
| 更新器目标选择  | 单选列表，不是 CRUD 管理                 |

## 原则

1. **一致性**：视觉与交互统一（hover / focus / dark）。
2. **按钮统一**：危险/主行动作用 `SettingsButton` / `ButtonItem`；list chrome 小芯片用 `CheckboxCard` `variant="action"`。
3. **选择统一**：单选/多选分段一律 `SegmentedControl`（轨道 + 浮起选中）；不要手写 button 组。
4. **无原生 option 列表**：系统 `option` 弹层几乎不可样式化；统一 `FieldSelect`。
5. **开关复用**：不要手写 `.toggle-switch` DOM，用 `ToggleSwitch`。
6. **可访问性**：`aria-label` / `htmlFor` / 键盘 Escape 关菜单。

仍可保留领域 DOM 的选择：更新通道大卡、OAuth preset 卡、用户行展开等非紧凑 chip/segmented 场景。

## 示例

```tsx
import {
  FieldSelect,
  InputItem,
  SettingGroup,
  SettingSection,
  SwitchItem,
  ToggleSwitch,
} from '../settings'

;<SettingSection title="示例" sectionId="demo">
  <SettingGroup title="基础">
    <SwitchItem
      itemKey="enabled"
      label="启用"
      value={enabled}
      onChange={setEnabled}
    />
    <InputItem
      itemKey="name"
      label="名称"
      value={name}
      onChange={setName}
      layout="vertical"
    />
    <FieldSelect
      value={interval}
      options={[
        { value: '3600', label: '每小时' },
        { value: '86400', label: '每天' },
      ]}
      onChange={setInterval}
    />
  </SettingGroup>
</SettingSection>

{
  /* 紧凑开关（平台卡 / OAuth 头） */
}
;<ToggleSwitch
  checked={on}
  onChange={setOn}
  aria-label="启用"
  preview={{
    on: '开启后会在报告页显示这张卡片',
    off: '关闭后报告页不再显示，数据仍保留',
    disabled: '先填好凭证才能打开',
  }}
/>
```

## 已迁移的设置区块

见 `components/config/`：`Music`、`Network`、`OAuth`、`UI`、`Permissions`、`Module`、`Users`、`Notification`、`Updater` 等；`ConfigForm` 平台卡/平台弹层也使用 `ToggleSwitch` / `InputItem`。
