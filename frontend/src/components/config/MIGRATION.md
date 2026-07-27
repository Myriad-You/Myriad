# Config 区块与设置原语

`ConfigForm` 的各配置区块已拆到 `components/config/*ConfigSection.tsx`，并优先复用 `components/settings/` 统一原语。

## 设置原语（必读）

详见 [`../settings/README.md`](../settings/README.md)。

| 场景 | 使用 |
| ---- | ---- |
| 带标签的开关行 | `SwitchItem` |
| 卡片头 / 紧凑开关 | `ToggleSwitch` |
| 文本 / 密码 / URL / email | `InputItem` |
| 数字 + 单位 | `NumberItem` |
| 下拉 | `FieldSelect` 或 `SelectItem`（禁止原生 `<select>`） |
| 多选芯片 | `CheckboxGroupItem` |
| 区块结构 | `SettingSection` + `SettingGroup` |

## 已接入的配置区块

| 组件 | 区块 |
| ---- | ---- |
| `MusicConfigSection` | 音乐 |
| `NetworkConfigSection` | 网络代理 |
| `OAuthConfigSection` | OAuth |
| `UiConfigSection` | UI |
| `PermissionsConfigSection` | 权限 |
| `ModuleConfigSection` | 模块 / 报告 / 一言 |
| `UsersConfigSection` | 用户管理 |
| `NotificationConfigSection` | 通知 |
| `UpdaterConfigSection` | 更新器 |
| `AiConfigSection` | AI |
| `FederationConfigSection` | 联邦 |
| `AdvancedConfigSection` | 导入导出等 |
| `AboutConfigSection` | 关于 |

`ConfigForm` 本身还负责：平台卡片开关（`ToggleSwitch`）、平台配置弹层（`InputItem`）、导航搜索等壳层 UI。

## 新增设置时的约定

1. 不要手写 `.toggle-switch` DOM 或原生 `<select>` option 列表。
2. 深色对比与 focus 样式跟 `settings/items/*.css` 走。
3. 领域专用控件（如来源 chip、更新通道卡）可以保留自定义 DOM，但开关/输入/下拉仍优先原语。
