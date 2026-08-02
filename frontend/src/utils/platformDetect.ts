/**
 * 统一平台 / 引擎检测（动效硬件档、音频通路、Tapp WebKit 特判共用）。
 *
 * 区分两类概念：
 * - **OS**（android/ios/macos/…）：硬件门槛、产品策略
 * - **引擎**（Apple WebKit 且非 Chromium）：Safari/WK 合成层、多窗口等宿主行为
 */

export type OsKind = 'android' | 'ios' | 'macos' | 'windows' | 'linux' | 'unknown'

const RE_APPLE_WEBKIT = /\bAppleWebKit\b/
const RE_CHROMIUM = /\bChrom(e|ium)\b/

function getNav(
  nav?: Navigator | null,
): Navigator | null {
  if (nav !== undefined) return nav
  return typeof navigator !== 'undefined' ? navigator : null
}

function getUa(ua?: string, nav?: Navigator | null): string {
  if (ua !== undefined) return ua
  const n = getNav(nav)
  return n?.userAgent || ''
}

/** iPadOS 13+ 桌面模式：UA/platform 像 Mac，但多点触控 */
export function isIpadOsDesktopUa(
  ua?: string,
  nav?: Navigator | null,
): boolean {
  const n = getNav(nav)
  const maxTouch =
    n && typeof n.maxTouchPoints === 'number' ? n.maxTouchPoints : 0
  if (maxTouch <= 1) return false
  // 历史 musicPlayer：platform === MacIntel && touch
  if (n?.platform === 'MacIntel') return true
  // 历史 hardware tier：Macintosh UA && touch（排除已带 iPhone 的串）
  const u = getUa(ua, n)
  if (/iPhone|iPod|iPad/i.test(u)) return false
  return /Macintosh|Mac OS X/i.test(u)
}

/** iPhone / iPod / iPad（含 iPadOS 桌面 UA） */
export function isAppleTouchDevice(
  ua?: string,
  nav?: Navigator | null,
): boolean {
  const n = getNav(nav)
  const u = getUa(ua, n)
  if (/iPad|iPhone|iPod/i.test(u)) return true
  if (isIpadOsDesktopUa(u, n)) return true
  return false
}

/** 粗指针触控为主（多数手机；部分平板） */
export function isCoarsePointerPrimary(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false
  }
  try {
    return window.matchMedia('(hover: none) and (pointer: coarse)').matches
  } catch {
    return false
  }
}

/**
 * OS 种类。优先 userAgentData.platform，再 UA。
 * Android 必须在 Linux 之前匹配。
 */
export function detectOsKind(
  ua?: string,
  nav?: Navigator | null,
): OsKind {
  const n = getNav(nav)
  const u = getUa(ua, n)

  const uaDataPlatform =
    n &&
    'userAgentData' in n &&
    (n as Navigator & { userAgentData?: { platform?: string } }).userAgentData
      ?.platform

  const p = (uaDataPlatform || '').toLowerCase()
  if (p === 'android') return 'android'
  if (p === 'ios') return 'ios'
  if (p === 'macos') return 'macos'
  if (p === 'windows') return 'windows'
  if (p === 'linux') return 'linux'

  if (/Android/i.test(u)) return 'android'
  if (/iPhone|iPod/i.test(u)) return 'ios'
  if (/iPad/i.test(u)) return 'ios'
  if (isIpadOsDesktopUa(u, n)) return 'ios'
  if (/Mac OS X|Macintosh/i.test(u)) return 'macos'
  if (/Windows NT|Win64|WOW64|Windows /i.test(u)) return 'windows'
  if (/Linux/i.test(u)) return 'linux'

  return 'unknown'
}

/** 从 UA 解析 iOS / iPadOS 主版本 */
export function parseIosMajorVersion(ua?: string): number | null {
  const u = getUa(ua)
  const patterns = [
    /OS (\d+)[._](\d+)/i,
    /iPhone OS (\d+)/i,
    /CPU OS (\d+)/i,
  ]
  for (const re of patterns) {
    const m = u.match(re)
    if (m) {
      const major = Number.parseInt(m[1], 10)
      if (Number.isFinite(major)) return major
    }
  }
  return null
}

/**
 * Apple WebKit 引擎且非 Chromium（Safari / iOS 上多数浏览器壳）。
 * 注意：这是引擎判定，不是 OS（桌面 Safari 也是 true）。
 */
export function detectIsWebKitEngine(
  ua?: string,
  nav?: Navigator | null,
): boolean {
  const n = getNav(nav)
  if (!n) return false
  const u = getUa(ua, n)
  const uaIsWebKit = RE_APPLE_WEBKIT.test(u) && !RE_CHROMIUM.test(u)
  const isAppleVendor = n.vendor === 'Apple Computer, Inc.'
  return uaIsWebKit && isAppleVendor
}

/**
 * 模块加载时缓存的 WebKit 引擎标记（与历史 TappPageSandbox.isWebKit 行为一致）。
 */
export const isWebKit: boolean = (() => {
  if (typeof navigator === 'undefined') return false
  return detectIsWebKitEngine()
})()

/**
 * 是否应走原生 HTMLAudio（不经 Web Audio 图）—— iOS/Android 后台播放。
 *
 * **不要**仅凭 `(hover:none) and (pointer:coarse)` 判定：Windows 触控本 /
 * Surface 会误锁，导致 `createMediaElementSource` 永不接入 → 频谱全 0 →
 * 音乐播放器 Tapp 的 Aurora / 节奏涟漪等桌面动效全部静默。
 * 桌面 OS 一律允许 Web Audio 频谱（与后台策略无关）。
 */
export function shouldPreserveNativeAudioOutput(): boolean {
  if (typeof navigator === 'undefined' || typeof window === 'undefined') {
    return false
  }
  const os = detectOsKind()
  if (os === 'ios' || os === 'android') return true
  // 未知 UA 的粗指针设备（多数移动壳）仍保原生输出；桌面 OS 已在上方放过
  if (os === 'unknown' && isCoarsePointerPrimary()) return true
  return false
}
