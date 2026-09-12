export type CollectionStatusKey =
  | 'wish'
  | 'done'
  | 'doing'
  | 'on_hold'
  | 'dropped'

export interface ProgressPart {
  current: number
  total: number | null
}

export interface WatchProgress {
  primary: ProgressPart
  episodes?: ProgressPart
  chapters?: ProgressPart
  volumes?: ProgressPart
  status: CollectionStatusKey | null
  /** 0–100; null if total unknown. */
  percent: number | null
}

function toNonNegInt(value: unknown): number | null {
  if (value == null || value === '') return null
  const n = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(n) || n < 0) return null
  return Math.floor(n)
}

function part(current: number, totalRaw: number | null | undefined): ProgressPart {
  const total =
    totalRaw != null && totalRaw > 0 ? totalRaw : null
  return { current: Math.max(0, current), total }
}

export function parseProgressString(
  raw: unknown,
): ProgressPart | null {
  if (raw == null) return null
  if (typeof raw === 'number') {
    if (!Number.isFinite(raw) || raw < 0) return null
    return part(Math.floor(raw), null)
  }
  if (typeof raw !== 'string') return null
  const s = raw.trim()
  if (!s) return null
  const m = s.match(/^(\d+)\s*\/\s*(\d+|\?)$/)
  if (m) {
    const current = Number(m[1])
    const total = m[2] === '?' ? null : Number(m[2])
    return part(current, total && total > 0 ? total : null)
  }
  const only = s.match(/^(\d+)$/)
  if (only) return part(Number(only[1]), null)
  return null
}

function bangumiStatus(type: unknown): CollectionStatusKey | null {
  const n = toNonNegInt(type)
  switch (n) {
    case 1:
      return 'wish'
    case 2:
      return 'done'
    case 3:
      return 'doing'
    case 4:
      return 'on_hold'
    case 5:
      return 'dropped'
    default:
      return null
  }
}

function malStatus(status: unknown): CollectionStatusKey | null {
  if (typeof status !== 'string' || !status) return null
  switch (status) {
    case 'completed':
      return 'done'
    case 'watching':
    case 'reading':
      return 'doing'
    case 'plan_to_watch':
    case 'plan_to_read':
      return 'wish'
    case 'on_hold':
      return 'on_hold'
    case 'dropped':
      return 'dropped'
    default:
      return null
  }
}

function pickStatus(meta: Record<string, unknown>): CollectionStatusKey | null {
  return bangumiStatus(meta.type) ?? malStatus(meta.status)
}

function episodeTotal(meta: Record<string, unknown>): number | null {
  const subject = meta.subject as Record<string, unknown> | undefined
  const node = meta.node as Record<string, unknown> | undefined
  return (
    toNonNegInt(subject?.eps) ??
    toNonNegInt(node?.num_episodes) ??
    toNonNegInt(meta.num_episodes) ??
    null
  )
}

function chapterTotal(meta: Record<string, unknown>): number | null {
  const subject = meta.subject as Record<string, unknown> | undefined
  const node = meta.node as Record<string, unknown> | undefined
  return (
    toNonNegInt(node?.num_chapters) ??
    toNonNegInt(subject?.eps) ??
    toNonNegInt(meta.num_chapters) ??
    null
  )
}

function volumeTotal(meta: Record<string, unknown>): number | null {
  const subject = meta.subject as Record<string, unknown> | undefined
  const node = meta.node as Record<string, unknown> | undefined
  return (
    toNonNegInt(node?.num_volumes) ??
    toNonNegInt(subject?.volumes) ??
    toNonNegInt(meta.num_volumes) ??
    null
  )
}

function listStatus(meta: Record<string, unknown>): Record<string, unknown> | undefined {
  const ls = meta.list_status
  if (ls && typeof ls === 'object' && !Array.isArray(ls)) {
    return ls as Record<string, unknown>
  }
  return undefined
}

function hasMeaningfulPart(p: ProgressPart | undefined): boolean {
  if (!p) return false
  if (p.current === 0 && (p.total == null || p.total === 0)) return false
  return true
}

function percentOf(p: ProgressPart): number | null {
  if (p.total == null || p.total <= 0) return null
  return Math.min(100, Math.round((p.current / p.total) * 100))
}

function fillCompletedPart(
  p: ProgressPart | null | undefined,
  status: CollectionStatusKey | null,
  totalHint?: number | null,
): ProgressPart | null {
  if (status !== 'done') {
    return p ?? null
  }

  const total =
    (p?.total != null && p.total > 0 ? p.total : null) ??
    (totalHint != null && totalHint > 0 ? totalHint : null)

  if (total != null) {
    const current = Math.max(p?.current ?? 0, total)
    return part(current, total)
  }

  if (p && hasMeaningfulPart(p)) return p
  return null
}

export function getWatchProgress(
  itemType: string,
  metadata: unknown,
): WatchProgress | null {
  if (!metadata || typeof metadata !== 'object') return null
  const meta = metadata as Record<string, unknown>
  const status = pickStatus(meta)
  const ls = listStatus(meta)

  const isAnimeLike =
    itemType === 'anime' || itemType === 'tv_series' || itemType === 'video'
  const isBook = itemType === 'book'

  if (isAnimeLike) {
    let episodes: ProgressPart | null = parseProgressString(meta.progress)
    const epTotal = episodeTotal(meta)

    if (!episodes) {
      const watched =
        toNonNegInt(meta.ep_status) ??
        toNonNegInt(ls?.num_episodes_watched) ??
        toNonNegInt(meta.num_episodes_watched)
      if (watched != null) {
        episodes = part(watched, epTotal)
      }
    } else if (episodes.total == null && epTotal != null) {
      episodes = part(episodes.current, epTotal)
    }

    episodes = fillCompletedPart(episodes, status, epTotal)

    if (!episodes || !hasMeaningfulPart(episodes)) return null

    return {
      primary: episodes,
      episodes,
      status,
      percent: percentOf(episodes),
    }
  }

  if (isBook) {
    const chTotal = chapterTotal(meta)
    const volTotal = volumeTotal(meta)

    const chCurrent =
      toNonNegInt(meta.ep_status) ??
      toNonNegInt(ls?.num_chapters_read) ??
      toNonNegInt(meta.num_chapters_read)
    const volCurrent =
      toNonNegInt(meta.vol_status) ??
      toNonNegInt(ls?.num_volumes_read) ??
      toNonNegInt(meta.num_volumes_read)

    const fromProgress = parseProgressString(meta.progress)

    let chapters: ProgressPart | null =
      chCurrent != null
        ? part(chCurrent, chTotal)
        : fromProgress
          ? part(fromProgress.current, fromProgress.total ?? chTotal)
          : null

    let volumes: ProgressPart | null =
      volCurrent != null ? part(volCurrent, volTotal) : null

    chapters = fillCompletedPart(chapters, status, chTotal)
    volumes = fillCompletedPart(volumes, status, volTotal)

    if (status === 'done' && !chapters && !volumes) {
      if (chTotal != null) chapters = part(chTotal, chTotal)
      if (volTotal != null) volumes = part(volTotal, volTotal)
    }

    const meaningfulCh = hasMeaningfulPart(chapters ?? undefined)
    const meaningfulVol = hasMeaningfulPart(volumes ?? undefined)

    if (!meaningfulCh && !meaningfulVol) return null

    const primary =
      meaningfulCh && chapters
        ? chapters
        : meaningfulVol && volumes
          ? volumes
          : chapters ?? volumes!

    return {
      primary,
      chapters: meaningfulCh ? chapters ?? undefined : undefined,
      volumes: meaningfulVol ? volumes ?? undefined : undefined,
      status,
      percent: percentOf(primary),
    }
  }

  return null
}

export interface WatchProgressLabels {
  progressEp: string
  progressEpOnly: string
  progressCh: string
  progressChOnly: string
  progressVol: string
  progressVolOnly: string
  progressJoin: string
  statusDoing: string
  statusDone: string
  statusWish: string
  statusOnHold: string
  statusDropped: string
}

function fill(
  template: string,
  vars: Record<string, string | number>,
): string {
  return Object.entries(vars).reduce(
    (acc, [k, v]) => acc.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

function formatPart(
  p: ProgressPart,
  withTotal: string,
  only: string,
): string {
  if (p.total != null && p.total > 0) {
    return fill(withTotal, { current: p.current, total: p.total })
  }
  return fill(only, { current: p.current })
}

export function formatWatchProgressText(
  progress: WatchProgress,
  labels: WatchProgressLabels,
): string {
  if (progress.chapters || progress.volumes) {
    const bits: string[] = []
    if (progress.chapters) {
      bits.push(
        formatPart(
          progress.chapters,
          labels.progressCh,
          labels.progressChOnly,
        ),
      )
    }
    if (progress.volumes) {
      bits.push(
        formatPart(
          progress.volumes,
          labels.progressVol,
          labels.progressVolOnly,
        ),
      )
    }
    return bits.join(labels.progressJoin)
  }

  const p = progress.episodes ?? progress.primary
  return formatPart(p, labels.progressEp, labels.progressEpOnly)
}

export function formatWatchStatusLabel(
  status: CollectionStatusKey | null | undefined,
  labels: WatchProgressLabels,
  opts?: { onlyDoing?: boolean },
): string | null {
  if (!status) return null
  if (opts?.onlyDoing !== false && status !== 'doing') return null
  switch (status) {
    case 'doing':
      return labels.statusDoing
    case 'done':
      return labels.statusDone
    case 'wish':
      return labels.statusWish
    case 'on_hold':
      return labels.statusOnHold
    case 'dropped':
      return labels.statusDropped
    default:
      return null
  }
}
