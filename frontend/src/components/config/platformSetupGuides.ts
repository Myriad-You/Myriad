import type { SetupFlowStep } from '../settings/SetupFlow'

/** i18n slice; don't import full I18n */
export interface PlatformSetupI18n {
  platformSetupTitle: string
  platformSetupOptional: string
  platformSetupOpen: string
  platformSetupFillTitle: string
  platformSetupFillDesc: string

  platformSetupGithub1Title: string
  platformSetupGithub1Desc: string
  platformSetupGithub2Title: string
  platformSetupGithub2Desc: string

  platformSetupBilibili1Title: string
  platformSetupBilibili1Desc: string

  platformSetupSteam1Title: string
  platformSetupSteam1Desc: string
  platformSetupSteam2Title: string
  platformSetupSteam2Desc: string

  platformSetupYoutube1Title: string
  platformSetupYoutube1Desc: string
  platformSetupYoutube2Title: string
  platformSetupYoutube2Desc: string

  platformSetupNetease1Title: string
  platformSetupNetease1Desc: string

  platformSetupBangumi1Title: string
  platformSetupBangumi1Desc: string
  platformSetupBangumi2Title: string
  platformSetupBangumi2Desc: string

  platformSetupX1Title: string
  platformSetupX1Desc: string
  platformSetupX2Title: string
  platformSetupX2Desc: string

  platformSetupDiscord1Title: string
  platformSetupDiscord1Desc: string
  platformSetupDiscord2Title: string
  platformSetupDiscord2Desc: string
  platformSetupDiscord2Action: string

  platformSetupMal1Title: string
  platformSetupMal1Desc: string
  platformSetupMal2Title: string
  platformSetupMal2Desc: string

  platformSetupXbox1Title: string
  platformSetupXbox1Desc: string
  platformSetupXbox2Title: string
  platformSetupXbox2Desc: string

  platformSetupPsn1Title: string
  platformSetupPsn1Desc: string
  platformSetupPsn2Title: string
  platformSetupPsn2Desc: string

  discordConnect: string
  oauth: string
}

export interface PlatformSetupHandlers {
  connectDiscordOAuth?: () => void
  openOAuthSection?: () => void
}

export interface PlatformSetupGuide {
  title: string
  optionalLabel: string
  steps: SetupFlowStep[]
}

function fillStep(t: PlatformSetupI18n): SetupFlowStep {
  return {
    key: 'fill',
    title: t.platformSetupFillTitle,
    description: t.platformSetupFillDesc,
  }
}

function openLabel(t: PlatformSetupI18n): string {
  return t.platformSetupOpen
}

function normalizePlatformKey(name: string): string {
  const n = name.trim().toLowerCase()
  if (n === 'netease music' || n === 'netease_music') return 'netease'
  if (n === 'myanimelist' || n === 'mal') return 'mal'
  if (n === 'twitter') return 'x'
  if (n === 'psn' || n === 'playstation') return 'playstation'
  return n
}

export function getPlatformSetupGuide(
  platformName: string,
  t: PlatformSetupI18n,
  handlers: PlatformSetupHandlers = {},
): PlatformSetupGuide | null {
  const key = normalizePlatformKey(platformName)
  const open = openLabel(t)
  const fill = fillStep(t)

  switch (key) {
    case 'github':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'username',
            title: t.platformSetupGithub1Title,
            description: t.platformSetupGithub1Desc,
            href: 'https://github.com/',
            actionLabel: open,
          },
          {
            key: 'token',
            title: t.platformSetupGithub2Title,
            description: t.platformSetupGithub2Desc,
            href: 'https://github.com/settings/tokens',
            actionLabel: open,
            optional: true,
          },
          fill,
        ],
      }

    case 'bilibili':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'uid',
            title: t.platformSetupBilibili1Title,
            description: t.platformSetupBilibili1Desc,
            href: 'https://space.bilibili.com/',
            actionLabel: open,
          },
          fill,
        ],
      }

    case 'steam':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'apikey',
            title: t.platformSetupSteam1Title,
            description: t.platformSetupSteam1Desc,
            href: 'https://steamcommunity.com/dev/apikey',
            actionLabel: open,
          },
          {
            key: 'steamid',
            title: t.platformSetupSteam2Title,
            description: t.platformSetupSteam2Desc,
            href: 'https://store.steampowered.com/account/',
            actionLabel: open,
          },
          fill,
        ],
      }

    case 'youtube':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'apikey',
            title: t.platformSetupYoutube1Title,
            description: t.platformSetupYoutube1Desc,
            href: 'https://console.cloud.google.com/apis/library/youtube.googleapis.com',
            actionLabel: open,
          },
          {
            key: 'channel',
            title: t.platformSetupYoutube2Title,
            description: t.platformSetupYoutube2Desc,
            href: 'https://www.youtube.com/account_advanced',
            actionLabel: open,
          },
          fill,
        ],
      }

    case 'netease':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'userid',
            title: t.platformSetupNetease1Title,
            description: t.platformSetupNetease1Desc,
            href: 'https://music.163.com/',
            actionLabel: open,
          },
          fill,
        ],
      }

    case 'bangumi':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'username',
            title: t.platformSetupBangumi1Title,
            description: t.platformSetupBangumi1Desc,
            href: 'https://bgm.tv/',
            actionLabel: open,
          },
          {
            key: 'token',
            title: t.platformSetupBangumi2Title,
            description: t.platformSetupBangumi2Desc,
            href: 'https://next.bgm.tv/demo/access-token',
            actionLabel: open,
            optional: true,
          },
          fill,
        ],
      }

    case 'x':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'username',
            title: t.platformSetupX1Title,
            description: t.platformSetupX1Desc,
            href: 'https://x.com/',
            actionLabel: open,
          },
          {
            key: 'bearer',
            title: t.platformSetupX2Title,
            description: t.platformSetupX2Desc,
            href: 'https://developer.x.com/en/portal/dashboard',
            actionLabel: open,
          },
          fill,
        ],
      }

    case 'discord':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'oauth-app',
            title: t.platformSetupDiscord1Title,
            description: t.platformSetupDiscord1Desc,
            onAction: handlers.openOAuthSection,
            actionLabel: t.oauth,
          },
          {
            key: 'authorize',
            title: t.platformSetupDiscord2Title,
            description: t.platformSetupDiscord2Desc,
            onAction: handlers.connectDiscordOAuth,
            actionLabel: t.platformSetupDiscord2Action || t.discordConnect,
          },
          fill,
        ],
      }

    case 'mal':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'username',
            title: t.platformSetupMal1Title,
            description: t.platformSetupMal1Desc,
            href: 'https://myanimelist.net/',
            actionLabel: open,
          },
          {
            key: 'clientid',
            title: t.platformSetupMal2Title,
            description: t.platformSetupMal2Desc,
            href: 'https://myanimelist.net/apiconfig',
            actionLabel: open,
            optional: true,
          },
          fill,
        ],
      }

    case 'xbox':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'gamertag',
            title: t.platformSetupXbox1Title,
            description: t.platformSetupXbox1Desc,
            href: 'https://www.xbox.com/',
            actionLabel: open,
          },
          {
            key: 'openxbl',
            title: t.platformSetupXbox2Title,
            description: t.platformSetupXbox2Desc,
            href: 'https://xbl.io/',
            actionLabel: open,
          },
          fill,
        ],
      }

    case 'playstation':
      return {
        title: t.platformSetupTitle,
        optionalLabel: t.platformSetupOptional,
        steps: [
          {
            key: 'onlineid',
            title: t.platformSetupPsn1Title,
            description: t.platformSetupPsn1Desc,
            href: 'https://www.playstation.com/',
            actionLabel: open,
          },
          {
            key: 'npsso',
            title: t.platformSetupPsn2Title,
            description: t.platformSetupPsn2Desc,
            href: 'https://ca.account.sony.com/api/v1/ssocookie',
            actionLabel: open,
          },
          fill,
        ],
      }

    default:
      return null
  }
}
