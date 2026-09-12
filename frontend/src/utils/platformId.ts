const PLATFORM_NAME_TO_ID: Record<string, string> = {
  bilibili: 'bilibili',
  b站: 'bilibili',
  steam: 'steam',
  github: 'github',
  youtube: 'youtube',
  yt: 'youtube',
  bangumi: 'bangumi',
  bgm: 'bangumi',
  mal: 'mal',
  myanimelist: 'mal',
  'my anime list': 'mal',
  x: 'x',
  twitter: 'x',
  'x (twitter)': 'x',
  discord: 'discord',
  xbox: 'xbox',
  psn: 'psn',
  playstation: 'psn',
  'play station': 'psn',
  netease: 'netease',
  'netease music': 'netease',
  'netease cloud music': 'netease',
  'netease cloudmusic': 'netease',
  网易云: 'netease',
  网易云音乐: 'netease',
}

export const CANONICAL_PLATFORM_IDS = [
  'bilibili',
  'steam',
  'github',
  'youtube',
  'netease',
  'bangumi',
  'mal',
  'x',
  'discord',
  'xbox',
  'psn',
] as const

export function resolvePlatformId(platformName: string): string | null {
  const key = platformName.trim().toLowerCase().replaceAll(/\s+/g, ' ')
  if (!key) return null
  if (PLATFORM_NAME_TO_ID[key]) return PLATFORM_NAME_TO_ID[key]
  if ((CANONICAL_PLATFORM_IDS as readonly string[]).includes(key)) return key
  return null
}
