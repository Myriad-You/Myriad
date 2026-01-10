/**
 * ControlIsland 模式组件共享常量
 */

// Framer Motion transition 配置常量
export const SPRING_SNAPPY = { type: 'spring', stiffness: 400, damping: 25 } as const
export const SPRING_SMOOTH = { type: 'spring', stiffness: 350, damping: 28 } as const
export const TRANSITION_QUICK = { duration: 0.12 } as const
export const TRANSITION_NORMAL = { duration: 0.15 } as const
export const TRANSITION_SLOW = { duration: 0.25, ease: 'easeOut' } as const

// API URL
export const API_URL = import.meta.env.PUBLIC_API_URL || ''

/**
 * 处理图标 URL - 确保正确的完整路径
 */
export function getIconUrl(iconUrl: string | null | undefined): string | null {
  if (!iconUrl)
    return null
  if (iconUrl.startsWith('/api/')) {
    return `${API_URL}${iconUrl}`
  }
  if (iconUrl.startsWith('http://') || iconUrl.startsWith('https://')) {
    return `${API_URL}/api/proxy/image?url=${encodeURIComponent(iconUrl)}`
  }
  return iconUrl
}

/**
 * 判断是否为 base64 图片数据
 */
export function isBase64Image(str: string | null): boolean {
  if (!str)
    return false
  return str.startsWith('data:image/')
}

/**
 * 从 base64 提取 MIME 类型和扩展名
 */
export function getBase64Info(base64: string): { mime: string, ext: string } {
  const match = base64.match(/^data:(image\/\w+);base64,/)
  if (match) {
    const mime = match[1]
    const ext = mime.split('/')[1] || 'png'
    return { mime, ext }
  }
  return { mime: 'image/png', ext: 'png' }
}

// 排序选项定义
export const SORT_OPTIONS = [
  { value: 'update', labelKey: 'sortByUpdate' },
  { value: 'custom', labelKey: 'sortByCustom' },
  { value: 'category', labelKey: 'sortByCategory' },
  { value: 'random', labelKey: 'sortByRandom' },
  { value: 'pinyin', labelKey: 'sortByPinyin' },
] as const
