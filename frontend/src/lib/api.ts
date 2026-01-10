import type { AxiosError } from 'axios'
import axios from 'axios'
import { clearCSRFToken, getCSRFHeaderName, getCSRFToken } from '../utils/csrf'
import { checkRateLimit, RateLimitError } from '../utils/rateLimiter'
import TokenManager from '../utils/tokenManager'

// 智能 API URL 检测（与 config.ts 保持一致）
// 生产环境使用相对路径（空字符串），开发环境使用 localhost
const API_BASE_URL = (import.meta.env.PUBLIC_API_URL || '').trim()
  || (typeof window !== 'undefined' ? window.location.origin : '')

// 验证 API URL 格式
function isValidUrl(url: string): boolean {
  // 空字符串是有效的（表示使用相对路径）
  if (url === '') {
    return true
  }
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  }
  catch {
    return false
  }
}

if (!isValidUrl(API_BASE_URL)) {
  throw new Error('Invalid API_BASE_URL configuration')
}

const api = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
  },
  timeout: 30000, // 30秒超时
  validateStatus: status => status < 500, // 只有5xx才算网络错误
  withCredentials: true, // ✅ 自动发送 HttpOnly Cookie
})

// 验证 JWT token 格式
function isValidToken(token: string): boolean {
  if (!token || typeof token !== 'string')
    return false
  const parts = token.split('.')
  return parts.length === 3 && parts.every(part => part.length > 0)
}

// Add request interceptor to include auth token
api.interceptors.request.use(
  async (config) => {
    // ✅ 安全修复 P0: 异步获取 CSRF Token（从服务器）
    const csrfToken = await getCSRFToken()
    if (csrfToken) {
      config.headers[getCSRFHeaderName()] = csrfToken
    }

    // ⚠️ HttpOnly Cookie 用于身份验证（自动发送，无需手动添加）
    // TokenManager.getToken() 返回 null（HttpOnly Cookie 无法被 JS 读取）
    // Axios 通过 withCredentials: true 自动发送 Cookie

    // Rate Limiting 检查（仅针对修改操作）
    if (config.method && ['post', 'put', 'patch', 'delete'].includes(config.method.toLowerCase())) {
      const endpoint = config.url || ''

      if (endpoint.includes('/auth/login')) {
        if (!checkRateLimit(endpoint, 'login')) {
          return Promise.reject(new RateLimitError('登录尝试过于频繁，请稍后再试', 300000))
        }
      }
      else if (endpoint.includes('/fetch')) {
        if (!checkRateLimit(endpoint, 'fetch')) {
          return Promise.reject(new RateLimitError('数据获取请求过于频繁，请稍后再试', 60000))
        }
      }
      else if (endpoint.includes('/analysis')) {
        if (!checkRateLimit(endpoint, 'analysis')) {
          return Promise.reject(new RateLimitError('分析请求过于频繁，请稍后再试', 60000))
        }
      }
      else {
        if (!checkRateLimit(endpoint, 'api')) {
          return Promise.reject(new RateLimitError('请求过于频繁，请稍后再试', 60000))
        }
      }
    }

    return config
  },
  (error) => {
    return Promise.reject(error)
  },
)

// Add response interceptor to handle 401 errors
api.interceptors.response.use(
  response => response,
  (error: AxiosError | RateLimitError) => {
    // 处理 Rate Limit 错误
    if (error instanceof RateLimitError) {
      return Promise.reject(error)
    }

    // 处理 Axios 错误
    if (error.response?.status === 401) {
      // Token expired or invalid, clear it and redirect to login
      TokenManager.removeToken()
      clearCSRFToken()
      window.dispatchEvent(new CustomEvent('auth-state-changed', {
        detail: { isAuthenticated: false },
      }))
    }

    // 处理 429 Too Many Requests
    if (error.response?.status === 429) {
      const retryAfter = error.response.headers['retry-after']
      const message = `请求过于频繁，请在 ${retryAfter || 60} 秒后重试`
      return Promise.reject(new RateLimitError(message, Number.parseInt(retryAfter || '60000')))
    }

    return Promise.reject(error)
  },
)

// Health check
export async function checkHealth() {
  const response = await api.get('/health')
  return response.data
}

// Setup APIs
export async function checkSetupStatus() {
  const response = await api.get('/api/setup/status')
  return response.data
}

export async function saveDatabaseConfig(config: {
  host: string
  port: number
  username: string
  password: string
  database: string
}) {
  // 输入验证
  if (!config.host || config.host.length > 255) {
    throw new Error('Invalid host')
  }
  if (config.port < 1 || config.port > 65535) {
    throw new Error('Invalid port')
  }
  if (!config.username || config.username.length > 100) {
    throw new Error('Invalid username')
  }
  if (!config.password || config.password.length > 255) {
    throw new Error('Invalid password')
  }
  if (!config.database || config.database.length > 100) {
    throw new Error('Invalid database name')
  }

  const response = await api.post('/api/setup/database-config', config)
  return response.data
}

export async function initDatabase() {
  const response = await api.post('/api/setup/init-database')
  return response.data
}

export async function createAdmin(credentials: {
  username: string
  password: string
}) {
  // 验证用户名
  if (!credentials.username || credentials.username.length < 3 || credentials.username.length > 50) {
    throw new Error('Username must be 3-50 characters')
  }
  if (!/^\w+$/.test(credentials.username)) {
    throw new Error('Username can only contain letters, numbers and underscores')
  }

  // 验证密码
  if (!credentials.password || credentials.password.length < 8 || credentials.password.length > 128) {
    throw new Error('Password must be 8-128 characters')
  }

  const response = await api.post('/api/setup/create-admin', credentials)
  return response.data
}

// Configuration
export async function fetchConfig() {
  const response = await api.get('/api/config')
  return response.data
}

export async function updateConfig(config: any) {
  const response = await api.post('/api/config', config)
  return response.data
}

// Platforms
export async function fetchPlatforms() {
  const response = await api.get('/api/platforms')
  return response.data
}

export async function fetchProfiles() {
  const response = await api.get('/api/profiles')
  return response.data
}

export async function triggerFetch() {
  const response = await api.post('/api/fetch')
  return response.data
}

// Analysis
export async function fetchAnalysis() {
  const response = await api.get('/api/analysis')
  return response.data
}

export async function triggerAnalysis() {
  const response = await api.post('/api/analysis')
  return response.data
}

// Config Permissions
export async function fetchPermissionsConfig() {
  const response = await api.get('/api/config/permissions')
  return response.data
}

export async function updatePermissionsConfig(permissions: Record<string, boolean | number>) {
  const response = await api.post('/api/config/permissions', permissions)
  return response.data
}

// System
export async function reloadSystemConfig() {
  const response = await api.post('/api/system/reload-config')
  return response.data
}

export async function testPlatformConfig(platform: string, config: any) {
  const response = await api.post('/api/config/test', { platform, config })
  return response.data
}

// Speech
export async function checkSpeechStatus() {
  const response = await api.get('/api/speech/status')
  return response.data
}

export default api
