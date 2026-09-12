import { formatMessage, getDefaultLocale } from '../../../i18n'

export function formatCompactNumber(n: number | undefined | null): string {
  const num = Number(n) || 0
  if (num >= 1_000_000_000) return `${(num / 1_000_000_000).toFixed(1)}B`
  if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`
  if (num >= 1_000) return `${(num / 1_000).toFixed(1)}K`
  return String(num)
}

interface DiscordTakeLabels {
  discordRoleOwner: string
  discordRoleAdmin: string
  discordRoleMod: string
  discordFeaturePartner: string
  discordFeatureVerified: string
  discordTakeOwnServer: string
  discordTakeAdminSeat: string
  discordTakeModSeat: string
  discordTakeCommunityStay: string
  discordTakeMember: string
  discordTakeWithSize: string
  discordSizeHuge: string
  discordSizeLarge: string
  discordSizeMid: string
  discordSizeSmall: string
}

const DISCORD_SIZE_ALIASES: [string, keyof DiscordTakeLabels][] = [
  ['万人广场', 'discordSizeHuge'],
  ['万人级', 'discordSizeLarge'],
  ['千人圈', 'discordSizeMid'],
  ['小圈子', 'discordSizeSmall'],
  ['万人広場', 'discordSizeHuge'],
  ['万人級', 'discordSizeLarge'],
  ['千人圏', 'discordSizeMid'],
  ['小規模', 'discordSizeSmall'],
  ['100k+', 'discordSizeHuge'],
  ['10k+', 'discordSizeLarge'],
  ['1k+', 'discordSizeMid'],
  ['small', 'discordSizeSmall'],
]

const DISCORD_ROLE_PREFIXES: [string, keyof DiscordTakeLabels][] = [
  ['自建·', 'discordRoleOwner'],
  ['掌舵·', 'discordRoleAdmin'],
  ['协管·', 'discordRoleMod'],
  ['自作·', 'discordRoleOwner'],
  ['運営·', 'discordRoleAdmin'],
  ['モデ·', 'discordRoleMod'],
  ['Owner · ', 'discordRoleOwner'],
  ['Admin · ', 'discordRoleAdmin'],
  ['Mod · ', 'discordRoleMod'],
]

const DISCORD_MEMBER_PREFIXES = ['常驻·', '常駐·', 'Member · ']

const DISCORD_EXACT_TAKES: Record<string, keyof DiscordTakeLabels> = {
  自建领地: 'discordTakeOwnServer',
  自作サーバー: 'discordTakeOwnServer',
  'Own server': 'discordTakeOwnServer',
  管理席位: 'discordTakeAdminSeat',
  管理者: 'discordTakeAdminSeat',
  'Admin seat': 'discordTakeAdminSeat',
  协管席位: 'discordTakeModSeat',
  モデ席: 'discordTakeModSeat',
  'Mod seat': 'discordTakeModSeat',
  官方合作服: 'discordFeaturePartner',
  公式提携: 'discordFeaturePartner',
  Partnered: 'discordFeaturePartner',
  认证大服: 'discordFeatureVerified',
  認証サーバー: 'discordFeatureVerified',
  Verified: 'discordFeatureVerified',
  社区服常驻: 'discordTakeCommunityStay',
  コミュニティ常駐: 'discordTakeCommunityStay',
  'Community stay': 'discordTakeCommunityStay',
  社区成员: 'discordTakeMember',
  メンバー: 'discordTakeMember',
  Member: 'discordTakeMember',
}

/** Remap leftover Discord fallback takes so a language switch does not keep 自建·万人级. */
export function localizeDiscordGuildTake(
  take: string,
  labels: DiscordTakeLabels,
): string {
  const raw = take.trim()
  if (!raw) return take
  const exact = DISCORD_EXACT_TAKES[raw]
  if (exact) return labels[exact]
  for (const prefix of DISCORD_MEMBER_PREFIXES) {
    if (!raw.startsWith(prefix)) continue
    const sizeRaw = raw.slice(prefix.length).trim()
    const sizeKey = DISCORD_SIZE_ALIASES.find(([alias]) => alias === sizeRaw)?.[1]
    if (!sizeKey) continue
    return formatMessage(getDefaultLocale(), labels.discordTakeWithSize, {
      role: labels.discordTakeMember,
      size: labels[sizeKey],
    })
  }
  for (const [prefix, roleKey] of DISCORD_ROLE_PREFIXES) {
    if (!raw.startsWith(prefix)) continue
    const sizeRaw = raw.slice(prefix.length).trim()
    const sizeKey = DISCORD_SIZE_ALIASES.find(([alias]) => alias === sizeRaw)?.[1]
    if (!sizeKey) continue
    return formatMessage(getDefaultLocale(), labels.discordTakeWithSize, {
      role: labels[roleKey],
      size: labels[sizeKey],
    })
  }
  return take
}

export function formatYoutubeDuration(
  iso?: string | null,
): string | null {
  if (!iso || typeof iso !== 'string') return null
  const m = iso.trim().match(/^PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?$/i)
  if (!m) return null
  const h = Number(m[1] || 0)
  const min = Number(m[2] || 0)
  const s = Number(m[3] || 0)
  if (!Number.isFinite(h + min + s) || (h === 0 && min === 0 && s === 0)) {
    return null
  }
  if (h > 0) {
    return `${h}:${String(min).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${min}:${String(s).padStart(2, '0')}`
}
