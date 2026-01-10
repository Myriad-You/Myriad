export const SITE = {
  title: 'Myriad',
  description: 'Multi-platform personal information aggregation and analysis platform',
  defaultLanguage: 'en-us',
} as const

// API URL 配置：
// - 开发环境（.env.example）：PUBLIC_API_URL=http://localhost:3000
// - 生产环境（.env.production）：PUBLIC_API_URL="" (使用相对路径 /api/*)
//   相对路径会自动使用当前域名，符合同源策略和 CSP 要求
export const API_URL = (import.meta.env.PUBLIC_API_URL || '').trim() || ''
