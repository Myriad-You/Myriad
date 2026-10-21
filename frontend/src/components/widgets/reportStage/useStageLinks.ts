import type { LibraryItem } from '../../library/libraryCanvasVisible'
import type { StageMedia } from './stageMedia'
import { useEffect, useMemo, useState } from 'react'
import { apiService } from '../../../services/api'
import { resolveLibraryItemUrl } from '../../../utils/libraryItemUrl'

/** 点封面做什么：去原站，或者用站内播放器放这首歌。 */
export type StageLink = { kind: 'open'; href: string } | { kind: 'play'; item: LibraryItem }

/** 网易云的歌在站内放，要先知道歌曲 id；卡片数据里没有，按名字去资料库查。 */
function needsLookup(platform: string, item: StageMedia): boolean {
  return platform === 'netease' || !item.href
}

// 同一份封面列表只查一次：重开舞台、来回切平台都不再请求。
const lookups = new Map<string, Promise<(LibraryItem | null)[]>>()

function lookup(platform: string, items: readonly StageMedia[]): Promise<(LibraryItem | null)[]> {
  const params = new URLSearchParams({ platform })
  for (const item of items) {
    params.append('title', item.title)
    if (item.by) params.append('by', item.by)
  }
  const key = params.toString()
  let pending = lookups.get(key)
  if (!pending) {
    pending = apiService
      .get<{ items?: (LibraryItem | null)[] }>(`/api/library/lookup?${key}`)
      .then((body) => (Array.isArray(body?.items) ? body.items : []))
      .catch(() => {
        // 查不到就只是点不了；下次打开再试。
        lookups.delete(key)
        return []
      })
    lookups.set(key, pending)
  }
  return pending
}

/**
 * 每张封面（按封面地址）点了做什么。卡片数据自带原站地址的直接用；
 * 其余的在舞台打开时向资料库查一次，没查到的那张就不能点。
 */
export function useStageLinks(
  platform: string,
  media: readonly StageMedia[],
  active: boolean,
): ReadonlyMap<string, StageLink> {
  const [found, setFound] = useState<{ media: readonly StageMedia[]; items: (LibraryItem | null)[] }>()

  const missing = useMemo(() => media.filter((item) => needsLookup(platform, item)), [media, platform])
  const lookupNow = active && missing.length > 0
  useEffect(() => {
    if (!lookupNow) return
    let alive = true
    void lookup(platform, missing).then((items) => {
      if (alive) setFound({ media, items })
    })
    return () => {
      alive = false
    }
  }, [lookupNow, media, missing, platform])

  return useMemo(() => {
    const links = new Map<string, StageLink>()
    const looked = found?.media === media ? found.items : []
    let index = 0
    for (const item of media) {
      if (!needsLookup(platform, item)) {
        if (item.href) links.set(item.cover, { kind: 'open', href: item.href })
        continue
      }
      const match = looked[index++]
      if (match && platform === 'netease') {
        links.set(item.cover, { kind: 'play', item: match })
        continue
      }
      const href = (match && resolveLibraryItemUrl(match)) || item.href
      if (href) links.set(item.cover, { kind: 'open', href })
    }
    return links
  }, [found, media, platform])
}
