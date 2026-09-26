const BANGUMI_COVER_LARGE = /\/pic\/cover\/l\//gi
const BANGUMI_COVER_GRID = /\/pic\/cover\/g\//gi
const BANGUMI_RESIZE_COVER = /\/r\/\d+\/pic\/cover\//i

export function preferCardCoverUrl(url: string | null | undefined): string | null {
  if (url == null || typeof url !== 'string') return null
  const trimmed = url.trim()
  if (!trimmed) return null

  // Do not no-op on encoded paths (pic%2Fcover).
  if (trimmed.includes('/api/proxy/image') && trimmed.includes('url=')) {
    try {
      const base =
        typeof window !== 'undefined' ? window.location.origin : 'http://local'
      const parsed = new URL(trimmed, base)
      const upstream = parsed.searchParams.get('url')
      if (upstream) {
        const preferred = preferCardCoverUrl(upstream)
        if (preferred && preferred !== upstream) {
          parsed.searchParams.set('url', preferred)
          if (trimmed.startsWith('/')) {
            return `${parsed.pathname}${parsed.search}`
          }
          return parsed.toString()
        }
      }
    } catch {
    }
    return trimmed
  }

  if (trimmed.includes('bgm.tv') || trimmed.includes('lain.bgm')) {
    if (BANGUMI_RESIZE_COVER.test(trimmed)) return trimmed
    return trimmed
      .replaceAll(BANGUMI_COVER_LARGE, '/pic/cover/c/')
      .replaceAll(BANGUMI_COVER_GRID, '/pic/cover/c/')
  }

  if (isNeteaseHost(hostOf(trimmed))) {
    return withNeteaseCardSize(trimmed)
  }

  const hdslb = withBilibiliCardSize(trimmed)
  if (hdslb) return hdslb

  return trimmed
}

/** Card paint is ~220px. 288 is 1× plus ~20% slack (hover / canvas zoom). */
const NETEASE_CARD_PARAM = '288y288'
const NETEASE_CARD_EDGE = 288

function withNeteaseCardSize(url: string): string {
  const match = url.match(/[?&]param=(\d+)y(\d+)/)
  if (match) {
    const width = Number(match[1])
    const height = Number(match[2])
    if (width <= NETEASE_CARD_EDGE && height <= NETEASE_CARD_EDGE) return url
    return url.replace(/param=\d+y\d+/, `param=${NETEASE_CARD_PARAM}`)
  }
  const sep = url.includes('?') ? '&' : '?'
  return `${url}${sep}param=${NETEASE_CARD_PARAM}`
}

/** Card paint is ~220px; 2× retina plus ~20% slack. Width-only so CSS object-fit keeps aspect. */
const BILIBILI_CARD_WIDTH_SUFFIX = '@528w.webp'

function hostOf(url: string): string | null {
  try {
    const absolute = url.startsWith('//') ? `https:${url}` : url
    return new URL(absolute).hostname.replace(/\.$/, '').toLowerCase()
  } catch {
    return null
  }
}

function isNeteaseHost(host: string | null): boolean {
  if (!host) return false
  return ['music.126.net', 'music.163.com'].some(
    (d) => host === d || host.endsWith(`.${d}`),
  )
}

function isHdslbHost(host: string): boolean {
  return host === 'hdslb.com' || host.endsWith('.hdslb.com')
}

function withBilibiliCardSize(url: string): string | null {
  const host = hostOf(url)
  if (!host || !isHdslbHost(host)) return null
  const queryAt = url.indexOf('?')
  const path = queryAt === -1 ? url : url.slice(0, queryAt)
  const query = queryAt === -1 ? '' : url.slice(queryAt)
  if (path.includes('@')) return null
  if (/\.(gif|svg)$/i.test(path)) return null
  return `${path}${BILIBILI_CARD_WIDTH_SUFFIX}${query}`
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function slimArtistList(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value
      .map((entry) => {
        if (typeof entry === 'string') return entry
        if (isPlainObject(entry) && typeof entry.name === 'string') {
          return { name: entry.name }
        }
        return null
      })
      .filter(Boolean)
  }
  if (typeof value === 'string') return value
  return value
}

function slimAlbum(value: unknown): unknown {
  if (typeof value === 'string') return value
  if (!isPlainObject(value)) return value
  const out: Record<string, unknown> = {}
  if (typeof value.name === 'string') out.name = value.name
  const pic =
    (typeof value.picUrl === 'string' && value.picUrl) ||
    (typeof value.pic_url === 'string' && value.pic_url) ||
    (typeof value.cover === 'string' && value.cover) ||
    null
  if (pic) out.picUrl = preferCardCoverUrl(pic)
  return out
}

function pickKeys(
  obj: Record<string, unknown>,
  keys: readonly string[],
): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const key of keys) {
    if (obj[key] !== undefined) out[key] = obj[key]
  }
  return out
}

const FLAT_KEYS = [
  'id',
  'name',
  'url',
  'link',
  'web_url',
  'html_url',
  'short_link',
  'short_link_v2',
  'share_url',
  'appid',
  'season_id',
  'bvid',
  'aid',
  'subject_id',
  'media_type',
  'video_id',
  'full_name',
  'playtime_forever',
  'rate',
  'score',
  'artist',
  'dt',
  'duration',
  'fee',
  'isVip',
  'is_vip',
  'type',
  'status',
  'progress',
  'ep_status',
  'vol_status',
  'num_episodes_watched',
  'num_chapters_read',
  'num_volumes_read',
  'num_episodes',
  'num_chapters',
  'num_volumes',
  'platform',
] as const

export function slimLibraryMetadata(metadata: unknown): Record<string, unknown> {
  if (!isPlainObject(metadata)) return {}
  const out = pickKeys(metadata, FLAT_KEYS)

  if (metadata.ar !== undefined) out.ar = slimArtistList(metadata.ar)
  if (metadata.artists !== undefined) out.artists = slimArtistList(metadata.artists)
  if (metadata.al !== undefined) out.al = slimAlbum(metadata.al)
  if (metadata.album !== undefined) out.album = slimAlbum(metadata.album)

  if (isPlainObject(metadata.privilege) && metadata.privilege.fee !== undefined) {
    out.privilege = { fee: metadata.privilege.fee }
  }

  if (isPlainObject(metadata.list_status)) {
    out.list_status = pickKeys(metadata.list_status, [
      'score',
      'status',
      'num_episodes_watched',
      'num_chapters_read',
      'num_volumes_read',
    ])
  }

  if (isPlainObject(metadata.subject)) {
    out.subject = pickKeys(metadata.subject, [
      'id',
      'url',
      'eps',
      'volumes',
      'platform',
      'name',
      'name_cn',
    ])
  }

  if (isPlainObject(metadata.node)) {
    out.node = pickKeys(metadata.node, [
      'id',
      'url',
      'title',
      'num_episodes',
      'num_chapters',
      'num_volumes',
    ])
  }

  if (isPlainObject(metadata.owner)) {
    out.owner = pickKeys(metadata.owner, ['login'])
  }

  return out
}

export interface SlimLibraryItemInput {
  id: string
  item_type: string
  title: string
  cover: string | null
  platform: string
  metadata: unknown
}

export function slimLibraryItem<T extends SlimLibraryItemInput>(item: T): T {
  return {
    ...item,
    cover: preferCardCoverUrl(item.cover) ?? item.cover,
    metadata: slimLibraryMetadata(item.metadata),
  }
}

export function slimLibraryItems<T extends SlimLibraryItemInput>(items: T[]): T[] {
  return items.map(slimLibraryItem)
}
