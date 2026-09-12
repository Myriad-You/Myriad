import type { TouchMotionSource } from '../motion/touchSource'
import { API_URL } from '../../../config'
import { getCSRFToken } from '../../../utils/csrf'
import { liveMotionGeneration } from '../motion/liveGeneration'
import { reportPresence } from '../perception/inbound'
import { perceptionRegistry } from '../perception/registry'
import { TouchAppraisal } from './touchAppraisal'
import { TouchEncounter } from './touchEncounter'

export function createTouchAppraisal(owner: string, source: TouchMotionSource) {
  let generation = liveMotionGeneration()
  let completion: AbortController | null = null
  const encounter = new TouchEncounter(summary => {
    if (document.visibilityState !== 'visible') return
    source.expectSpeech(owner, summary.displayedReaction, performance.now())
    const capturedAt = Date.now()
    perceptionRegistry.replace({ sourceId: 'avatar-touch', kind: 'pointer', privacy: 'local',
      capturedAt, expiresAt: capturedAt + 20_000,
      summary: 'Completed contact with the displayed avatar; intention unknown.',
      safeFacts: { ...summary, displayedReaction: summary.displayedReaction ?? 'unobserved',
        completed: true, intention: 'unknown' } })
    completion?.abort()
    const controller = new AbortController()
    completion = controller
    const timeout = setTimeout(() => controller.abort(), 4000)
    void (async () => {
      await reportPresence('avatar-touch')
      const token = await getCSRFToken()
      if (!token || controller.signal.aborted || document.visibilityState !== 'visible') return
      await fetch(`${API_URL}/api/agent/addressee/touch/complete`, {
        method: 'POST', credentials: 'include', signal: controller.signal,
        headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': token },
        body: JSON.stringify(summary),
      })
    })().catch(() => {}).finally(() => {
      clearTimeout(timeout)
      if (completion === controller) completion = null
    })
  })
  const appraisal = new TouchAppraisal({
    now: () => performance.now(),
    request: async (summary, signal) => {
      generation = liveMotionGeneration()
      const displayedReaction = source.displayedReaction(owner, contactId)
      const token = await getCSRFToken()
      if (signal.aborted || !token) return null
      const response = await fetch(`${API_URL}/api/agent/addressee/touch`, {
        method: 'POST', credentials: 'include', signal,
        headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': token },
        body: JSON.stringify({ ...summary, displayedReaction }),
      })
      return response.ok ? response.json() : null
    },
    apply: (revision, reaction) => {
      if (document.visibilityState !== 'visible' || liveMotionGeneration() !== generation) return
      source.refine(owner, revision, reaction, performance.now())
    },
  })
  const cancel = () => {
    source.cancelExpectedSpeech(owner)
    appraisal.cancel(); encounter.cancel(); completion?.abort(); completion = null
    perceptionRegistry.forget('avatar-touch')
    void reportPresence('avatar-touch')
  }
  let contactId = -1
  return {
    observe: (touch: Parameters<TouchAppraisal['observe']>[0], revision: number) => {
      contactId = touch.id
      appraisal.observe(touch, revision)
      encounter.observe(touch, source.displayedReaction(owner, touch.id))
    },
    cancel,
    dispose: () => { cancel(); appraisal.dispose() },
  }
}
