/**
 * 设置组件统一导出
 */

// 类型导出
export type {
  SettingType,
  SettingSize,
  SettingLayout,
  SettingOption,
  BaseSettingItemConfig,
  SwitchSettingConfig,
  CheckboxSettingConfig,
  InputSettingConfig,
  NumberSettingConfig,
  SelectSettingConfig,
  ProviderSettingConfig,
  SliderSettingConfig,
  ButtonSettingConfig,
  CustomSettingConfig,
  SettingItemConfig,
  SettingGroupConfig,
  SettingSectionConfig,
  PermissionItem,
  PermissionGroupConfig,
  QuotaItem,
  QuotaGroupConfig,
  InfoCardConfig,
} from './types';

// 核心组件
export { SettingItem } from './SettingItem';
export { SettingGroup } from './SettingGroup';
export { SettingSection } from './SettingSection';
export { InfoCard } from './InfoCard';

// 具体设置项组件
export { SwitchItem } from './items/SwitchItem';
export { InputItem } from './items/InputItem';
export { NumberItem } from './items/NumberItem';
export { SelectItem } from './items/SelectItem';
export { ProviderItem } from './items/ProviderItem';
export { ButtonItem } from './items/ButtonItem';
export { CheckboxItem } from './items/CheckboxItem';

// 预设组合组件
export { PermissionGroup, QuotaGroup } from './presets';
export { CompactSettingGroup } from './CompactSettingGroup';
