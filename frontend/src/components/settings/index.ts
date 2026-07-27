/**
 * 设置组件统一导出
 */

export { CompactSettingGroup } from './CompactSettingGroup'

export { InfoCard } from './InfoCard'
export { ButtonItem } from './items/ButtonItem'
export { CheckboxGroupItem } from './items/CheckboxGroupItem'
export { CheckboxItem } from './items/CheckboxItem'
export { FieldSelect } from './items/FieldSelect'
export { InputItem } from './items/InputItem'

export { NumberGroupItem } from './items/NumberGroupItem'
export { NumberItem } from './items/NumberItem'
export { ProviderItem } from './items/ProviderItem'
export { SelectItem } from './items/SelectItem'
// 具体设置项组件
export { SwitchItem } from './items/SwitchItem'
export { ToggleSwitch } from './items/ToggleSwitch'
// 预设组合组件
export { PermissionGroup, QuotaGroup } from './presets'
export { SettingGroup } from './SettingGroup'
// 核心组件
export { SettingItem } from './SettingItem'

export { SettingSection } from './SettingSection'
// 类型导出
export type {
  BaseSettingItemConfig,
  ButtonSettingConfig,
  CheckboxSettingConfig,
  CustomSettingConfig,
  InfoCardConfig,
  InputSettingConfig,
  NumberSettingConfig,
  PermissionGroupConfig,
  PermissionItem,
  ProviderSettingConfig,
  QuotaGroupConfig,
  QuotaItem,
  SelectSettingConfig,
  SettingGroupConfig,
  SettingItemConfig,
  SettingLayout,
  SettingOption,
  SettingSectionConfig,
  SettingSize,
  SettingType,
  SwitchSettingConfig,
} from './types'
