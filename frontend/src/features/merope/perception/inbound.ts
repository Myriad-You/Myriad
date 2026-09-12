/** POST /agent/presence only; no consciousness import. */
import type { PageContent } from '../../../contexts/PageContentContext'
import type { PerceptionSnapshot } from './registry'
import {
  getAgentContextConsent,
  subscribeAgentContextConsent,
} from '../../../components/agent-panel/agentContextConsent'
import { subscribeAgentPanelVisible } from '../../../components/agent-panel/agentPanelVisible'
import {
  subscribeAgentSelection,
  turnSelectionText,
} from '../../../components/agent-panel/agentSelection'
import { subscribeScreenConsent } from '../../../components/agent-panel/screenConsent'
import {
  getCurrentPageContent,
  subscribeCurrentPageContent,
} from '../../../contexts/currentPage'
import {
  bindPublishedMusicState,
  currentAgentMusicStatus,
  subscribeCurrentSong,
} from '../../../contexts/currentSong'
import { authSubject } from '../../../utils/authSubject'
import { PERSONA_UPDATED_EVENT } from '../events'
import {
  getVoicePresence,
  subscribeVoicePresence,
} from '../speech/voicePresence'
import { PRESENCE_LEASE_MS } from './leasePolicy'
import { MAX_PERCEPTION_ITEMS } from './registry'
import { subscribeForegroundSurface } from './surface'

const MIN_INTERVAL_MS = 2000

interface CaptureInput {
  route: string
  page: PageContent | null
  pageConsent: boolean
  selection?: string
}
type CaptureFn = (input: CaptureInput) => PerceptionSnapshot[] | Promise<PerceptionSnapshot[]>
type PresencePost = (body: unknown, signal: AbortSignal) => Promise<void>
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
let leaseTimer: ReturnType<typeof setInterval> | null = null
let trailingTimer: ReturnType<typeof setTimeout> | null = null
let reportGeneration = 0
let postGeneration = 0
let postChain: Promise<void> = Promise.resolve()
let postFailures = 0

// A new subject must never wait for, retry, or dedupe against the old subject's work.
authSubject.subscribe(() => {
  reportGeneration += 1
  postGeneration += 1
  stopTrailingReport()
  postChain = Promise.resolve()
  postFailures = 0
  lastRevisionKey = ''
  lastSentAt = 0
})

async function defaultPostPresence(body: unknown, signal: AbortSignal): Promise<void> {
  const { apiService } = await import('../../../services/api')
  signal.throwIfAborted()
  await apiService.post('/agent/presence', body, { signal })
}

async function defaultCapture(
  input: CaptureInput,
  subject: AbortSignal,
): Promise<PerceptionSnapshot[]> {
  const { capturePerceptionSnapshots } = await import('./capture')
  if (subject.aborted) return []
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
  reportGeneration += 1
  postGeneration += 1
  stopTrailingReport()
  postChain = Promise.resolve()
  postFailures = 0
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
  stopPresenceLease()
}

function stopTrailingReport(): void {
  if (trailingTimer !== null) clearTimeout(trailingTimer)
  trailingTimer = null
}

function scheduleLatestReport(delay: number): void {
  if (trailingTimer !== null) return
  trailingTimer = setTimeout(() => {
    trailingTimer = null
    void reportPresence('trailing')
  }, Math.max(1, delay))
}

function stopPresenceLease(): void {
  if (leaseTimer != null) {
    clearInterval(leaseTimer)
    leaseTimer = null
  }
}

function startPresenceLease(): void {
  stopPresenceLease()
  if (typeof document === 'undefined' || document.hidden) return
  leaseTimer = setInterval(() => {
    void reportPresence('lease')
  }, PRESENCE_LEASE_MS)
}

async function meropeIsEnabled(): Promise<boolean> {
  try {
    if (enabledFn) return await enabledFn()
    const { getPublicConfigDeduped } =
      await import('../../../utils/requestDedup')
    const config = await getPublicConfigDeduped()
    return config?.meropeEnabled === true
  } catch {
    return false
  }
}

function revisionKey(snapshots: PerceptionSnapshot[]): string {
  return snapshots
    .map((item) => JSON.stringify([item.sourceId, item.kind, item.summary, item.privacy,
      Object.entries(item.safeFacts).toSorted(([a], [b]) => a.localeCompare(b))]))
    .toSorted()
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

/** Lease is not a decision heartbeat */
export async function reportPresence(reason: string): Promise<void> {
  const subject = authSubject.signal
  if (!inboundArmed) {
    return
  }
  const consentChange = reason === 'page-consent' || reason === 'screen-consent'
  if (documentIsHidden() && reason !== 'visibility' && !consentChange) {
    return
  }
  const generation = ++reportGeneration
  const captureStartedAt = Date.now()
  const pageConsent = getAgentContextConsent()
  const input: CaptureInput = {
    route: currentRoute(),
    page: pageConsent ? getCurrentPageContent() : null,
    pageConsent,
    selection: turnSelectionText(),
  }
  const captured = captureFn ? captureFn(input) : defaultCapture(input, subject)
  const snapshots = (Array.isArray(captured) ? captured : await captured)
    .slice(0, MAX_PERCEPTION_ITEMS)
  if (subject.aborted || !inboundArmed || generation !== reportGeneration) return
  const presence = factsFn ? factsFn() : await defaultFacts()
  if (subject.aborted || !inboundArmed || generation !== reportGeneration) return
  const key = JSON.stringify([revisionKey(snapshots), presence])
  const now = Date.now()
  if (reason !== 'lease' && reason !== 'panel' && reason !== 'visibility' && !consentChange) {
    if (key && key === lastRevisionKey) {
      return
    }
    if (lastSentAt > 0 && now - lastSentAt < MIN_INTERVAL_MS) {
      scheduleLatestReport(MIN_INTERVAL_MS - (now - lastSentAt))
      return
    }
  }
  stopTrailingReport()
  // Serialize network writes so a slow pre-revocation payload cannot land after the clear.
  const post = postPresence
  const sendGeneration = ++postGeneration
  postChain = postChain.catch(() => {}).then(async () => {
    if (subject.aborted || !inboundArmed || sendGeneration !== postGeneration) return
    lastSentAt = Date.now()
    try {
      const elapsed = Math.max(0, Date.now() - captureStartedAt)
      const perception = snapshots.map((snapshot) => ({
        ...snapshot,
        ttlMs: Math.max(0, snapshot.ttlMs - elapsed),
      })).filter((snapshot) => snapshot.ttlMs > 0)
      const musicStatus = currentAgentMusicStatus()
      await post({ presence, perception, ...(musicStatus ? { musicStatus } : {}) }, subject)
      if (subject.aborted || sendGeneration !== postGeneration) return
      postFailures = 0
      if (sendGeneration === postGeneration) lastRevisionKey = key
    } catch {
      // Do not mark a failed observation delivered or leak response payloads.
      if (!subject.aborted && sendGeneration === postGeneration && inboundArmed) {
        lastRevisionKey = ''
        postFailures += 1
        if (postFailures === 1) scheduleLatestReport(MIN_INTERVAL_MS)
      }
    }
  })
  await postChain
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
  let configurationRevision = 0
  let unbind = () => {}
  const disarm = () => {
    inboundArmed = false
    reportGeneration += 1
    postGeneration += 1
    stopTrailingReport()
    unbind()
    unbind = () => {}
    stopPresenceLease()
    lastRevisionKey = ''
    lastSentAt = 0
    lastTtsPlaying = null
    postFailures = 0
  }
  const refresh = () => {
    const revision = ++configurationRevision
    disarm()
    arming = arm(revision)
  }
  const arm = async (revision: number) => {
    const enabled = await meropeIsEnabled()
    if (cancelled || revision !== configurationRevision || !enabled) return
    bindPublishedMusicState()
    if (typeof location !== 'undefined' && !lastRoute) {
      lastRoute = location.pathname
    }
    const onVisibility = () => {
      void reportPresence('visibility')
      if (documentIsHidden()) stopPresenceLease()
      else startPresenceLease()
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
    const stopPage = subscribeCurrentPageContent(() => {
      void reportPresence('page-content')
    })
    const stopSelection = subscribeAgentSelection(() => {
      void reportPresence('selection')
    })
    const stopSurface = subscribeForegroundSurface(() => {
      void reportPresence('surface')
    })
    const stopPanel = subscribeAgentPanelVisible(() => {
      void reportPresence('panel')
    })
    unbind = () => {
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisibility)
      }
      stopConsent()
      stopScreen()
      stopVoice()
      stopSong()
      stopPage()
      stopSelection()
      stopSurface()
      stopPanel()
      stopPresenceLease()
    }
    if (cancelled || revision !== configurationRevision) {
      unbind()
      return
    }
    inboundArmed = true
    startPresenceLease()
    void reportPresence('start')
  }
  if (typeof window !== 'undefined') window.addEventListener(PERSONA_UPDATED_EVENT, refresh)
  refresh()
  return () => {
    if (cancelled) return
    cancelled = true
    configurationRevision += 1
    if (typeof window !== 'undefined') window.removeEventListener(PERSONA_UPDATED_EVENT, refresh)
    disarm()
    started = false
  }
}
