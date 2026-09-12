import { LEGACY_CONFIG_SECTION_MAP } from './defaults'

export const CONFIG_NAV_STORAGE_KEY = 'myriad_config_nav_v1'

export const CONFIG_NAV_SECTIONS = [
  'platforms',
  'ai',
  'tripo',
  'basic',
  'oauth',
  'federation',
  'permissions',
  'users',
  'notifications',
  'modules',
  'advanced',
  'about',
] as const

export interface ConfigNavPersisted {
  section: string
  mobilePane?: 'nav' | 'section'
  platformFocus?: string | null
  scrollY?: number
}

function isKnownSection(section: string, isAdmin: boolean): boolean {
  if (section === 'federation' && !isAdmin) return false
  return (CONFIG_NAV_SECTIONS as readonly string[]).includes(section)
}

export function normalizeConfigSection(
  raw: string | null | undefined,
  isAdmin: boolean,
): string | null {
  if (!raw) return null
  const next = LEGACY_CONFIG_SECTION_MAP[raw] ?? raw
  return isKnownSection(next, isAdmin) ? next : null
}

export function loadConfigNavPersisted(): ConfigNavPersisted | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = sessionStorage.getItem(CONFIG_NAV_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as Partial<ConfigNavPersisted>
    if (!parsed || typeof parsed.section !== 'string') return null
    return {
      section: parsed.section,
      mobilePane:
        parsed.mobilePane === 'nav' || parsed.mobilePane === 'section'
          ? parsed.mobilePane
          : undefined,
      platformFocus:
        parsed.platformFocus === null ||
        typeof parsed.platformFocus === 'string'
          ? parsed.platformFocus
          : undefined,
      scrollY:
        typeof parsed.scrollY === 'number' &&
        Number.isFinite(parsed.scrollY) &&
        parsed.scrollY >= 0
          ? parsed.scrollY
          : undefined,
    }
  } catch {
    return null
  }
}

export function saveConfigNavPersisted(
  patch: Partial<ConfigNavPersisted>,
): void {
  if (typeof window === 'undefined') return
  try {
    const prev = loadConfigNavPersisted() ?? { section: 'platforms' }
    const next: ConfigNavPersisted = {
      section: patch.section ?? prev.section,
      mobilePane: patch.mobilePane ?? prev.mobilePane,
      platformFocus:
        patch.platformFocus !== undefined
          ? patch.platformFocus
          : prev.platformFocus,
      scrollY: patch.scrollY ?? prev.scrollY,
    }
    sessionStorage.setItem(CONFIG_NAV_STORAGE_KEY, JSON.stringify(next))
  } catch {
    /* private mode / quota */
  }
}

export function snapshotConfigNavScroll(): void {
  if (typeof window === 'undefined') return
  saveConfigNavPersisted({ scrollY: window.scrollY || window.pageYOffset || 0 })
}

/** URL ?section=, else sessionStorage, else platforms */
export function resolveInitialConfigSection(isAdmin: boolean): string {
  if (typeof window === 'undefined') return 'platforms'
  try {
    const params = new URLSearchParams(window.location.search)
    const fromUrl = normalizeConfigSection(params.get('section'), isAdmin)
    if (fromUrl) return fromUrl
    const stored = loadConfigNavPersisted()
    const fromStore = normalizeConfigSection(stored?.section, isAdmin)
    if (fromStore) return fromStore
  } catch {
    /* ignore */
  }
  return 'platforms'
}

/** replaceState ?section=; keep other query */
export function syncConfigSectionToUrl(section: string): void {
  if (typeof window === 'undefined') return
  try {
    const url = new URL(window.location.href)
    if (url.searchParams.get('section') === section) return
    url.searchParams.set('section', section)
    if (section !== 'ai') url.searchParams.delete('page')
    window.history.replaceState(window.history.state, '', url.toString())
  } catch {
    /* ignore */
  }
}
