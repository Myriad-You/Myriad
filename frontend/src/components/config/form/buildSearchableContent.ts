import type { Locale } from '../../../i18n'
import type { ConfigSearchableItem } from '../../settings/guides/configSearch'
import type { Config } from './types'
import { buildGuideSearchIndex } from '../../settings/guides/guideSearchIndex'

/** i18n 切片：只取 build 搜索索引需要的文案字段 */
export interface ConfigSearchI18n {
  config: {
    platforms: string
    platformsDesc: string
    connectedPlatforms: string
    connectedPlatformsDesc: string
    data: string
    dataDesc: string
    ai: string
    aiDesc: string
    agentHeartbeatTitle: string
    agentSkillsTitle: string
    agentMemoryTitle: string
    tripo: string
    tripoDesc: string
    basic: string
    basicDesc: string
    oauth: string
    oauthDesc: string
    music: string
    musicDesc: string
    network: string
    networkDesc: string
    advanced: string
    advancedDesc: string
    mcpTitle: string
    mcpDesc: string
    about: string
    aboutDesc: string
    permissions: string
    permissionsDesc: string
    users: string
    usersDesc: string
    federation: string
    federationDesc: string
    moduleSettings: string
    moduleSettingsDesc: string
    analytics: {
      visitorTitle: string
      visitorDesc: string
      aiUsageTitle: string
      aiUsageDesc: string
    }
    thirdPartyAnalytics: string
    thirdPartyAnalyticsDesc: string
  }
  notificationCenter: {
    title: string
    settingsDesc: string
  }
}

/**
 * 构建设置页搜索索引（section / platform / alias / guide）。
 * 平台名从 config.platforms 动态并入，避免手工列表漂移。
 */
export function buildSearchableContent(
  config: Config | null,
  t: ConfigSearchI18n,
  locale: Locale,
  options?: { isAdmin?: boolean },
): ConfigSearchableItem[] {
  if (!config) return []
  const isAdmin = options?.isAdmin !== false // default include; pass false to filter

  const items: ConfigSearchableItem[] = []

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.platforms,
    description: t.config.platformsDesc,
    keywords: [
      '数据及统计',
      '数据页',
      '平台',
      '接入平台',
      '数据源',
      'token',
      'api',
      '访客',
      '访问',
      '统计',
      'analytics',
      'visitor',
      '数据管理',
      '缓存',
      '刷新',
      ...config.platforms.flatMap((p) => {
        const n = p.name.trim()
        return n ? [n, n.toLowerCase()] : []
      }),
    ],
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.connectedPlatforms,
    description: t.config.connectedPlatformsDesc,
    keywords: [
      '接入平台',
      '数据平台',
      '自动刷新',
      '刷新频率',
      '平台',
      '数据源',
      'connected',
      'platforms',
    ],
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.analytics.visitorTitle,
    description: t.config.analytics.visitorDesc,
    keywords: [
      '访客',
      '统计',
      'PV',
      'UV',
      'visitor',
      'analytics',
      '页面',
      '访问分析',
      'pageview',
      '事件',
      '来源',
      'referrer',
      '开关',
      '启用',
      'analytics_enabled',
      'opt-out',
      'optout',
      '退出',
      '隐私',
    ],
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.analytics.aiUsageTitle,
    description: t.config.analytics.aiUsageDesc,
    keywords: [
      'ai',
      'usage',
      'token',
      '用量',
      '使用量',
      '模型',
      'model',
      '调用',
      'ledger',
      'ai-usage',
      'ai_usage',
    ],
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.thirdPartyAnalytics,
    description: t.config.thirdPartyAnalyticsDesc,
    keywords: [
      '第三方',
      'third-party',
      '第三方统计',
      'ga',
      'ga4',
      'google analytics',
      'gtag',
      'umami',
      'ga_measurement_id',
      'umami_website_id',
      'umami_script_url',
      '外部统计',
      'analytics script',
    ],
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.data,
    description: t.config.dataDesc,
    keywords: ['数据管理', 'data', '缓存', 'cache', '过滤', '智能过滤', '刷新'],
  })

  config.platforms.forEach((platform) => {
    items.push({
      type: 'platform',
      section: 'platforms',
      title: platform.name,
      description: platform.description,
      keywords: [platform.name.toLowerCase(), '平台', '数据源', 'token', 'api'],
    })
  })

  items.push({
    type: 'section',
    section: 'ai',
    title: t.config.ai,
    description: t.config.aiDesc,
    keywords: [
      'ai',
      'gemini',
      'openai',
      'api',
      '模型',
      '智能',
      '图片',
      '生成',
      'image',
      'agent',
      '定时',
      '定时任务',
      '技能',
      '记忆',
      'heartbeat',
      'skills',
      'memory',
      t.config.agentHeartbeatTitle,
      t.config.agentSkillsTitle,
      t.config.agentMemoryTitle,
    ],
  })

  items.push({
    type: 'section',
    section: 'tripo',
    title: t.config.tripo,
    description: t.config.tripoDesc,
    keywords: [
      'tripo',
      '3d',
      'glb',
      'gltf',
      'low poly',
      'rig',
      'animation',
      '低模',
      '骨骼',
      '动作',
      '模型',
    ],
  })

  items.push({
    type: 'section',
    section: 'basic',
    title: t.config.basic,
    description: t.config.basicDesc,
    keywords: [
      'basic',
      '基础',
      'ui',
      '站点',
      '主题',
      '背景',
      '样式',
      'theme',
      'url',
      'domain',
      '域名',
      '更换域名',
      'base_url',
      'cors',
      'origin',
      'pwa',
      'service worker',
      '安装',
      'install',
      '离线',
      'offline',
      'pwa_enabled',
    ],
  })

  items.push({
    type: 'section',
    section: 'oauth',
    title: t.config.oauth,
    description: t.config.oauthDesc,
    keywords: ['oauth', 'github', '登录', 'auth', '认证'],
  })

  items.push({
    type: 'alias',
    section: 'modules',
    title: t.config.music,
    description: t.config.musicDesc,
    keywords: ['音乐', 'music', '歌单', '播放器', '网易云', 'qq音乐'],
  })

  items.push({
    type: 'alias',
    section: 'advanced',
    title: t.config.network,
    description: t.config.networkDesc,
    keywords: [
      'proxy',
      '代理',
      '网络',
      'gemini',
      'github',
      'api',
      '镜像',
      'mirror',
      'socks',
      'network',
    ],
  })

  items.push({
    type: 'section',
    section: 'notifications',
    title: t.notificationCenter.title,
    description: t.notificationCenter.settingsDesc,
    keywords: [
      'notification',
      '通知',
      '提醒',
      'toast',
      'browser',
      'arael',
      'brew',
      'tapp',
      'mcp',
      'aro',
    ],
  })

  items.push({
    type: 'section',
    section: 'advanced',
    title: t.config.advanced,
    description: t.config.advancedDesc,
    keywords: [
      'advanced',
      '高级',
      'danger',
      'reset',
      '重置',
      '危险',
      'proxy',
      '代理',
      '导入',
      '导出',
      '运行',
      '诊断',
      'health',
      'database',
      'storage',
      'task',
      'mcp',
      'model context protocol',
      '工具服务器',
      'tool server',
    ],
  })

  items.push({
    type: 'alias',
    section: 'advanced',
    title: t.config.mcpTitle,
    description: t.config.mcpDesc,
    keywords: [
      'mcp',
      'MCP',
      'model context protocol',
      '工具服务器',
      'tool server',
      'stdio',
      'reload',
      '热重载',
      'arael',
      'agent',
    ],
  })

  items.push({
    type: 'section',
    section: 'about',
    title: t.config.about,
    description: t.config.aboutDesc,
    keywords: [
      'about',
      '关于',
      '版本',
      'version',
      'logo',
      'myriad',
      'updater',
      '更新',
      'update',
      'upgrade',
      '升级',
      '回滚',
      'rollback',
      'snapshot',
      '快照',
    ],
  })

  items.push({
    type: 'section',
    section: 'permissions',
    title: t.config.permissions,
    description: t.config.permissionsDesc,
    keywords: [
      '权限',
      'permission',
      'elevated',
      '下放',
      '配额',
      'quota',
      'ai',
      '游客',
      'guest',
    ],
  })

  items.push({
    type: 'section',
    section: 'users',
    title: t.config.users,
    description: t.config.usersDesc,
    keywords: [
      '用户',
      'user',
      'users',
      '管理员',
      'admin',
      'oauth',
      '账户',
      'account',
      '在线',
      'online',
      '注册',
      'register',
      'identity',
      '绑定',
    ],
  })

  // Federation settings are admin-only in the nav; hide from search for non-admin
  // so users are not dropped into an empty section.
  if (isAdmin) {
    items.push({
      type: 'section',
      section: 'federation',
      title: t.config.federation,
      description: t.config.federationDesc,
      keywords: [
        'federation',
        '联邦',
        'trust',
        '信任',
        'allowlist',
        '白名单',
        'block',
        '封禁',
        'filter',
        '过滤',
        'mfp',
        'aro',
      ],
    })
  }

  items.push({
    type: 'section',
    section: 'modules',
    title: t.config.moduleSettings,
    description: t.config.moduleSettingsDesc,
    keywords: [
      '模块',
      'module',
      '资料库',
      'library',
      '来源',
      'source',
      '平台',
      '分类',
      '可见性',
      'visibility',
      '登录用户',
      '管理员',
      '一言',
      'hitokoto',
      'quote',
    ],
  })

  const guideEntries = buildGuideSearchIndex(locale)
  for (const g of guideEntries) {
    items.push({
      type: 'guide',
      section: g.section,
      title: g.title,
      description: g.description,
      keywords: g.keywords,
      haystack: g.haystack,
      guidePath: g.guidePath,
    })
  }

  const bySection = new Map<string, string[]>()
  for (const g of guideEntries) {
    const arr = bySection.get(g.section) ?? []
    for (const k of g.keywords) {
      if (k.length >= 2 && k.length <= 12) arr.push(k)
    }
    bySection.set(g.section, arr)
  }
  for (const item of items) {
    if (item.type !== 'section' && item.type !== 'alias') continue
    const extra = bySection.get(item.section)
    if (!extra?.length) continue
    const merged = new Set([
      ...item.keywords.map((k) => k.toLowerCase()),
      ...extra,
    ])
    item.keywords = Array.from(merged)
    item.haystack = [item.title, item.description, ...item.keywords]
      .join('\n')
      .toLowerCase()
  }

  return items
}
