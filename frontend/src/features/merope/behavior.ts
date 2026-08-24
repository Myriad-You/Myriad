import type { MeropeMessage, MeropeSnapshot } from './types'

export const MEROPE_CONFIG_CHANGED_EVENT = 'merope-config-changed'
export const MEROPE_CONFIG_REVISION_KEY = 'myriad-merope-config-revision'

export function announceMeropeConfigChanged(): void {
  window.dispatchEvent(new CustomEvent(MEROPE_CONFIG_CHANGED_EVENT))
  try {
    localStorage.setItem(MEROPE_CONFIG_REVISION_KEY, String(Date.now()))
  } catch {}
}

export function shouldMountMerope(
  hasChecked: boolean,
  isAuthenticated: boolean,
  enabled: unknown,
): boolean {
  return hasChecked && isAuthenticated && enabled === true
}

export function meropePollInterval(documentHidden: boolean): number {
  return documentHidden ? 120_000 : 30_000
}

export function snapshotChanged(
  current: MeropeSnapshot | null,
  next: MeropeSnapshot,
): boolean {
  return current?.fingerprint !== next.fingerprint
}

export function latestUnreadProactive(
  messages: MeropeMessage[],
): MeropeMessage | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]
    if (message.role === 'proactive' && !message.meta.readAt) return message
  }
  return null
}

/** Floating overlay only mounts chat when onboarding is completed (Reports entry). */
export function isMeropeOverlayReady(
  hasCharacter: boolean,
  onboardingCompleted: boolean | undefined,
): boolean {
  return hasCharacter && onboardingCompleted === true
}

type ScheduleFrame = (callback: FrameRequestCallback) => number
type CancelFrame = (handle: number) => void

/**
 * Leaves one fully painted, phase-continuous frame between expanding the panel
 * and starting its greeting. Two animation frames are intentional: the first
 * observes the React commit, the second begins the velocity handoff.
 */
export function deferMeropeExpansionGesture(
  callback: () => void,
  scheduleFrame: ScheduleFrame,
  cancelFrame: CancelFrame,
): () => void {
  let firstFrame = 0
  let secondFrame = 0
  firstFrame = scheduleFrame(() => {
    firstFrame = 0
    secondFrame = scheduleFrame(() => {
      secondFrame = 0
      callback()
    })
  })
  return () => {
    if (firstFrame) cancelFrame(firstFrame)
    if (secondFrame) cancelFrame(secondFrame)
    firstFrame = 0
    secondFrame = 0
  }
}

export function meropeExpansionGenerationIsCurrent(
  scheduledGeneration: number,
  currentGeneration: number,
  collapsed: boolean,
): boolean {
  return scheduledGeneration === currentGeneration && !collapsed
}

export function shouldAnimateMeropeUnread(
  expanded: boolean,
  previousUnreadCount: number,
  unreadCount: number,
): boolean {
  return expanded && unreadCount > previousUnreadCount
}
