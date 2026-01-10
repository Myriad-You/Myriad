/**
 * 输入清洗和验证工具
 * 防止 XSS、SQL 注入等攻击
 */

/**
 * HTML 实体编码
 */
export function escapeHtml(unsafe: string): string {
  return unsafe
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;')
}

/**
 * 移除 HTML 标签
 */
export function stripHtmlTags(html: string): string {
  return html.replace(/<[^>]*>/g, '')
}

/**
 * 清洗用户名输入
 */
export function sanitizeUsername(username: string): string {
  // 只保留字母、数字、下划线
  return username.replace(/\W/g, '').slice(0, 50)
}

/**
 * 清洗邮箱输入
 */
export function sanitizeEmail(email: string): string {
  return email.trim().toLowerCase().slice(0, 255)
}

/**
 * 验证邮箱格式
 */
export function isValidEmail(email: string): boolean {
  const emailRegex = /^[\w.%+-]+@[a-z0-9.-]+\.[a-z]{2,}$/i
  return emailRegex.test(email) && email.length <= 255
}

/**
 * 验证用户名格式
 */
export function isValidUsername(username: string): boolean {
  const usernameRegex = /^\w{3,50}$/
  return usernameRegex.test(username)
}

/**
 * 验证密码强度
 */
export interface PasswordStrength {
  isValid: boolean
  score: number // 0-4
  feedback: string[]
}

export function checkPasswordStrength(password: string): PasswordStrength {
  const feedback: string[] = []
  let score = 0

  // 长度检查
  if (password.length < 8) {
    feedback.push('密码至少需要8个字符')
    return { isValid: false, score: 0, feedback }
  }
  if (password.length >= 12)
    score++
  if (password.length >= 16)
    score++

  // 复杂度检查
  if (/[a-z]/.test(password))
    score++
  if (/[A-Z]/.test(password))
    score++
  if (/\d/.test(password))
    score++
  if (/[^a-z0-9]/i.test(password))
    score++

  // 反馈
  if (!/[a-z]/.test(password))
    feedback.push('建议包含小写字母')
  if (!/[A-Z]/.test(password))
    feedback.push('建议包含大写字母')
  if (!/\d/.test(password))
    feedback.push('建议包含数字')
  if (!/[^a-z0-9]/i.test(password))
    feedback.push('建议包含特殊字符')

  // 常见弱密码检查
  const commonPasswords = ['password', '12345678', 'qwerty', 'admin', 'letmein']
  if (commonPasswords.some(weak => password.toLowerCase().includes(weak))) {
    feedback.push('密码过于常见，请使用更复杂的密码')
    score = Math.max(0, score - 2)
  }

  return {
    isValid: password.length >= 8 && password.length <= 128,
    score: Math.min(4, score),
    feedback,
  }
}

/**
 * 清洗 URL 输入
 */
export function sanitizeUrl(url: string): string {
  try {
    const parsed = new URL(url)
    // 只允许 http 和 https 协议
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error('Invalid protocol')
    }
    return parsed.toString()
  }
  catch {
    return ''
  }
}

/**
 * 验证并清洗整数输入
 */
export function sanitizeInteger(value: any, min?: number, max?: number): number | null {
  const num = Number.parseInt(value, 10)
  if (isNaN(num))
    return null
  if (min !== undefined && num < min)
    return null
  if (max !== undefined && num > max)
    return null
  return num
}

/**
 * 验证并清洗文本输入
 */
export function sanitizeText(text: string, maxLength: number = 1000): string {
  return stripHtmlTags(text)
    .trim()
    .slice(0, maxLength)
}

/**
 * 防止路径遍历攻击
 */
export function sanitizePath(path: string): string {
  return path
    .replace(/\.\./g, '') // 移除 ..
    .replace(/\/\//g, '/') // 移除双斜杠
    .replace(/^\//, '') // 移除开头斜杠
}

/**
 * 验证文件名
 */
export function isValidFilename(filename: string): boolean {
  // 不允许路径分隔符和特殊字符
  const invalidChars = /[<>:"/\\|?*\x00-\x1F]/
  return !invalidChars.test(filename) && filename.length > 0 && filename.length <= 255
}

/**
 * 清洗文件名
 */
export function sanitizeFilename(filename: string): string {
  return filename
    .replace(/[<>:"/\\|?*\x00-\x1F]/g, '_')
    .slice(0, 255)
}
