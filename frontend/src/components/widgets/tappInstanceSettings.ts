import type { TappSettingItem } from '@myriad/tapp-runtime'

/** 按 widget 声明的设置项校验一次实例设置补丁。任何一项不合声明就整包拒绝。 */
export function acceptTappInstanceSettingsPatch(
  declarations: readonly TappSettingItem[],
  patch: Record<string, unknown>,
): Record<string, unknown> | null {
  const byKey = new Map(declarations.map((setting) => [setting.key, setting]))
  const accepted: Record<string, unknown> = {}

  for (const [key, value] of Object.entries(patch)) {
    const setting = byKey.get(key)
    if (!setting) return null
    if (setting.type === 'toggle' && typeof value !== 'boolean') return null
    if (
      (setting.type === 'input' || setting.type === 'color') &&
      typeof value !== 'string'
    ) {
      return null
    }
    if (
      setting.type === 'select' &&
      (typeof value !== 'string' ||
        !setting.options?.some((option) => option.value === value))
    ) {
      return null
    }
    if (setting.type === 'number') {
      if (typeof value !== 'number' || !Number.isFinite(value)) return null
      if (setting.min !== undefined && value < setting.min) return null
      if (setting.max !== undefined && value > setting.max) return null
    }
    accepted[key] = value
  }
  return accepted
}
