/** 站长公开资料。缓存走 HTTP Cache-Control/ETag；变更靠 avatar/profile-display 广播。 */
import { useCallback, useEffect, useRef, useState } from 'react'
import { API_URL } from '../config'
import {
  onAvatarChanged,
  onProfileDisplayChanged,
} from '../services/profileDisplayEvents'
import { proxyImageUrl } from '../utils/proxyImageUrl'

export interface SiteOwnerProfile {
  name: string

  /** 可能为 null：由调用方 / <Avatar> 生成本地兜底，不在这里编造地址。 */
  avatar: string | null
  bio: string
  platform?: string | null
}

export interface FetchSiteOwnerProfileOptions {

  /** 绕过浏览器 HTTP 缓存与 in-flight 复用。仅 mutation 后刷新；冷启动访客走默认缓存。 */
  force?: boolean
}

/** 并发挂载多个消费者时只发一次非强制请求。 */
let inflight: Promise<SiteOwnerProfile | null> | null = null

/** 并发 force 合并为一次（avatar-changed 与 profile-display-changed 常双发）。 */
let forceInflight: Promise<SiteOwnerProfile | null> | null = null

/** @internal */
export function __resetSiteOwnerProfileInflightForTests(): void {
  inflight = null
  forceInflight = null
}

const PLACEHOLDER_BIOS = new Set([
  'No bio available',
  '这家伙很懒，没有介绍呢',
])

function normalizeOwnerBio(bio: string): string {
  const trimmed = bio.trim()
  return !trimmed || PLACEHOLDER_BIOS.has(trimmed) ? '' : trimmed
}

function parseProfilePayload(data: unknown): SiteOwnerProfile | null {
  if (!data || typeof data !== 'object') return null
  const root = data as { success?: unknown; user_info?: unknown }
  if (!root.success || !root.user_info || typeof root.user_info !== 'object') {
    return null
  }
  const info = root.user_info as Record<string, unknown>
  return {
    name: typeof info.name === 'string' ? info.name : '',

    // 后端已代理；这里兜底旧响应里的直链。
    avatar: proxyImageUrl(info.avatar as string | null | undefined) ?? null,
    bio: normalizeOwnerBio(typeof info.bio === 'string' ? info.bio : ''),
    platform: typeof info.platform === 'string' ? info.platform : null,
  }
}

/** force: cache:'no-store' + _ts query，忽略 max-age / 陈旧 ETag。 */
export async function fetchSiteOwnerProfile(
  options: FetchSiteOwnerProfileOptions = {},
): Promise<SiteOwnerProfile | null> {
  const force = options.force === true

  if (!force && inflight) return inflight

  // 同帧双事件合并为一次 force 请求。
  if (force && forceInflight) return forceInflight

  const run = (async (): Promise<SiteOwnerProfile | null> => {
    try {
      // force 附带 _ts：部分中间层不尊重 Request.cache，靠唯一 URL 破缓存。
      const url = force
        ? `${API_URL}/api/profile/user-info?_ts=${Date.now()}`
        : `${API_URL}/api/profile/user-info`

      const response = await fetch(url, {
        credentials: 'include',

        // 仅 mutation 刷新绕过 HTTP 缓存；冷路径保留 public max-age。
        ...(force ? { cache: 'no-store' as RequestCache } : {}),
      })
      if (!response.ok) return null
      const data: unknown = await response.json()
      return parseProfilePayload(data)
    } catch {
      return null
    }
  })()

  if (force) {
    forceInflight = run.finally(() => {
      forceInflight = null
    })
    return forceInflight
  }

  inflight = run.finally(() => {
    inflight = null
  })
  return inflight
}

interface SiteOwnerProfileOptions {

  /** 站长资料尚未就绪时的占位名（通常是站点标题）。 */
  fallbackName?: string
  fallbackBio?: string

  /** false 时不发请求也不回退占位。控制面板只有站长需要这份资料。 */
  enabled?: boolean
}

export function useSiteOwnerProfile({
  fallbackName,
  fallbackBio,
  enabled = true,
}: SiteOwnerProfileOptions = {}) {
  const [profile, setProfile] = useState<SiteOwnerProfile | null>(null)

  const [avatarEpoch, setAvatarEpoch] = useState(0)
  const requestGen = useRef(0)

  // 双发时后一次 refresh 会 supersede；用 ref 记住本轮要 bump epoch。
  const pendingAvatarBump = useRef(false)

  const applyProfile = useCallback(
    (next: SiteOwnerProfile | null) => {
      setProfile(
        next ??
          (fallbackName === undefined
            ? null
            : { name: fallbackName, avatar: null, bio: fallbackBio ?? '' }),
      )
    },
    [fallbackName, fallbackBio],
  )

  const refresh = useCallback(
    async (force = false) => {
      if (!enabled) {
        setProfile(null)
        return
      }
      const gen = ++requestGen.current
      const next = await fetchSiteOwnerProfile({ force })

      // 被更新的 force 刷新 superseded 时丢弃陈旧结果。
      if (gen !== requestGen.current) return
      applyProfile(next)

      // 仅头像相关变更 bump epoch。纯文案刷新不得 bump，否则首页头像闪一下。
      if (force && pendingAvatarBump.current) {
        pendingAvatarBump.current = false
        setAvatarEpoch((n) => n + 1)
      }
    },
    [enabled, applyProfile],
  )

  useEffect(() => {
    void refresh(false)
  }, [refresh])

  useEffect(() => {
    const offDisplay = onProfileDisplayChanged(() => void refresh(true))
    const offAvatar = onAvatarChanged(() => {
      pendingAvatarBump.current = true
      void refresh(true)
    })
    return () => {
      offDisplay()
      offAvatar()
    }
  }, [refresh])

  return { profile, refresh, avatarEpoch }
}
