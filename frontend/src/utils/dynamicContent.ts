/**
 * 动态内容岛工具
 * 提供天气、问候语、一言等动态内容
 *
 * 注意：天气和一言功能已拆分到独立文件
 */

import { currentCopy } from '../i18n/localeCopy'

export * from './quote'
// 重新导出类型和函数，保持向后兼容
export * from './weather'

export type GreetingIconName =
  'sunrise' | 'sun' | 'cloud-sun' | 'sunset' | 'moon'
export type ThemeIconName = 'sun' | 'moon'

export interface GreetingData {
  text: string
  icon: GreetingIconName
  time: string
}

export interface GreetingTranslations {
  morning: string
  forenoon: string
  noon: string
  afternoon: string
  dusk: string
  evening: string
  night: string
}

/**
 * 获取时间段问候语
 * @param username 用户名（可选）
 * @param translations 翻译对象（可选，用于国际化）
 * @param locale 语言代码（可选，用于时间格式化）
 */
export function getGreeting(
  username?: string,
  translations?: GreetingTranslations,
  locale?: string,
): GreetingData {
  const hour = new Date().getHours()
  const time = new Date().toLocaleTimeString(locale || 'zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
  })

  let text = ''
  let icon: GreetingIconName = 'sun'

  const g = currentCopy().greeting
  const t = translations || {
    morning: g.morning,
    forenoon: g.forenoon,
    noon: g.noon,
    afternoon: g.afternoon,
    dusk: g.dusk,
    evening: g.evening,
    night: g.night,
  }

  // 细分时间段：图标跟随太阳实际状态
  // 日出仅清晨显示，临近正午用太阳；日落仅傍晚显示，入夜后用月亮
  if (hour >= 5 && hour < 8) {
    icon = 'sunrise'
    text = t.morning
  } else if (hour >= 8 && hour < 11) {
    icon = 'sun'
    text = t.forenoon
  } else if (hour >= 11 && hour < 13) {
    icon = 'sun'
    text = t.noon
  } else if (hour >= 13 && hour < 17) {
    icon = 'cloud-sun'
    text = t.afternoon
  } else if (hour >= 17 && hour < 19) {
    icon = 'sunset'
    text = t.dusk
  } else if (hour >= 19 && hour < 22) {
    icon = 'moon'
    text = t.evening
  } else {
    icon = 'moon'
    text = t.night
  }

  if (username) {
    text += locale?.startsWith('zh') ? `，${username}` : `, ${username}`
  }

  return { text, icon, time }
}

/**
 * 获取主题状态信息
 * @param translations 翻译对象（可选）
 */
export function getThemeInfo(translations?: { dark: string; light: string }): {
  text: string
  icon: ThemeIconName
} {
  const isDark = document.documentElement.classList.contains('dark')
  const panel = currentCopy().controlPanel
  const t = translations || { dark: panel.dark, light: panel.light }
  return {
    text: isDark ? t.dark : t.light,
    icon: isDark ? 'moon' : 'sun',
  }
}
