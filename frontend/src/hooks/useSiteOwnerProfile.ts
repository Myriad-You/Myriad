/**
 * 站长公开资料（首页信息条 / 对外展示）。
 *
 * 取代原先 `userInfoCache` 的 30 分钟 localStorage 缓存 —— 那份缓存只在登录/
 * 登出时失效，站长换了头像来源或重抓平台数据后，首页最长半小时不更新。
 * 现在缓存交给 HTTP 层（后端带 `Cache-Control: max-age=60` + 内容 ETag），
 * 变更即时性交给 `avatar-changed` / `profile-display-changed` 广播。
 *
 * **强制刷新**必须绕过浏览器 HTTP 缓存：plain `fetch` 会遵守
 * `public, max-age=60`，切换来源后最多再等一分钟才看到新脸/新文案。
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { API_URL } from '../config'
import {
  onAvatarChanged,
  onProfileDisplayChanged,
} from '../services/profileDisplayEvents'
import { proxyImageUrl } from '../utils/proxyImageUrl'

export interface SiteOwnerProfile {
  name: string
  /** 可能为 null：由调用方 / <Avatar> 生成本地兜底，不在这里编造地址 */
  avatar: string | null
  bio: string
  platform?: string | null
}

export interface FetchSiteOwnerProfileOptions {
  /**
   * 绕过浏览器 HTTP 缓存与 in-flight 复用。
   * 仅用于 mutation 后的主动刷新（avatar-changed 等）；冷启动访客仍走默认缓存。
   */
  force?: boolean
}

/** 并发挂载多个消费者时只发一次「非强制」请求 */
let inflight: Promise<SiteOwnerProfile | null> | null = null

/** 并发 force 刷新合并为一次（avatar-changed 与 profile-display-changed 常双发） */
let forceInflight: Promise<SiteOwnerProfile | null> | null = null

/** @internal */
export function __resetSiteOwnerProfileInflightForTests(): void {
  inflight = null
  forceInflight = null
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
    // 后端已代理；这里兜底旧响应里的直链
    avatar: proxyImageUrl(info.avatar as string | null | undefined) ?? null,
    bio: typeof info.bio === 'string' ? info.bio : '',
    platform: typeof info.platform === 'string' ? info.platform : null,
  }
}

/**
 * 拉取站长公开资料。
 *
 * - 默认：可复用 in-flight，浏览器可按 Cache-Control 缓存（访客友好）。
 * - `force: true`：`cache: 'no-store'` + 时间戳 query，忽略 max-age / 陈旧 ETag 命中。
 */
export async function fetchSiteOwnerProfile(
  options: FetchSiteOwnerProfileOptions = {},
): Promise<SiteOwnerProfile | null> {
  const force = options.force === true

  if (!force && inflight) return inflight
  // 同帧双事件（notifyAvatarChanged 同时派发 avatar + profile-display）合并为一次 force 请求
  if (force && forceInflight) return forceInflight

  const run = (async (): Promise<SiteOwnerProfile | null> => {
    try {
      // force 时附带 _ts：部分中间层不尊重 Request.cache，靠唯一 URL 破缓存
      const url = force
        ? `${API_URL}/api/profile/user-info?_ts=${Date.now()}`
        : `${API_URL}/api/profile/user-info`

      const response = await fetch(url, {
        credentials: 'include',
        // 仅 mutation 触发的刷新绕过 HTTP 缓存；冷路径保留 public max-age 收益
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
    // 不与冷路径 inflight 混用；仅合并并发 force
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
  /** 站长资料尚未就绪时的占位名（通常是站点标题） */
  fallbackName?: string
  fallbackBio?: string
  /**
   * 为 false 时不发请求也不回退占位（默认 true）。
   * 控制面板只有站长需要这份资料，普通用户不必为此多打一次公开接口。
   */
  enabled?: boolean
}

export function useSiteOwnerProfile({
  fallbackName,
  fallbackBio,
  enabled = true,
}: SiteOwnerProfileOptions = {}) {
  const [profile, setProfile] = useState<SiteOwnerProfile | null>(null)
  /**
   * 强制刷新代数：avatar 代理 URL 未变时仍用于 remount `<img>`，
   * 避免浏览器图片缓存继续显示旧字节。
   */
  const [avatarEpoch, setAvatarEpoch] = useState(0)
  const requestGen = useRef(0)
  /**
   * avatar-changed 与 profile-display-changed 常双发；后一次 refresh 的
   * requestGen 会 supersede 前一次。用 ref 记住「本轮需要 bump epoch」，
   * 避免双发时只剩 bumpAvatar=false 的那次生效。
   */
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
      // 被更新的 force 刷新 superseded 时丢弃陈旧结果
      if (gen !== requestGen.current) return
      applyProfile(next)
      // 仅头像相关变更 bump epoch（代理 URL 未变时仍 remount <img>）。
      // 纯文案 profile-display 刷新不得 bump，否则首页头像闪一下。
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

  // 站长换了头像来源 / 名称简介来源 / 重抓平台数据 → 立即跟上（含跨标签页）。
  // profile-display：强制刷新文案。
  // avatar-changed：标记 pendingAvatarBump 后再 force 刷新；notifyAvatarChanged
  // 双发时 forceInflight 合并为一次 /user-info，epoch 仍会 bump。
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
