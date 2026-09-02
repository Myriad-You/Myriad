import type { PageContent } from '../../../contexts/PageContentContext'
import type { PerceptionSnapshot } from './registry'
import { getAgentContextConsent, subscribeAgentContextConsent } from '../../../components/agent-panel/agentContextConsent'
import { subscribeScreenConsent } from '../../../components/agent-panel/screenConsent'
import { getCurrentPageContent } from '../../../contexts/currentPage'
import { bindPublishedMusicState, subscribeCurrentSong } from '../../../contexts/currentSong'
import { getVoicePresence, subscribeVoicePresence } from '../speech/voicePresence'
import { MAX_PERCEPTION_ITEMS } from './registry'
import { subscribeForegroundSurface } from './surface'

const MIN_INTERVAL_MS = 2000

interface CaptureInput {
  route: string
  page: PageContent | null
  pageConsent: boolean
}
type CaptureFn = (input: CaptureInput) => PerceptionSnapshot[]
type PresencePost = (body: unknown) => Promise<void>
type PresenceFacts = () => unknown
type PresenceEnabled = () => boolean | Promise<boolean>

let lastRevisionKey = ''
let lastSentAt = 0
let lastRoute = ''
let lastTtsPlaying: boolean | null = null
let started = false
let inboundArmed = false
let captureFn: CaptureFn | null = null
let factsFn: PresenceFacts | null = null
let enabledFn: PresenceEnabled | null = null
let postPresence: PresencePost = defaultPostPresence
let arming: Promise<void> = Promise.resolve()

async function defaultPostPresence(body: unknown): Promise<void> {
  const { apiService } = await import('../../../services/api')
  await apiService.post('/agent/presence', body)
}

async function defaultCapture(input: CaptureInput): Promise<PerceptionSnapshot[]> {
  const { capturePerceptionSnapshots } = await import('./capture')
  return capturePerceptionSnapshots(input)
}

async function defaultFacts(): Promise<unknown> {
  const { livePresenceFacts } = await import('../livePresence')
  return livePresenceFacts()
}

export function setPresencePostForTest(post: PresencePost): void {
  postPresence = post
}

export function setPresenceCaptureForTest(capture: CaptureFn): void {
  captureFn = capture
}

export function setPresenceFactsForTest(facts: PresenceFacts): void {
  factsFn = facts
}

export function setPresenceEnabledForTest(enabled: PresenceEnabled): void {
  enabledFn = enabled
}

export function setPresenceArmedForTest(armed: boolean): void {
  inboundArmed = armed
}

export function presenceInboundArmingForTest(): Promise<void> {
  return arming
}

export function resetPresenceInboundForTest(): void {
  lastRevisionKey = ''
  lastSentAt = 0
  lastRoute = ''
  lastTtsPlaying = null
  started = false
  inboundArmed = false
  captureFn = null
  factsFn = null
  enabledFn = null
  postPresence = defaultPostPresence
  arming = Promise.resolve()
}

async function meropeIsEnabled(): Promise<boolean> {
  if (enabledFn) return enabledFn()
  try {
    const { getPublicConfigDeduped } = await import('../../../utils/requestDedup')
    const config = await getPublicConfigDeduped()
    return config?.meropeEnabled === true
  } catch {
    return false
  }
}

function revisionKey(snapshots: PerceptionSnapshot[]): string {
  return snapshots
    .map((item) => `${item.sourceId}:${item.revision}`)
    .sort()
    .join('|')
}

function documentIsHidden(): boolean {
  return typeof document !== 'undefined' && document.hidden
}

function currentRoute(): string {
  if (lastRoute) return lastRoute
  if (typeof location !== 'undefined') return location.pathname
  return '/'
}

/**
 * Report live presence when a discrete fact changes. Not a heartbeat:
 * no polling timer, and no reports while the page is already hidden
 * except the hide transition itself.
 */
export async function reportPresence(reason: string): Promise<void> {
  if (!inboundArmed) {
    return
  }
  if (documentIsHidden() && reason !== 'visibility') {
    return
  }
  const pageConsent = getAgentContextConsent()
  const input: CaptureInput = {
    route: currentRoute(),
    page: pageConsent ? getCurrentPageContent() : null,
    pageConsent,
  }
  const snapshots = (
    captureFn ? captureFn(input) : await defaultCapture(input)
  ).slice(0, MAX_PERCEPTION_ITEMS)
  const key = revisionKey(snapshots)
  if (key && key === lastRevisionKey) {
    return
  }
  const now = Date.now()
  if (lastSentAt > 0 && now - lastSentAt < MIN_INTERVAL_MS) {
    return
  }
  lastRevisionKey = key
  lastSentAt = now
  const presence = factsFn ? factsFn() : await defaultFacts()
  await postPresence({
    presence,
    perception: snapshots,
  })
}

export function notePresenceRoute(pathname: string): void {
  if (pathname === lastRoute) return
  lastRoute = pathname
  void reportPresence('route')
}

export function startPresenceInbound(): () => void {
  if (started) {
    return () => {}
  }
  started = true
  let cancelled = false
  let unbind = () => {}
  arming = (async () => {
    const enabled = await meropeIsEnabled()
    if (cancelled || !enabled) return
    bindPublishedMusicState()
    if (typeof location !== 'undefined' && !lastRoute) {
      lastRoute = location.pathname
    }
    const onVisibility = () => {
      void reportPresence('visibility')
    }
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', onVisibility)
    }
    const stopConsent = subscribeAgentContextConsent(() => {
      void reportPresence('page-consent')
    })
    const stopScreen = subscribeScreenConsent(() => {
      void reportPresence('screen-consent')
    })
    const stopVoice = subscribeVoicePresence(() => {
      const playing = getVoicePresence().ttsPlaying
      if (lastTtsPlaying === playing) return
      lastTtsPlaying = playing
      void reportPresence('tts')
    })
    const stopSong = subscribeCurrentSong(() => {
      void reportPresence('track')
    })
    const stopSurface = subscribeForegroundSurface(() => {
      void reportPresence('surface')
    })
    unbind = () => {
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisibility)
      }
      stopConsent()
      stopScreen()
      stopVoice()
      stopSong()
      stopSurface()
    }
    if (cancelled) {
      unbind()
      return
    }
    inboundArmed = true
    void reportPresence('start')
  })()
  return () => {
    cancelled = true
    inboundArmed = false
    started = false
    unbind()
  }
}
