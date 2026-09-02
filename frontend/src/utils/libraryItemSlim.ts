/**
 * Client-side library item compacting (defense in depth for older API caches
 * and pre-normalize payloads). Mirrors backend slim_library_metadata /
 * prefer_card_cover_url so the infinite canvas does not retain fat platform JSON
 * or oversized cover bitmaps.
 */

const BANGUMI_COVER_LARGE = /\/pic\/cover\/l\//i
const BANGUMI_COVER_GRID = /\/pic\/cover\/g\//i
/** Current API common/medium/grid: `/r/{width}/pic/cover/l/` — `l` is the source file. */
const BANGUMI_RESIZE_COVER = /\/r\/\d+\/pic\/cover\//i

/** Prefer card-sized CDN variants before decode. */
export function preferCardCoverUrl(url: string | null | undefined): string | null {
  if (url == null || typeof url !== 'string') return null
  const trimmed = url.trim()
  if (!trimmed) return null

  // Proxied URLs embed the CDN host in ?url= — rewrite the upstream first so
  // we don't no-op on encoded paths like pic%2Fcover%2Fl%2F.
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
          // Keep relative proxy paths relative for same-origin.
          if (trimmed.startsWith('/')) {
            return `${parsed.pathname}${parsed.search}`
          }
          return parsed.toString()
        }
      }
    } catch {
      // ignore malformed proxy URLs
    }
    return trimmed
  }

  if (trimmed.includes('bgm.tv') || trimmed.includes('lain.bgm')) {
    // `/r/{n}/pic/cover/l/` is a width resize of large; swapping `l`→`c` 400s.
    if (BANGUMI_RESIZE_COVER.test(trimmed)) return trimmed
    return trimmed
      .replace(BANGUMI_COVER_LARGE, '/pic/cover/c/')
      .replace(BANGUMI_COVER_GRID, '/pic/cover/c/')
  }

  if (
    (trimmed.includes('music.126.net') || trimmed.includes('music.163.com')) &&
    !trimmed.includes('param=')
  ) {
    const sep = trimmed.includes('?') ? '&' : '?'
    return `${trimmed}${sep}param=300y300`
  }

  return trimmed
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

/**
 * Drop bulk platform blobs; keep fields used by cards, progress, and play.
 */
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
