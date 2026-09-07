/**
 * 宿主对 window.__musicPlayerState 的唯一写入入口。
 * Context / Tapp 只读全局态并消费事件，禁止把碎片字段回写。
 */

import type { MusicPlayerSnapshotInput } from '../../utils/musicPlayerState'
import { buildMusicPlayerSnapshot } from '../../utils/musicPlayerState'

export type MusicPlayerWindowState = Record<string, unknown>

type MusicPlayerWindow = Window & {
  __musicPlayerState?: MusicPlayerWindowState
}

function getMusicWindow(): MusicPlayerWindow | null {
  if (typeof window === 'undefined') return null
  return window as MusicPlayerWindow
}

export function getGlobalState(): MusicPlayerWindowState | null {
  return getMusicWindow()?.__musicPlayerState ?? null
}

export function setGlobalState(state: Record<string, unknown>): void {
  const w = getMusicWindow()
  if (!w) return
  w.__musicPlayerState = {
    ...(w.__musicPlayerState || {}),
    ...state,
  }
}

/**
 * 立即补丁加载/错误标志并通知 Tapp（不依赖 React 下一帧）。
 * 用于 canplay / error 热路径，让缓冲条与错误提示及时出现。
 */
export function patchPlaybackFlags(flags: {
  isAudioLoading?: boolean
  lastPlaybackError?: string | null
  generation?: number
}): void {
  const w = getMusicWindow()
  if (!w) return
  const prev = w.__musicPlayerState || {}
  const next = { ...prev, ...flags }
  w.__musicPlayerState = next
  w.dispatchEvent(
    new CustomEvent('music-player-state-change', {
      detail: {
        ...next,
        isAudioLoading: next.isAudioLoading,
        lastPlaybackError: next.lastPlaybackError ?? null,
        generation: next.generation ?? 0,
      },
    }),
  )
}

/** 进度 / 歌词 / isPlaying 热路径：就地补丁，避免每 tick 换新对象。 */
export function patchLiveGlobalState(patch: Record<string, unknown>): void {
  const prev = getGlobalState()
  if (!prev) return
  Object.assign(prev, patch)
}

/** 完整快照写入并派发 music-player-state-change。 */
export function publishMusicPlayerSnapshot(
  input: MusicPlayerSnapshotInput,
): Record<string, unknown> {
  const detail = buildMusicPlayerSnapshot(input)
  setGlobalState(detail)
  const w = getMusicWindow()
  if (w) {
    w.dispatchEvent(new CustomEvent('music-player-state-change', { detail }))
  }
  return detail
}
