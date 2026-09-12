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

export function isIpadOsDesktopUa(
  ua?: string,
  nav?: Navigator | null,
): boolean {
  const n = getNav(nav)
  const maxTouch =
    n && typeof n.maxTouchPoints === 'number' ? n.maxTouchPoints : 0
  if (maxTouch <= 1) return false
  if (n?.platform === 'MacIntel') return true
  const u = getUa(ua, n)
  if (/iPhone|iPod|iPad/i.test(u)) return false
  return /Macintosh|Mac OS X/i.test(u)
}

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

/** Match Android before Linux. */
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

export const isWebKit: boolean = (() => {
  if (typeof navigator === 'undefined') return false
  return detectIsWebKitEngine()
})()

/** Do not use (hover:none) and (pointer:coarse) alone (Surface false-positive; no Web Audio). */
export function shouldPreserveNativeAudioOutput(): boolean {
  if (typeof navigator === 'undefined' || typeof window === 'undefined') {
    return false
  }
  const os = detectOsKind()
  if (os === 'ios' || os === 'android') return true
  if (os === 'unknown' && isCoarsePointerPrimary()) return true
  return false
}
