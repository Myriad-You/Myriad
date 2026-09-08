export function isChannelPairingProvider(
  provider: string | null | undefined,
): boolean {
  const key = provider?.trim().toLowerCase()
  return key === 'qq' || key === 'telegram' || key === 'discord_dm'
}

export function pairingCodeLive(
  expiresAt: string | null | undefined,
  now = Date.now(),
): boolean {
  if (!expiresAt) return false
  const at = Date.parse(expiresAt)
  return Number.isFinite(at) && at > now
}

export function telegramOpenHref(username?: string | null): string | null {
  const handle = username?.trim().replace(/^@/, '')
  if (!handle) return null
  return `https://t.me/${encodeURIComponent(handle)}`
}

export function discordOpenHref(userId?: string | null): string | null {
  const id = userId?.trim()
  if (!id) return null
  return `https://discord.com/users/${encodeURIComponent(id)}`
}

export function formatInboundTime(
  iso: string | null | undefined,
  locale: string,
): string | null {
  if (!iso) return null
  const at = Date.parse(iso)
  if (!Number.isFinite(at)) return null
  return new Date(at).toLocaleString(locale, {
    hour: '2-digit',
    minute: '2-digit',
  })
}
