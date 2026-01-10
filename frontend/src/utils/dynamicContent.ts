/**
 * 动态内容岛工具
 * 提供天气、问候语、一言等动态内容
 *
 * 注意：天气和一言功能已拆分到独立文件
 */

export * from './quote'
// 重新导出类型和函数，保持向后兼容
export * from './weather'

export interface GreetingData {
  text: string
  icon: string
  time: string
}

export interface GreetingTranslations {
  morning: string
  noon: string
  afternoon: string
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
  let icon = ''

  // 默认中文翻译
  const t = translations || {
    morning: '早上好',
    noon: '中午好',
    afternoon: '下午好',
    evening: '晚上好',
    night: '夜深了',
  }

  if (hour >= 5 && hour < 12) {
    icon = '🌅'
    text = t.morning
  }
  else if (hour >= 12 && hour < 14) {
    icon = '☀️'
    text = t.noon
  }
  else if (hour >= 14 && hour < 18) {
    icon = '🌤️'
    text = t.afternoon
  }
  else if (hour >= 18 && hour < 22) {
    icon = '🌆'
    text = t.evening
  }
  else {
    icon = '🌙'
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
export function getThemeInfo(translations?: { dark: string, light: string }): { text: string, icon: string } {
  const isDark = document.documentElement.classList.contains('dark')
  const t = translations || { dark: '深色模式', light: '浅色模式' }
  return {
    text: isDark ? t.dark : t.light,
    icon: isDark ? '🌙' : '☀️',
  }
}
