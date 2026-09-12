import type { OAuthProviderEntry } from '../../utils/oauthSettings'
import type { SetupFlowStep } from '../settings/SetupFlow'
import { findPreset, OAUTH_PRESETS } from './oauthPresets'

export interface OAuthSetupI18n {
  platformSetupOptional: string
  platformSetupOpen: string
  oauthSetupTitle: string
  oauthSetupCopyCallback: string
  oauthSetupFillTitle: string
  oauthSetupFillDesc: string

  oauthSetupGithub1Title: string
  oauthSetupGithub1Desc: string
  oauthSetupGithub2Title: string
  oauthSetupGithub2Desc: string
  oauthSetupGithub3Title: string
  oauthSetupGithub3Desc: string

  oauthSetupGoogle1Title: string
  oauthSetupGoogle1Desc: string
  oauthSetupGoogle2Title: string
  oauthSetupGoogle2Desc: string
  oauthSetupGoogle3Title: string
  oauthSetupGoogle3Desc: string

  oauthSetupMicrosoft1Title: string
  oauthSetupMicrosoft1Desc: string
  oauthSetupMicrosoft2Title: string
  oauthSetupMicrosoft2Desc: string
  oauthSetupMicrosoft3Title: string
  oauthSetupMicrosoft3Desc: string

  oauthSetupGitlab1Title: string
  oauthSetupGitlab1Desc: string
  oauthSetupGitlab2Title: string
  oauthSetupGitlab2Desc: string
  oauthSetupGitlab3Title: string
  oauthSetupGitlab3Desc: string

  oauthSetupDiscord1Title: string
  oauthSetupDiscord1Desc: string
  oauthSetupDiscord2Title: string
  oauthSetupDiscord2Desc: string
  oauthSetupDiscord3Title: string
  oauthSetupDiscord3Desc: string

  oauthSetupAuthentik1Title: string
  oauthSetupAuthentik1Desc: string
  oauthSetupAuthentik2Title: string
  oauthSetupAuthentik2Desc: string
  oauthSetupAuthentik3Title: string
  oauthSetupAuthentik3Desc: string

  oauthSetupKeycloak1Title: string
  oauthSetupKeycloak1Desc: string
  oauthSetupKeycloak2Title: string
  oauthSetupKeycloak2Desc: string
  oauthSetupKeycloak3Title: string
  oauthSetupKeycloak3Desc: string

  oauthSetupAuth0Step1Title: string
  oauthSetupAuth0Step1Desc: string
  oauthSetupAuth0Step2Title: string
  oauthSetupAuth0Step2Desc: string
  oauthSetupAuth0Step3Title: string
  oauthSetupAuth0Step3Desc: string

  oauthSetupCustom1Title: string
  oauthSetupCustom1Desc: string
  oauthSetupCustom2Title: string
  oauthSetupCustom2Desc: string
  oauthSetupCustom3Title: string
  oauthSetupCustom3Desc: string
}

export interface OAuthSetupGuide {
  title: string
  optionalLabel: string
  steps: SetupFlowStep[]
}

export interface OAuthSetupHandlers {
  copyCallback?: () => void
  hasCallback?: boolean
}

function fillStep(t: OAuthSetupI18n): SetupFlowStep {
  return {
    key: 'fill',
    title: t.oauthSetupFillTitle,
    description: t.oauthSetupFillDesc,
  }
}

function open(t: OAuthSetupI18n): string {
  return t.platformSetupOpen
}

function callbackStep(
  t: OAuthSetupI18n,
  title: string,
  description: string,
  handlers: OAuthSetupHandlers,
): SetupFlowStep {
  return {
    key: 'callback',
    title,
    description,
    onAction: handlers.hasCallback ? handlers.copyCallback : undefined,
    actionLabel: handlers.hasCallback ? t.oauthSetupCopyCallback : undefined,
  }
}

export function resolveOAuthPresetId(entry: OAuthProviderEntry): string {
  if (entry.kind === 'github') return 'github'

  const slug = (entry.slug || '').toLowerCase()
  const name = (entry.display_name || '').toLowerCase()
  const discovery = (entry.discovery_url || '').toLowerCase()

  for (const p of OAUTH_PRESETS) {
    if (p.id === 'custom') continue
    const key = (p.defaultSlug || p.id).toLowerCase()
    if (!key) continue
    if (slug === key || slug.startsWith(`${key}-`) || slug.includes(key)) {
      return p.id
    }
    if (name.includes(p.display_name.toLowerCase())) return p.id
  }

  if (discovery.includes('accounts.google.com')) return 'google'
  if (
    discovery.includes('microsoftonline.com') ||
    discovery.includes('login.microsoft')
  ) {
    return 'microsoft'
  }
  if (discovery.includes('gitlab')) return 'gitlab'
  if (discovery.includes('discord.com')) return 'discord'
  if (discovery.includes('auth0.com')) return 'auth0'
  if (discovery.includes('keycloak') || discovery.includes('/realms/')) {
    return 'keycloak'
  }
  if (discovery.includes('authentik') || discovery.includes('/application/o/')) {
    return 'authentik'
  }

  return 'custom'
}

export function getOAuthSetupGuide(
  presetId: string,
  t: OAuthSetupI18n,
  handlers: OAuthSetupHandlers = {},
): OAuthSetupGuide {
  const preset = findPreset(presetId)
  const docs = preset?.docs_url
  const openLabel = open(t)
  const fill = fillStep(t)
  const optionalLabel = t.platformSetupOptional

  const portal = (
    title: string,
    description: string,
    href?: string,
  ): SetupFlowStep => ({
    key: 'portal',
    title,
    description,
    href: href || docs,
    actionLabel: href || docs ? openLabel : undefined,
  })

  switch (presetId) {
    case 'github':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(
            t.oauthSetupGithub1Title,
            t.oauthSetupGithub1Desc,
            docs,
          ),
          callbackStep(
            t,
            t.oauthSetupGithub2Title,
            t.oauthSetupGithub2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupGithub3Title,
            description: t.oauthSetupGithub3Desc,
          },
          fill,
        ],
      }

    case 'google':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(t.oauthSetupGoogle1Title, t.oauthSetupGoogle1Desc, docs),
          callbackStep(
            t,
            t.oauthSetupGoogle2Title,
            t.oauthSetupGoogle2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupGoogle3Title,
            description: t.oauthSetupGoogle3Desc,
          },
          fill,
        ],
      }

    case 'microsoft':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(
            t.oauthSetupMicrosoft1Title,
            t.oauthSetupMicrosoft1Desc,
            docs,
          ),
          callbackStep(
            t,
            t.oauthSetupMicrosoft2Title,
            t.oauthSetupMicrosoft2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupMicrosoft3Title,
            description: t.oauthSetupMicrosoft3Desc,
          },
          fill,
        ],
      }

    case 'gitlab':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(t.oauthSetupGitlab1Title, t.oauthSetupGitlab1Desc, docs),
          callbackStep(
            t,
            t.oauthSetupGitlab2Title,
            t.oauthSetupGitlab2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupGitlab3Title,
            description: t.oauthSetupGitlab3Desc,
          },
          fill,
        ],
      }

    case 'discord':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(t.oauthSetupDiscord1Title, t.oauthSetupDiscord1Desc, docs),
          callbackStep(
            t,
            t.oauthSetupDiscord2Title,
            t.oauthSetupDiscord2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupDiscord3Title,
            description: t.oauthSetupDiscord3Desc,
          },
          fill,
        ],
      }

    case 'authentik':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(
            t.oauthSetupAuthentik1Title,
            t.oauthSetupAuthentik1Desc,
            docs,
          ),
          callbackStep(
            t,
            t.oauthSetupAuthentik2Title,
            t.oauthSetupAuthentik2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupAuthentik3Title,
            description: t.oauthSetupAuthentik3Desc,
          },
          fill,
        ],
      }

    case 'keycloak':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(
            t.oauthSetupKeycloak1Title,
            t.oauthSetupKeycloak1Desc,
            docs,
          ),
          callbackStep(
            t,
            t.oauthSetupKeycloak2Title,
            t.oauthSetupKeycloak2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupKeycloak3Title,
            description: t.oauthSetupKeycloak3Desc,
          },
          fill,
        ],
      }

    case 'auth0':
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          portal(
            t.oauthSetupAuth0Step1Title,
            t.oauthSetupAuth0Step1Desc,
            docs,
          ),
          callbackStep(
            t,
            t.oauthSetupAuth0Step2Title,
            t.oauthSetupAuth0Step2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupAuth0Step3Title,
            description: t.oauthSetupAuth0Step3Desc,
          },
          fill,
        ],
      }

    case 'custom':
    default:
      return {
        title: t.oauthSetupTitle,
        optionalLabel,
        steps: [
          {
            key: 'portal',
            title: t.oauthSetupCustom1Title,
            description: t.oauthSetupCustom1Desc,
          },
          callbackStep(
            t,
            t.oauthSetupCustom2Title,
            t.oauthSetupCustom2Desc,
            handlers,
          ),
          {
            key: 'creds',
            title: t.oauthSetupCustom3Title,
            description: t.oauthSetupCustom3Desc,
          },
          fill,
        ],
      }
  }
}

export function getOAuthSetupGuideForEntry(
  entry: OAuthProviderEntry,
  t: OAuthSetupI18n,
  handlers: OAuthSetupHandlers = {},
): OAuthSetupGuide {
  return getOAuthSetupGuide(resolveOAuthPresetId(entry), t, handlers)
}
