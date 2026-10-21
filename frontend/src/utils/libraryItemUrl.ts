/** 资料库条目在原站的地址；拿不到时为 null。资料库卡片和报告舞台共用。 */
export function resolveLibraryItemUrl(item: {
  id: string
  platform: string
  item_type: string
  metadata: any
}): string | null {
  const m =
    item.metadata && typeof item.metadata === 'object' ? item.metadata : {}
  const asHttp = (v: unknown): string | null => {
    if (typeof v !== 'string') return null
    const s = v.trim()
    if (!s) return null
    if (/^https?:\/\//i.test(s)) return s
    if (s.startsWith('//')) return `https:${s}`
    return null
  }

  const direct =
    asHttp(m.url) ||
    asHttp(m.link) ||
    asHttp(m.web_url) ||
    asHttp(m.html_url) ||
    asHttp(m.short_link_v2) ||
    asHttp(m.short_link) ||
    asHttp(m?.subject?.url) ||
    asHttp(m?.node?.url) ||
    asHttp(m?.share_url)
  if (direct) return direct

  const platform = (item.platform || '').toLowerCase()
  const id = item.id || ''

  if (platform.includes('steam') || id.startsWith('steam_')) {
    const appid = m.appid ?? id.replaceAll(/^steam_game_/g, '')
    if (appid !== '' && appid != null) {
      return `https://store.steampowered.com/app/${appid}`
    }
  }

  if (
    platform.includes('bilibili') ||
    platform.includes('bili') ||
    id.startsWith('bilibili_')
  ) {
    if (m.season_id != null) {
      return `https://www.bilibili.com/bangumi/play/ss${m.season_id}`
    }
    const bvid = typeof m.bvid === 'string' ? m.bvid : null
    if (bvid) return `https://www.bilibili.com/video/${bvid}`
    const aid = m.aid ?? m.id
    if (aid != null && /^\d+$/.test(String(aid))) {
      return `https://www.bilibili.com/video/av${aid}`
    }
  }

  if (
    platform.includes('bangumi') ||
    platform.includes('bgm') ||
    id.startsWith('bangumi_')
  ) {
    const sid =
      m.subject_id ??
      m.subject?.id ??
      (id.startsWith('bangumi_subject_')
        ? id.slice('bangumi_subject_'.length)
        : null)
    if (sid != null && String(sid) !== '') {
      return `https://bgm.tv/subject/${sid}`
    }
  }

  if (
    platform.includes('mal') ||
    platform.includes('myanimelist') ||
    id.startsWith('mal_')
  ) {
    const mal = id.match(/^mal_(anime|manga)_(\d+)$/i)
    if (mal) return `https://myanimelist.net/${mal[1].toLowerCase()}/${mal[2]}`
    const kind =
      m.media_type === 'manga' || item.item_type === 'book' ? 'manga' : 'anime'
    const mid = m.id ?? m.node?.id
    if (mid != null) return `https://myanimelist.net/${kind}/${mid}`
  }

  if (platform.includes('github')) {
    if (typeof m.full_name === 'string' && m.full_name.includes('/')) {
      return `https://github.com/${m.full_name}`
    }
    if (typeof m.name === 'string' && typeof m.owner?.login === 'string') {
      return `https://github.com/${m.owner.login}/${m.name}`
    }
  }

  if (platform.includes('youtube') || platform.includes('yt')) {
    const vid = m.video_id ?? m.id
    if (typeof vid === 'string' && vid.length >= 6) {
      return `https://www.youtube.com/watch?v=${vid}`
    }
  }

  return null
}
