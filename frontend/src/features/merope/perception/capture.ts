import type { PageContent } from '../../../contexts/PageContentContext'
import type { PerceptionSnapshot } from './registry'
import { getScreenConsent } from '../../../components/agent-panel/screenConsent'
import { captureProductionRigStateSummary } from '../motion/runtimeHost'
import { getVoicePresence } from '../speech/voicePresence'
import { replaceMusicTrackSource, replaceSurfaceSource } from './consentedSources'
import { pagePerceptionCopy } from './pageCopy'
import { MAX_PERCEPTION_ITEMS, perceptionRegistry } from './registry'

const PAGE_TTL_MS = 8_000
const POINTER_TTL_MS = 3_000
const MUSIC_TTL_MS = 2_000
const VOICE_TTL_MS = 2_000
const PRESENCE_TTL_MS = 4_000

/**
 * Event-time capture. High-frequency pointer/audio frames stay local;
 * Lite only sees these bounded summaries.
 */
export function capturePerceptionSnapshots(input: {
  route: string
  page: PageContent | null
  pageConsent: boolean
  selection?: string
}): PerceptionSnapshot[] {
  const now = Date.now()
  if (input.pageConsent && input.page) {
    const copy = pagePerceptionCopy(input.page, input.route)
    perceptionRegistry.replace({
      sourceId: 'page',
      kind: 'page',
      expiresAt: now + PAGE_TTL_MS,
      summary: copy.summary,
      safeFacts: {
        title: copy.title,
        type: input.page.type,
        hasBody: copy.hasBody,
        ...(copy.author ? { author: copy.author } : {}),
      },
      privacy: 'consented',
    })
  } else {
    perceptionRegistry.forget('page')
  }

  if (input.selection?.trim()) {
    perceptionRegistry.replace({
      sourceId: 'pointer',
      kind: 'pointer',
      expiresAt: now + POINTER_TTL_MS,
      summary: input.selection.trim().slice(0, 200),
      safeFacts: { selected: true, length: input.selection.trim().length },
      privacy: 'consented',
    })
  } else {
    perceptionRegistry.replace({
      sourceId: 'pointer',
      kind: 'pointer',
      expiresAt: now + POINTER_TTL_MS,
      summary: input.route,
      safeFacts: { route: input.route, selected: false },
      privacy: 'local',
    })
  }

  replaceSurfaceSource({ now, ttlMs: PRESENCE_TTL_MS })

  const rig = captureProductionRigStateSummary()
  perceptionRegistry.replace({
    sourceId: 'music',
    kind: 'music',
    expiresAt: now + MUSIC_TTL_MS,
    summary: rig.musicPlaying
      ? `music ${rig.music?.energy ?? 'present'} ${rig.music?.beat ?? 'hold'}`
      : 'music idle',
    safeFacts: {
      playing: rig.musicPlaying,
      singing: rig.singing,
      energy: rig.music?.energy ?? 'quiet',
      beat: rig.music?.beat ?? 'rest',
    },
    privacy: 'system',
  })

  replaceMusicTrackSource({
    now,
    pageConsent: input.pageConsent,
    ttlMs: MUSIC_TTL_MS,
  })

  const voice = getVoicePresence()
  perceptionRegistry.replace({
    sourceId: 'voice',
    kind: 'voice',
    expiresAt: now + VOICE_TTL_MS,
    summary: voice.userSpeaking
      ? 'user speaking'
      : voice.ttsPlaying
        ? 'agent speaking'
        : voice.listening
          ? 'listening'
          : 'voice idle',
    safeFacts: {
      listening: voice.listening,
      ttsPlaying: voice.ttsPlaying,
      userSpeaking: voice.userSpeaking,
    },
    privacy: 'local',
  })

  perceptionRegistry.replace({
    sourceId: 'presence',
    kind: 'presence',
    expiresAt: now + PRESENCE_TTL_MS,
    summary: rig.pageVisible ? 'page visible' : 'page hidden',
    safeFacts: {
      pageVisible: rig.pageVisible,
      faceVisible: rig.faceVisible,
    },
    privacy: 'system',
  })

  if (getScreenConsent()) {
    perceptionRegistry.replace({
      sourceId: 'screen',
      kind: 'screen',
      expiresAt: now + PAGE_TTL_MS,
      summary: 'screen permission enabled; no visual observation available',
      safeFacts: { consented: true, pixels: false, observed: false },
      privacy: 'consented',
    })
  } else {
    perceptionRegistry.forget('screen')
  }

  return perceptionRegistry.active(now).slice(0, MAX_PERCEPTION_ITEMS)
}
