// API URL 配置：
// - 开发环境：默认使用相对路径 /api/*，由 Astro dev proxy 转发到后端
// - 生产环境（.env.production）：PUBLIC_API_URL="" (使用相对路径 /api/*)
//   相对路径会自动使用当前域名，符合同源策略和 CSP 要求
const configuredApiUrl = (import.meta.env?.PUBLIC_API_URL || '').trim()
export const API_URL = configuredApiUrl
