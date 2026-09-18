import type { Locale } from '../../../i18n'
import type { LocaleConfig } from '../../../i18n/assembleLocale'
import type { ConfigSearchableItem } from '../../settings/guides/configSearch'
import type { Config } from './types'
import { buildGuideSearchIndex } from '../../settings/guides/guideSearchIndex'
import { configSectionCatalog } from './configSections'

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
    agent: string
    agentDesc: string
    agentChannelsTitle: string
    agentChannelAdd: string
    agentHeartbeatTitle: string
    agentSkillsTitle: string
    agentMemoryTitle: string
    lab: string
    labDesc: string
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
    searchKeywords: LocaleConfig['searchKeywords']
  }
  notificationCenter: {
    title: string
    settingsDesc: string
  }
}

/** search index; platforms come from config, not a hand list */
export function buildSearchableContent(
  config: Config | null,
  t: ConfigSearchI18n,
  locale: Locale,
  options?: {
    isAdmin?: boolean
    agentTitle?: string
    federationEnabled?: boolean
  },
): ConfigSearchableItem[] {
  if (!config) return []
  const isAdmin = options?.isAdmin !== false // default include; pass false to filter
  const federationEnabled = options?.federationEnabled !== false
  const agentTitle = options?.agentTitle?.trim() || t.config.agent

  const sections = configSectionCatalog(
    t,
    isAdmin,
    agentTitle,
    federationEnabled,
  )
  const visibleSections = new Set<string>(sections.map((section) => section.id))
  const items: ConfigSearchableItem[] = sections.map((section) => ({
    type: 'section',
    section: section.id,
    title: section.title,
    description: section.description,
    keywords: section.keywords,
  }))
  items
    .find((item) => item.section === 'platforms')
    ?.keywords.push(
      ...config.platforms.flatMap((platform) => {
        const name = platform.name.trim()
        return name ? [name, name.toLowerCase()] : []
      }),
    )
  items
    .find((item) => item.section === 'agent')
    ?.keywords.push(
      t.config.agent,
      agentTitle,
      t.config.agentChannelsTitle,
      t.config.agentChannelAdd,
      t.config.agentHeartbeatTitle,
      t.config.agentSkillsTitle,
      t.config.agentMemoryTitle,
    )

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.connectedPlatforms,
    description: t.config.connectedPlatformsDesc,
    keywords: Iterator.from(
      t.config.searchKeywords.connectedPlatforms,
    ).toArray(),
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.analytics.visitorTitle,
    description: t.config.analytics.visitorDesc,
    keywords: Iterator.from(t.config.searchKeywords.visitor).toArray(),
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.analytics.aiUsageTitle,
    description: t.config.analytics.aiUsageDesc,
    keywords: Iterator.from(t.config.searchKeywords.aiUsage).toArray(),
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.thirdPartyAnalytics,
    description: t.config.thirdPartyAnalyticsDesc,
    keywords: Iterator.from(t.config.searchKeywords.thirdParty).toArray(),
  })

  items.push({
    type: 'section',
    section: 'platforms',
    title: t.config.data,
    description: t.config.dataDesc,
    keywords: Iterator.from(t.config.searchKeywords.data).toArray(),
  })

  config.platforms.forEach((platform) => {
    items.push({
      type: 'platform',
      section: 'platforms',
      title: platform.name,
      description: platform.description,
      keywords: [
        platform.name.toLowerCase(),
        ...t.config.searchKeywords.platformItem,
      ],
    })
  })

  items.push({
    type: 'alias',
    section: 'modules',
    title: t.config.music,
    description: t.config.musicDesc,
    keywords: Iterator.from(t.config.searchKeywords.music).toArray(),
  })

  items.push({
    type: 'alias',
    section: 'advanced',
    title: t.config.network,
    description: t.config.networkDesc,
    keywords: Iterator.from(t.config.searchKeywords.network).toArray(),
  })

  items.push({
    type: 'alias',
    section: 'advanced',
    title: t.config.mcpTitle,
    description: t.config.mcpDesc,
    keywords: Iterator.from(t.config.searchKeywords.mcp).toArray(),
  })

  const guideEntries = buildGuideSearchIndex(locale).filter((guide) =>
    visibleSections.has(guide.section),
  )
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
    const merged = new Set(item.keywords.map((k) => k.toLowerCase())).union(
      new Set(extra),
    )
    item.keywords = Iterator.from(merged).toArray()
    item.haystack = [item.title, item.description, ...item.keywords]
      .join('\n')
      .toLowerCase()
  }

  return items
}
