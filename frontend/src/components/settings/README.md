# 通用设置组件架构

设置页统一使用本目录下的组件，避免各区块手写原生 `select` / `checkbox` / toggle DOM。

## 目录

```
settings/
├── types.ts                 # 类型定义
├── SettingItem.tsx          # 按 type 分发的工厂组件
├── SettingGroup.tsx         # 分组容器
├── SettingSection.tsx       # 带标题/图标的区块
├── CompactSettingGroup.tsx
├── InfoCard.tsx
├── items/
│   ├── ToggleSwitch.tsx     # 统一开关控件（无 label 壳）
│   ├── SwitchItem.tsx       # 开关设置项
│   ├── InputItem.tsx        # 文本输入
│   ├── NumberItem.tsx       # 数字输入
│   ├── SelectItem.tsx       # 下拉（内部用 FieldSelect）
│   ├── FieldSelect.tsx      # 自定义 listbox（替代原生 select）
│   ├── CheckboxItem.tsx
│   ├── CheckboxGroupItem.tsx
│   ├── NumberGroupItem.tsx
│   ├── ProviderItem.tsx
│   ├── ButtonItem.tsx
│   └── SettingItem.css      # 含 toggle-switch 全局样式
├── presets/
│   ├── PermissionGroup.tsx
│   └── QuotaGroup.tsx
└── index.ts
```

## 何时用哪个

| 场景 | 组件 |
| ---- | ---- |
| 带标签的开关设置行 | `SwitchItem` |
| 卡片头/紧凑行内开关（无整行 label 壳） | `ToggleSwitch` |
| 文本 / 密码 / URL / email | `InputItem` |
| 数字 + 单位 | `NumberItem` |
| 下拉（必须用自定义 listbox，勿用原生 `<select>`） | `FieldSelect` 或 `SelectItem` |
| 多选芯片组 | `CheckboxGroupItem` |
| 区块标题 | `SettingSection` + `SettingGroup` |

## 原则

1. **一致性**：视觉与交互统一（hover / focus / dark）。
2. **无原生 option 列表**：系统 `option` 弹层几乎不可样式化；统一 `FieldSelect`。
3. **开关复用**：不要手写 `.toggle-switch` DOM，用 `ToggleSwitch`。
4. **可访问性**：`aria-label` / `htmlFor` / 键盘 Escape 关菜单。

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

<SettingSection title="示例" sectionId="demo">
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

{/* 紧凑开关（平台卡 / OAuth 头） */}
<ToggleSwitch
  checked={on}
  onChange={setOn}
  aria-label="启用"
/>
```

## 已迁移的设置区块

见 `components/config/`：`Music`、`Network`、`OAuth`、`UI`、`Permissions`、`Module`、`Users`、`Notification`、`Updater` 等；`ConfigForm` 平台卡/平台弹层也使用 `ToggleSwitch` / `InputItem`。
