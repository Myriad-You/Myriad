export interface PlatformBrandTheme {
  color: string
  darkColor: string
}

const PLATFORM_BRANDS: Record<string, PlatformBrandTheme> = {
  github: { color: '#24292f', darkColor: '#e6edf3' },
  bilibili: { color: '#00A1D6', darkColor: '#00A1D6' },
  steam: { color: '#1b2838', darkColor: '#66c0f4' },
  youtube: { color: '#FF0000', darkColor: '#ff4d4d' },
  netease: { color: '#e60026', darkColor: '#ff4d67' },
  'netease music': { color: '#e60026', darkColor: '#ff4d67' },
  netease_music: { color: '#e60026', darkColor: '#ff4d67' },
  bangumi: { color: '#f09199', darkColor: '#f6a9b0' },
  mal: { color: '#2E51A2', darkColor: '#8ba6f8' },
  myanimelist: { color: '#2E51A2', darkColor: '#8ba6f8' },
  x: { color: '#0f1419', darkColor: '#e7e9ea' },
  twitter: { color: '#0f1419', darkColor: '#e7e9ea' },
  'x (twitter)': { color: '#0f1419', darkColor: '#e7e9ea' },
  discord: { color: '#5865F2', darkColor: '#8b9cff' },
  xbox: { color: '#107C10', darkColor: '#72d35c' },
  psn: { color: '#0070D1', darkColor: '#65b5ff' },
  playstation: { color: '#0070D1', darkColor: '#65b5ff' },
}

const FALLBACK: PlatformBrandTheme = {
  color: '#6b7280',
  darkColor: '#9ca3af',
}

function normalizePlatformKey(platform: string): string {
  return platform.trim().toLowerCase().replaceAll(/\s+/g, ' ')
}

export function getPlatformBrandTheme(platform: string): PlatformBrandTheme {
  const key = normalizePlatformKey(platform)
  if (PLATFORM_BRANDS[key]) return PLATFORM_BRANDS[key]
  const compact = key.replaceAll(/\s+/g, '_')
  if (PLATFORM_BRANDS[compact]) return PLATFORM_BRANDS[compact]
  return FALLBACK
}

export function getPlatformBrandColor(
  platform: string,
  isDark = false,
): string {
  const theme = getPlatformBrandTheme(platform)
  return isDark ? theme.darkColor : theme.color
}
