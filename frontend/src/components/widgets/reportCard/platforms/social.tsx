import { FaXTwitter, SiDiscord } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { extractColorsFromLoadedImage } from '../../../../utils/colorExtractor'
import {
  CONTENT_FADE_ANIMATE,
  CONTENT_FADE_EXIT,
  CONTENT_FADE_INITIAL,
  CONTENT_FADE_TRANSITION,
} from '../animations'
import { formatCompactNumber, localizeDiscordGuildTake } from '../format'

const X_CIRCLE_COLOR_CLASSES = [
  'bg-[#2563eb] dark:bg-[#3b82f6]',
  'bg-[#d97706]',
  'bg-[#7c3aed] dark:bg-[#8b5cf6]',
  'bg-[#db2777] dark:bg-[#ec4899]',
]

export const XWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const { t } = useI18n()
  const stats = data?.stats || {}
  const followingSample = useMemo(
    () => (Array.isArray(data?.following_sample) ? data.following_sample : []),
    [data?.following_sample],
  )
  const highlights = useMemo(() => {
    const list = Array.isArray(data?.following_highlights)
      ? data.following_highlights
      : []
    const sampleMap = new Map(
      followingSample.map((f: any) => [
        String(f.username || '').toLowerCase(),
        f,
      ]),
    )
    const enriched = list
      .filter((h: any) => h?.username || h?.name)
      .slice(0, 5)
      .map((h: any) => {
        const sample: any =
          sampleMap.get(String(h.username || '').toLowerCase()) || {}
        return {
          ...h,
          avatar: sample.avatar,
          description: sample.description,
          follower_count: sample.follower_count,
        }
      })
    if (enriched.length > 0) return enriched
    return followingSample
      .filter((f: any) => String(f.description || '').trim())
      .toSorted(
        (a: any, b: any) =>
          (Number(a.follower_count) || 0) - (Number(b.follower_count) || 0),
      )
      .slice(0, 5)
      .map((f: any) => ({
        username: f.username,
        name: f.name,
        avatar: f.avatar,
        description: f.description,
        follower_count: f.follower_count,
      }))
  }, [data?.following_highlights, followingSample])

  const circles = useMemo(() => {
    const list = Array.isArray(data?.interest_circles)
      ? data.interest_circles
      : []
    const cleaned = list
      .filter((c: any) => c?.name && Number(c?.count) > 0)
      .slice(0, X_CIRCLE_COLOR_CLASSES.length)
    const total = cleaned.reduce(
      (sum: number, c: any) => sum + Number(c.count),
      0,
    )
    if (total === 0) return []
    return cleaned.map((c: any, i: number) => {
      const rawName = String(c.name)
      return {
        name: rawName.length > 7 ? `${rawName.slice(0, 6)}…` : rawName,
        count: Number(c.count),
        pct: (Number(c.count) / total) * 100,
        colorClass: X_CIRCLE_COLOR_CLASSES[i],
      }
    })
  }, [data?.interest_circles])

  const wallAvatars = useMemo(() => {
    const withAvatar = followingSample.filter((f: any) => f.avatar)
    const rank = new Map<string, number>()
    const addPreferred = (username: unknown) => {
      const key = String(username || '').toLowerCase()
      if (key && !rank.has(key)) rank.set(key, rank.size)
    }
    if (Array.isArray(data?.following_highlights)) {
      for (const h of data.following_highlights) addPreferred(h?.username)
    }
    if (Array.isArray(data?.interest_circles)) {
      for (const c of data.interest_circles) {
        if (Array.isArray(c?.accounts)) c.accounts.forEach(addPreferred)
      }
    }
    const keyOf = (f: any) => String(f.username || '').toLowerCase()
    const curated = Iterator.from(withAvatar)
      .filter((f: any) => rank.has(keyOf(f)))
      .toArray()
      .toSorted((a: any, b: any) => rank.get(keyOf(a))! - rank.get(keyOf(b))!)
    const rest = Iterator.from(withAvatar)
      .filter((f: any) => !rank.has(keyOf(f)))
      .toArray()
    return [...curated, ...rest].slice(0, 7)
  }, [followingSample, data?.following_highlights, data?.interest_circles])

  const [slideIndex, setSlideIndex] = useState(0)
  const [tints, setTints] = useState<Record<string, string>>({})
  const tweetItems = useMemo(() => {
    const fromLib = Array.isArray(data?.library_items) ? data.library_items : []
    const fromTop = Array.isArray(data?.top_posts) ? data.top_posts : []
    const raw = fromLib.length > 0 ? fromLib : fromTop
    return raw
      .filter((p: any) => p?.title || p?.text)
      .slice(0, 8)
      .map((p: any) => ({
        kind: 'tweet' as const,
        title: String(p.title || p.text || '').slice(0, 120),
        text: String(p.text || p.title || ''),
        like_count: p.like_count,
        retweet_count: p.retweet_count,
      }))
  }, [data?.library_items, data?.top_posts])
  const flipMode: 'following' | 'tweets' =
    highlights.length > 0 ? 'following' : tweetItems.length > 0 ? 'tweets' : 'following'
  const flipItems = flipMode === 'tweets' ? tweetItems : highlights

  useEffect(() => {
    if (!showOverview && flipItems.length > 1) {
      const timer = setInterval(() => {
        setSlideIndex((i) => (i + 1) % flipItems.length)
      }, 4000)
      return () => clearInterval(timer)
    }
  }, [showOverview, flipItems.length])

  useEffect(() => {
    if (!showOverview && flipItems[slideIndex % flipItems.length]) {
      const item = flipItems[slideIndex % flipItems.length] as any
      if (item.kind === 'tweet') {
        onContentChange?.({
          title: String(item.title || '').slice(0, 40),
        })
      } else {
        const titles = [String(item.name || item.username || '')]
        if (item.username) titles.push(`@${item.username}`)
        onContentChange?.({ titles })
      }
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, slideIndex, flipItems, onContentChange])

  if (showOverview) {
    const profile = data?.profile || {}
    const statsParts = (
      [
        [stats.following, t.reportCardWidget.xFollowing],
        [stats.followers, t.reportCardWidget.xFollowers],
        [stats.posts, t.reportCardWidget.xPosts],
      ] as [number | null, string][]
    ).filter(([value]) => value != null)
    return (
      <div className="relative h-full w-full overflow-hidden">
        <div className="absolute inset-0 bg-linear-to-br from-gray-200/50 to-transparent dark:from-white/[0.06] dark:to-transparent" />
        {wallAvatars.length > 0 && (
          <div
            className="absolute inset-y-0 right-0 w-[55%] opacity-80 dark:opacity-60"
            style={{
              maskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
              WebkitMaskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
            }}
          >
            <div className="absolute inset-y-0 left-0 right-0 flex items-start pt-[44px] justify-end pr-3 rotate-6">
              {wallAvatars.map((f: any, i: number) => (
                <motion.div
                  key={f.username || i}
                  className="w-9 h-9 shrink-0 -ml-2 rounded-full overflow-hidden shadow-md ring-2 ring-white/80 dark:ring-black/60"
                  style={{ y: i % 2 === 0 ? -8 : 10 }}
                  initial={{ x: 40, opacity: 0 }}
                  animate={{ x: 0, opacity: 1 }}
                  transition={{
                    duration: 0.45,
                    delay: 0.15 + i * 0.06,
                    ease: 'easeOut',
                  }}
                >
                  <img
                    src={f.avatar}
                    alt={f.name || f.username}
                    className="w-full h-full object-cover"
                    loading="lazy"
                    referrerPolicy="no-referrer"
                  />
                </motion.div>
              ))}
            </div>
          </div>
        )}
        <div className="relative z-10 h-full flex flex-col p-3 pb-9">
          {(profile.avatar || profile.name || profile.username) && (
            <motion.div
              className="flex items-center gap-2 min-w-0 max-w-[70%]"
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              {profile.avatar && (
                <img
                  src={profile.avatar}
                  alt={profile.name || profile.username}
                  className="w-8 h-8 rounded-full object-cover ring-2 ring-white/80 dark:ring-black/50 shadow-sm shrink-0"
                  loading="lazy"
                  referrerPolicy="no-referrer"
                />
              )}
              <div className="min-w-0">
                <div className="text-[11px] font-bold text-gray-900 dark:text-gray-100 leading-tight truncate">
                  {profile.name || profile.username}
                </div>
                {profile.username && (
                  <div className="text-[9px] font-mono text-gray-500 dark:text-gray-400 truncate">
                    @{profile.username}
                  </div>
                )}
              </div>
            </motion.div>
          )}
          {(data?.vibe || data?.engagement_level) && (
            <div className="flex-1 min-h-0 flex items-center pt-2 pb-4">
              <motion.div
                className="max-w-[68%]"
                initial={{ y: 8, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                transition={{ duration: 0.4, delay: 0.2 }}
              >
                <span className="block text-[17px] font-black text-gray-900 dark:text-gray-100 leading-snug line-clamp-2 text-balance">
                  {data?.vibe || data?.engagement_level}
                </span>
              </motion.div>
            </div>
          )}
          {statsParts.length > 0 && (
            <motion.div
              className="absolute top-3 right-3 flex items-baseline gap-2"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.3 }}
            >
              {statsParts.map(([value, label]) => (
                <span key={label} className="flex items-baseline gap-0.5">
                  <span className="text-[10px] font-black tabular-nums text-gray-900 dark:text-gray-100">
                    {formatCompactNumber(value)}
                  </span>
                  <span className="text-[9px] font-medium text-gray-500 dark:text-gray-400">
                    {label}
                  </span>
                </span>
              ))}
            </motion.div>
          )}
          {circles.length > 0 ? (
            <div className="absolute bottom-3 right-3 w-[48%] flex flex-col items-end gap-1">
              <div className="flex flex-wrap justify-end gap-x-2.5 gap-y-0.5 max-h-[26px] overflow-hidden">
                {circles.map((circle: any, i: number) => (
                  <motion.span
                    key={circle.name}
                    className="flex items-center gap-1 text-[8px] font-bold text-gray-600 dark:text-gray-300"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    transition={{ duration: 0.3, delay: 0.35 + i * 0.12 }}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${circle.colorClass}`}
                    />
                    {circle.name}
                    <span className="font-mono text-gray-500 dark:text-gray-400">
                      {circle.count}
                    </span>
                  </motion.span>
                ))}
              </div>
              <div className="flex gap-[2px] h-1.5 w-full rounded-full overflow-hidden bg-gray-200/80 dark:bg-white/10 ring-1 ring-black/5 dark:ring-white/10">
                {circles.map((circle: any, i: number) => (
                  <motion.div
                    key={circle.name}
                    className={`h-full rounded-[2px] ${circle.colorClass}`}
                    initial={{ width: 0 }}
                    animate={{ width: `${circle.pct}%` }}
                    transition={{
                      duration: 0.35,
                      delay: 0.35 + i * 0.12,
                      ease: 'easeOut',
                    }}
                  />
                ))}
              </div>
            </div>
          ) : (
            Array.isArray(data?.signature_topics) &&
            data.signature_topics.length > 0 && (
              <div className="absolute bottom-3 right-3 max-w-[55%] flex flex-wrap justify-end gap-1">
                {data.signature_topics.slice(0, 4).map((topic: string) => (
                  <span
                    key={topic}
                    className="text-[9px] px-1.5 py-0.5 rounded-full bg-black/5 dark:bg-white/10 text-gray-700 dark:text-gray-300"
                  >
                    {topic}
                  </span>
                ))}
              </div>
            )
          )}
        </div>
      </div>
    )
  }

  const item =
    flipItems.length > 0 ? flipItems[slideIndex % flipItems.length] : null
  if (!item) {
    return (
      <div className="h-full w-full flex items-center justify-center text-gray-400 text-xs">
        <FaXTwitter />
      </div>
    )
  }

  if ((item as any).kind === 'tweet') {
    const tweet = item as {
      title: string
      text: string
      like_count?: number
      retweet_count?: number
    }
    return (
      <AnimatePresence mode="wait">
        <motion.div
          key={`tw-${slideIndex % flipItems.length}`}
          initial={CONTENT_FADE_INITIAL}
          animate={CONTENT_FADE_ANIMATE}
          exit={CONTENT_FADE_EXIT}
          transition={CONTENT_FADE_TRANSITION}
          className="relative h-full w-full overflow-hidden p-3 pb-10 flex flex-col"
        >
          <div className="absolute inset-0 bg-linear-to-br from-gray-200/40 to-transparent dark:from-white/[0.05] dark:to-transparent" />
          <p className="relative z-10 text-[12px] font-semibold leading-snug text-gray-900 dark:text-gray-100 line-clamp-5">
            {tweet.text || tweet.title}
          </p>
          <div className="relative z-10 mt-auto flex items-center gap-3 text-[9px] font-medium text-gray-500 dark:text-gray-400">
            {tweet.like_count != null && (
              <span>♥ {formatCompactNumber(tweet.like_count)}</span>
            )}
            {tweet.retweet_count != null && (
              <span>↻ {formatCompactNumber(tweet.retweet_count)}</span>
            )}
          </div>
          {flipItems.length > 1 && (
            <div className="absolute bottom-3 right-3 z-10 flex gap-1">
              {flipItems.map((_: any, i: number) => (
                <span
                  key={i}
                  className={`w-1 h-1 rounded-full transition-colors ${
                    i === slideIndex % flipItems.length
                      ? 'bg-gray-800 dark:bg-white/90'
                      : 'bg-gray-400/60 dark:bg-white/30'
                  }`}
                />
              ))}
            </div>
          )}
        </motion.div>
      </AnimatePresence>
    )
  }

  const tint = tints[String((item as any).username || '')]
  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={`hl-${slideIndex % flipItems.length}`}
        initial={CONTENT_FADE_INITIAL}
        animate={CONTENT_FADE_ANIMATE}
        exit={CONTENT_FADE_EXIT}
        transition={CONTENT_FADE_TRANSITION}
        className="relative h-full w-full overflow-hidden"
      >
        {tint && (
          <div
            className="absolute inset-0"
            style={{
              background: `radial-gradient(circle at 74% 50%, ${tint}30, transparent 75%)`,
            }}
          />
        )}
        {(item as any).avatar && (
          <motion.div
            className="absolute inset-y-0 right-0 w-[58%]"
            style={{
              maskImage:
                'radial-gradient(circle at 100% 50%, rgba(0,0,0,1) 42%, transparent 74%)',
              WebkitMaskImage:
                'radial-gradient(circle at 100% 50%, rgba(0,0,0,1) 42%, transparent 74%)',
            }}
            initial={{ opacity: 0, x: 14 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.6, ease: 'easeOut' }}
          >
            <img
              src={
                String((item as any).avatar).includes('/api/proxy/image')
                  ? (item as any).avatar
                  : String((item as any).avatar).replaceAll(
                      /_(normal|bigger)\./g,
                      '_400x400.',
                    )
              }
              alt={(item as any).name || (item as any).username}
              className="w-full h-full object-cover translate-x-[6%] scale-110"
              loading="lazy"
              referrerPolicy="no-referrer"
              onLoad={(e) => {
                const username = String((item as any).username || '')
                if (!username || tints[username]) return
                try {
                  const palette = extractColorsFromLoadedImage(e.currentTarget)
                  if (palette?.primary) {
                    setTints((prev) => ({
                      ...prev,
                      [username]: palette.primary,
                    }))
                  }
                } catch {
                }
              }}
            />
          </motion.div>
        )}
        <div className="relative z-10 h-full p-3 pb-12 flex flex-col">
          {(item as any).tag && (
            <motion.div
              className="min-w-0 max-w-[70%]"
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              <span className="block text-base font-black text-gray-900 dark:text-gray-100 leading-tight truncate">
                {(item as any).tag}
              </span>
            </motion.div>
          )}
          {(item as any).follower_count != null && (
            <motion.div
              className="mt-0.5 text-[10px] font-medium text-gray-500 dark:text-gray-400"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.2 }}
            >
              {formatCompactNumber((item as any).follower_count)}{' '}
              {t.reportCardWidget.xFollowers}
            </motion.div>
          )}
          {(item as any).description && (
            <div className="mt-1.5 flex-1 min-h-0 overflow-hidden max-w-[58%]">
              <p className="text-[9px] leading-relaxed text-gray-600 dark:text-gray-400 line-clamp-2">
                {(item as any).description}
              </p>
            </div>
          )}
        </div>
        {flipItems.length > 1 && (
          <div className="absolute bottom-3 right-3 z-10 flex gap-1">
            {flipItems.map((_: any, i: number) => (
              <span
                key={i}
                className={`w-1 h-1 rounded-full transition-colors ${
                  i === slideIndex % flipItems.length
                    ? 'bg-gray-800 dark:bg-white/90'
                    : 'bg-gray-400/60 dark:bg-white/30'
                }`}
              />
            ))}
          </div>
        )}
      </motion.div>
    </AnimatePresence>
  )
})

const DISCORD_BLURPLE = '#5865F2'

const DISCORD_TILE_COLORS = [
  '#5865F2',
  '#7289DA',
  '#4752C4',
  '#949CF7',
  '#3C45A5',
]
function discordTileColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) & 0xFFFFFFFF
  }
  return DISCORD_TILE_COLORS[Math.abs(hash) % DISCORD_TILE_COLORS.length]
}

function discordGuildBadge(
  g: any,
  t: any,
): { label: string; color: string } | null {
  const perms: string[] = Array.isArray(g?.permissions) ? g.permissions : []
  const features: string[] = Array.isArray(g?.features) ? g.features : []
  if (g?.owner) return { label: t.reportCardWidget.discordRoleOwner, color: '#F0B232' }
  if (perms.includes('ADMINISTRATOR'))
    return { label: t.reportCardWidget.discordRoleAdmin, color: '#5865F2' }
  if (perms.includes('MANAGE_GUILD'))
    return { label: t.reportCardWidget.discordRoleMod, color: '#3BA55D' }
  if (features.includes('PARTNERED'))
    return { label: t.reportCardWidget.discordFeaturePartner, color: '#5865F2' }
  if (features.includes('VERIFIED'))
    return { label: t.reportCardWidget.discordFeatureVerified, color: '#3BA55D' }
  if (features.includes('COMMUNITY'))
    return { label: t.reportCardWidget.discordFeatureCommunity, color: '#949CF7' }
  return null
}

function DiscordGuildIcon({
  icon,
  name,
  size,
}: {
  icon?: string | null
  name: string
  size: number
}) {
  const [failed, setFailed] = useState(false)
  if (icon && !failed) {
    return (
      <img
        src={icon}
        alt={name}
        className="w-full h-full object-cover"
        loading="lazy"
        referrerPolicy="no-referrer"
        onError={() => setFailed(true)}
      />
    )
  }
  const letter = Iterator.from(name.trim()).toArray()[0] || '#'
  return (
    <div
      className="w-full h-full flex items-center justify-center font-bold text-white"
      style={{
        background: discordTileColor(name),
        fontSize: Math.round(size * 0.42),
      }}
    >
      {letter}
    </div>
  )
}

export const DiscordWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const { t } = useI18n()
  const profile = data?.profile || {}
  const stats = data?.stats || {}
  const guilds = useMemo(() => {
    if (Array.isArray(data?.library_items) && data.library_items.length > 0) {
      return data.library_items
    }
    if (Array.isArray(data?.guilds_preview) && data.guilds_preview.length > 0) {
      return data.guilds_preview.map((g: any) => ({
        id: g.id,
        title: g.name || g.title,
        name: g.name || g.title,
        icon: g.icon || g.icon_url,
        owner: g.owner,
        permissions: g.permissions || g.permissions_highlight,
        member_count: g.member_count,
        presence_count: g.presence_count,
        features: g.features || g.feature_highlight,
      }))
    }
    return []
  }, [data?.library_items, data?.guilds_preview])
  const badges: string[] = useMemo(
    () => (Array.isArray(profile.badges) ? profile.badges.slice(0, 3) : []),
    [profile.badges],
  )
  const tags: string[] = useMemo(() => {
    const ct = Array.isArray(data?.community_tags) ? data.community_tags : []
    if (ct.length > 0) return ct.slice(0, 3)
    const lp = Array.isArray(data?.linked_platforms) ? data.linked_platforms : []
    return lp.slice(0, 3)
  }, [data?.community_tags, data?.linked_platforms])

  const iconWall = useMemo(() => guilds.slice(0, 7), [guilds])
  const flipItems = useMemo(() => guilds.slice(0, 8), [guilds])
  const guildTakeByKey = useMemo(() => {
    const map = new Map<string, string>()
    const raw = Array.isArray(data?.guild_takes) ? data.guild_takes : []
    const labels = t.reportCardWidget
    for (const entry of raw) {
      if (!entry || typeof entry !== 'object') continue
      const take = localizeDiscordGuildTake(
        String((entry as any).take || '').trim(),
        labels,
      )
      if (!take) continue
      const id = (entry as any).id
      if (id != null && String(id)) map.set(`id:${String(id)}`, take)
      const name = (entry as any).name
      if (name != null && String(name)) {
        map.set(`name:${String(name).toLowerCase()}`, take)
      }
    }
    return map
  }, [data?.guild_takes, t.reportCardWidget])

  const [slideIndex, setSlideIndex] = useState(0)
  useEffect(() => {
    if (!showOverview && flipItems.length > 1) {
      const timer = setInterval(() => {
        setSlideIndex((i) => (i + 1) % flipItems.length)
      }, 4000)
      return () => clearInterval(timer)
    }
  }, [showOverview, flipItems.length])

  const hasHandle = Boolean(profile.username || profile.nitro)
  const headerSlides = (hasHandle ? 1 : 0) + badges.length
  const [headerIdx, setHeaderIdx] = useState(0)
  useEffect(() => {
    if (showOverview && headerSlides > 1) {
      const timer = setInterval(() => {
        setHeaderIdx((i) => (i + 1) % headerSlides)
      }, 3200)
      return () => clearInterval(timer)
    }
  }, [showOverview, headerSlides])

  useEffect(() => {
    if (!showOverview && flipItems[slideIndex % flipItems.length]) {
      const g = flipItems[slideIndex % flipItems.length]
      onContentChange?.({ title: String(g.name || g.title || '') })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, slideIndex, flipItems, onContentChange])

  if (showOverview || flipItems.length === 0) {
    const displayName =
      profile.display_name || profile.username || 'Discord'
    const guildCount =
      Number(stats.guilds) > 0
        ? Number(stats.guilds)
        : guilds.length > 0
          ? guilds.length
          : 0
    const statsParts = (
      [
        [guildCount, t.reportCardWidget.discordGuilds],
        [Number(stats.connections) || 0, t.reportCardWidget.discordConnections],
      ] as [number, string][]
    ).filter(([value]) => value > 0)

    return (
      <div className="relative h-full w-full overflow-hidden">
        <div className="absolute inset-0 bg-linear-to-br from-[#5865F2]/10 to-transparent dark:from-[#5865F2]/[0.14] dark:to-transparent" />
        {iconWall.length > 0 && (
          <div
            className="absolute inset-y-0 right-0 w-[55%] opacity-80 dark:opacity-70"
            style={{
              maskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
              WebkitMaskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
            }}
          >
            <div className="absolute inset-y-0 left-0 right-0 flex items-center justify-end pr-3 -rotate-6">
              {iconWall.map((g: any, i: number) => (
                <motion.div
                  key={g.id || i}
                  className="w-10 h-10 shrink-0 -ml-2 rounded-lg overflow-hidden shadow-md ring-2 ring-white/80 dark:ring-black/60"
                  style={{ y: i % 2 === 0 ? -9 : 11 }}
                  initial={{ x: 40, opacity: 0 }}
                  animate={{ x: 0, opacity: 1 }}
                  transition={{
                    duration: 0.45,
                    delay: 0.15 + i * 0.06,
                    ease: 'easeOut',
                  }}
                >
                  <DiscordGuildIcon
                    icon={g.icon}
                    name={String(g.name || g.title || '?')}
                    size={40}
                  />
                </motion.div>
              ))}
            </div>
          </div>
        )}
        <div className="relative z-10 h-full flex flex-col p-3 pb-9">
          <motion.div
            className="flex items-center gap-2 min-w-0 max-w-[72%]"
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.1 }}
          >
            {profile.avatar_url ? (
              <img
                src={profile.avatar_url}
                alt={displayName}
                className="w-8 h-8 rounded-full object-cover ring-2 ring-white/80 dark:ring-black/50 shadow-sm shrink-0 self-center"
                loading="lazy"
                referrerPolicy="no-referrer"
              />
            ) : (
              <div
                className="w-8 h-8 rounded-full flex items-center justify-center text-white text-sm font-bold shrink-0 ring-2 ring-white/80 dark:ring-black/50 self-center"
                style={{ background: DISCORD_BLURPLE }}
              >
                {Iterator.from(String(displayName).trim()).toArray()[0] || '#'}
              </div>
            )}
            <div
              className={`min-w-0 flex flex-col justify-center ${
                headerSlides > 0 ? 'min-h-8' : ''
              }`}
            >
              <div className="h-[15px] text-[11px] font-bold text-gray-900 dark:text-gray-100 leading-[15px] truncate">
                {displayName}
              </div>
              {headerSlides > 0 && (
                <div className="relative h-[15px] shrink-0 overflow-hidden">
                  <AnimatePresence mode="wait" initial={false}>
                    <motion.div
                      key={headerIdx % headerSlides}
                      className="flex h-full items-center gap-1 min-w-0"
                      initial={{ y: 8, opacity: 0 }}
                      animate={{ y: 0, opacity: 1 }}
                      exit={{ y: -8, opacity: 0 }}
                      transition={{ duration: 0.28, ease: 'easeOut' }}
                    >
                      {hasHandle && headerIdx % headerSlides === 0 ? (
                        <>
                          {profile.username && (
                            <span className="text-[9px] font-mono text-gray-500 dark:text-gray-400 truncate">
                              @{profile.username}
                            </span>
                          )}
                          {profile.nitro && (
                            <span className="text-[8px] px-1 rounded bg-[#5865F2]/15 text-[#5865F2] font-bold shrink-0">
                              {profile.nitro}
                            </span>
                          )}
                        </>
                      ) : (
                        <span className="text-[9px] font-medium text-[#5865F2] dark:text-[#949CF7] truncate">
                          {
                            badges[
                              (headerIdx % headerSlides) - (hasHandle ? 1 : 0)
                            ]
                          }
                        </span>
                      )}
                    </motion.div>
                  </AnimatePresence>
                </div>
              )}
            </div>
          </motion.div>

          {(data?.vibe || data?.role_profile) && (
            <div className="flex-1 min-h-0 flex items-center pt-1.5 pb-4">
              <motion.div
                className="max-w-[68%]"
                initial={{ y: 8, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                transition={{ duration: 0.4, delay: 0.2 }}
              >
                <span className="block text-[16px] font-black text-gray-900 dark:text-gray-100 leading-snug line-clamp-2 text-balance">
                  {data?.vibe || data?.role_profile}
                </span>
              </motion.div>
            </div>
          )}

          {statsParts.length > 0 && (
            <motion.div
              className="absolute top-3 right-3 flex items-baseline gap-2"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.3 }}
            >
              {statsParts.map(([value, label]) => (
                <span key={label} className="flex items-baseline gap-0.5">
                  <span className="text-[10px] font-black tabular-nums text-gray-900 dark:text-gray-100">
                    {formatCompactNumber(value)}
                  </span>
                  <span className="text-[9px] font-medium text-gray-500 dark:text-gray-400">
                    {label}
                  </span>
                </span>
              ))}
            </motion.div>
          )}

          {tags.length > 0 && (
            <div className="absolute bottom-3 right-3 max-w-[55%] flex flex-nowrap justify-end gap-1 overflow-hidden">
              {tags.map((tag) => (
                <span
                  key={tag}
                  className="text-[9px] px-1.5 py-0.5 rounded-full bg-[#5865F2]/12 dark:bg-[#5865F2]/20 text-[#4752C4] dark:text-[#949CF7] font-medium shrink-0 max-w-[5.5rem] truncate"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      </div>
    )
  }

  const item =
    flipItems.length > 0 ? flipItems[slideIndex % flipItems.length] : null
  if (!item) {
    return (
      <div className="h-full w-full flex items-center justify-center text-gray-400 text-xl">
        <SiDiscord />
      </div>
    )
  }
  const badge = discordGuildBadge(item, t)
  const guildName = String(item.name || item.title || '')
  const guildTake =
    (item.id != null && guildTakeByKey.get(`id:${String(item.id)}`)) ||
    (guildName
      ? guildTakeByKey.get(`name:${guildName.toLowerCase()}`)
      : undefined) ||
    ''
  const detailHeadline = guildTake || guildName || '—'

  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={`dg-${slideIndex % flipItems.length}`}
        initial={CONTENT_FADE_INITIAL}
        animate={CONTENT_FADE_ANIMATE}
        exit={CONTENT_FADE_EXIT}
        transition={CONTENT_FADE_TRANSITION}
        className="relative h-full w-full overflow-hidden"
      >
        <motion.div
          className="absolute inset-y-0 right-0 flex items-center pr-4"
          initial={{ opacity: 0, x: 14 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.6, ease: 'easeOut' }}
        >
          <div className="h-[52%] max-h-28 aspect-square rounded-lg overflow-hidden shadow-xl">
            <DiscordGuildIcon icon={item.icon} name={guildName} size={96} />
          </div>
        </motion.div>
        <div className="relative z-10 h-full p-3 pb-12 flex flex-col justify-center">
          {badge && (
            <motion.span
              className="self-start text-[10px] font-bold px-2 py-0.5 rounded-full text-white shadow-sm"
              style={{ background: badge.color }}
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              {badge.label}
            </motion.span>
          )}
          <motion.div
            className="mt-2 min-w-0 max-w-[62%]"
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.15 }}
          >
            <span
              className={`block text-base font-black leading-tight line-clamp-2 ${
                guildTake
                  ? 'text-gray-900 dark:text-gray-100'
                  : 'text-gray-500 dark:text-gray-400'
              }`}
            >
              {detailHeadline}
            </span>
          </motion.div>
          {(Number(item.member_count) > 0 ||
            Number(item.presence_count) > 0) && (
            <motion.div
              className="mt-1 flex flex-wrap items-baseline gap-x-3 gap-y-0.5 text-[10px] text-gray-500 dark:text-gray-400"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.22 }}
            >
              {Number(item.member_count) > 0 && (
                <span className="inline-flex items-baseline gap-1">
                  <span className="font-bold text-gray-700 dark:text-gray-200 tabular-nums">
                    {formatCompactNumber(Number(item.member_count))}
                  </span>
                  <span>{t.reportCardWidget.discordMembers}</span>
                </span>
              )}
              {Number(item.presence_count) > 0 && (
                <span className="inline-flex items-baseline gap-1">
                  <span className="font-bold text-emerald-600 dark:text-emerald-400 tabular-nums">
                    {formatCompactNumber(Number(item.presence_count))}
                  </span>
                  <span>{t.reportCardWidget.discordOnline}</span>
                </span>
              )}
            </motion.div>
          )}
        </div>
        {flipItems.length > 1 && (
          <div className="absolute bottom-3 right-3 z-10 flex gap-1">
            {flipItems.map((_: any, i: number) => (
              <span
                key={i}
                className={`w-1 h-1 rounded-full transition-colors ${
                  i === slideIndex % flipItems.length
                    ? 'bg-[#5865F2] dark:bg-[#949CF7]'
                    : 'bg-gray-400/60 dark:bg-white/30'
                }`}
              />
            ))}
          </div>
        )}
      </motion.div>
    </AnimatePresence>
  )
})
