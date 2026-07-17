/**
 * Tapp Playground multi-session persistence (localStorage v2).
 *
 * - Migrates once from sessionStorage v1
 * - Caps sessions / revisions / rough JSON size to avoid quota blow-ups
 * - Pure helpers — page owns React state, calls save on change
 */

import type { PlaygroundLastFailedAttempt } from '../components/PlaygroundComposer'
import type {
  PlaygroundAgentStep,
  PlaygroundKnowledgeSource,
  PlaygroundValidationReport,
  TappPlaygroundProject,
} from '../services/TappPlaygroundService'

export const SESSION_V1_KEY = 'myriad:tapp-playground:session:v1'
export const SESSIONS_V2_KEY = 'myriad:tapp-playground:sessions:v2'

/** Soft cap on concurrent sessions (evict least-recently-updated). */
export const MAX_SESSIONS = 10
/** Cap revisions per session (same as previous single-session cap). */
export const MAX_REVISIONS = 20
/** Rough localStorage budget (bytes of JSON); leave headroom under ~5MB. */
export const MAX_STORE_BYTES = 4_500_000
/** Title length from first user instruction. */
export const TITLE_MAX_CHARS = 36

export interface PlaygroundRevision {
  id: string
  project: TappPlaygroundProject
  explanation: string
  instruction: string
  warnings: string[]
  createdAt: number
  origin?: 'user' | 'runtime-repair'
  agentTrace?: PlaygroundAgentStep[]
  knowledgeSources?: PlaygroundKnowledgeSource[]
  validation?: PlaygroundValidationReport
}

export interface PlaygroundSession {
  id: string
  title: string
  createdAt: number
  updatedAt: number
  revisions: PlaygroundRevision[]
  revisionIndex: number
  lastFailedAttempt?: PlaygroundLastFailedAttempt | null
}

export interface PlaygroundSessionsStore {
  activeSessionId: string
  sessions: PlaygroundSession[]
}

function randomId(prefix: string): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return `${prefix}_${crypto.randomUUID()}`
  }
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`
}

export function createSessionId(): string {
  return randomId('sess')
}

export function createRevisionId(): string {
  return randomId('rev')
}

/** Truncate instruction into a session title; empty → ''. */
export function titleFromInstruction(
  instruction: string,
  maxChars = TITLE_MAX_CHARS,
): string {
  const text = instruction.replace(/\s+/g, ' ').trim()
  if (!text) return ''
  if (text.length <= maxChars) return text
  return `${text.slice(0, Math.max(1, maxChars - 1))}…`
}

export function createEmptySession(
  now = Date.now(),
  partial?: Partial<PlaygroundSession>,
): PlaygroundSession {
  return {
    id: createSessionId(),
    title: '',
    createdAt: now,
    updatedAt: now,
    revisions: [],
    revisionIndex: -1,
    lastFailedAttempt: null,
    ...partial,
  }
}

export function createEmptyStore(now = Date.now()): PlaygroundSessionsStore {
  const session = createEmptySession(now)
  return { activeSessionId: session.id, sessions: [session] }
}

function isFailedAttempt(value: unknown): value is PlaygroundLastFailedAttempt {
  if (!value || typeof value !== 'object') return false
  const v = value as Record<string, unknown>
  return (
    typeof v.instruction === 'string' &&
    typeof v.error === 'string' &&
    typeof v.elapsedMs === 'number' &&
    typeof v.finishedAt === 'number'
  )
}

function normalizeRevision(raw: unknown): PlaygroundRevision | null {
  if (!raw || typeof raw !== 'object') return null
  const r = raw as Record<string, unknown>
  if (!r.project || typeof r.project !== 'object') return null
  if (typeof r.instruction !== 'string') return null
  const createdAt =
    typeof r.createdAt === 'number' && Number.isFinite(r.createdAt)
      ? r.createdAt
      : Date.now()
  return {
    id: typeof r.id === 'string' && r.id ? r.id : createRevisionId(),
    project: r.project as TappPlaygroundProject,
    explanation: typeof r.explanation === 'string' ? r.explanation : '',
    instruction: r.instruction,
    warnings: Array.isArray(r.warnings)
      ? r.warnings.filter((w): w is string => typeof w === 'string')
      : [],
    createdAt,
    origin:
      r.origin === 'user' || r.origin === 'runtime-repair' ? r.origin : undefined,
    agentTrace: Array.isArray(r.agentTrace)
      ? (r.agentTrace as PlaygroundAgentStep[])
      : undefined,
    knowledgeSources: Array.isArray(r.knowledgeSources)
      ? (r.knowledgeSources as PlaygroundKnowledgeSource[])
      : undefined,
    validation:
      r.validation && typeof r.validation === 'object'
        ? (r.validation as PlaygroundValidationReport)
        : undefined,
  }
}

function normalizeSession(raw: unknown): PlaygroundSession | null {
  if (!raw || typeof raw !== 'object') return null
  const s = raw as Record<string, unknown>
  if (!Array.isArray(s.revisions)) return null
  const revisions = s.revisions
    .map(normalizeRevision)
    .filter((r): r is PlaygroundRevision => r !== null)
  let revisionIndex =
    typeof s.revisionIndex === 'number' && Number.isInteger(s.revisionIndex)
      ? s.revisionIndex
      : revisions.length - 1
  if (revisions.length === 0) revisionIndex = -1
  else {
    revisionIndex = Math.max(0, Math.min(revisions.length - 1, revisionIndex))
  }
  const now = Date.now()
  const createdAt =
    typeof s.createdAt === 'number' && Number.isFinite(s.createdAt)
      ? s.createdAt
      : revisions[0]?.createdAt || now
  const updatedAt =
    typeof s.updatedAt === 'number' && Number.isFinite(s.updatedAt)
      ? s.updatedAt
      : revisions[revisions.length - 1]?.createdAt || createdAt
  const title =
    typeof s.title === 'string'
      ? s.title
      : titleFromInstruction(revisions[0]?.instruction || '')
  return {
    id: typeof s.id === 'string' && s.id ? s.id : createSessionId(),
    title,
    createdAt,
    updatedAt,
    revisions,
    revisionIndex,
    lastFailedAttempt: isFailedAttempt(s.lastFailedAttempt)
      ? s.lastFailedAttempt
      : null,
  }
}

function loadV1Session(): PlaygroundSession | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = sessionStorage.getItem(SESSION_V1_KEY)
    if (!raw) return null
    const value = JSON.parse(raw)
    if (!value || !Array.isArray(value.revisions)) return null
    const session = normalizeSession({
      id: createSessionId(),
      title: titleFromInstruction(value.revisions[0]?.instruction || ''),
      createdAt: value.revisions[0]?.createdAt || Date.now(),
      updatedAt:
        value.revisions[value.revisions.length - 1]?.createdAt || Date.now(),
      revisions: value.revisions,
      revisionIndex: value.revisionIndex,
      lastFailedAttempt: value.lastFailedAttempt,
    })
    return session
  } catch {
    return null
  }
}

/** Cap sessions by updatedAt; always keep active. Cap revisions per session. */
export function pruneStore(store: PlaygroundSessionsStore): PlaygroundSessionsStore {
  if (!store.sessions.length) return createEmptyStore()

  let sessions = store.sessions.map((session) => {
    const revisions = session.revisions.slice(-MAX_REVISIONS)
    let revisionIndex = session.revisionIndex
    if (revisions.length === 0) revisionIndex = -1
    else {
      // If we dropped from the front, shift index
      const dropped = session.revisions.length - revisions.length
      if (dropped > 0) {
        revisionIndex = Math.max(0, revisionIndex - dropped)
      }
      revisionIndex = Math.min(revisions.length - 1, revisionIndex)
    }
    return { ...session, revisions, revisionIndex }
  })

  // Evict oldest by updatedAt, but never drop active if possible
  if (sessions.length > MAX_SESSIONS) {
    const active = sessions.find((s) => s.id === store.activeSessionId)
    const others = sessions
      .filter((s) => s.id !== store.activeSessionId)
      .sort((a, b) => b.updatedAt - a.updatedAt)
    const keep = others.slice(0, MAX_SESSIONS - (active ? 1 : 0))
    sessions = active ? [active, ...keep] : keep
  }

  // Size budget: drop oldest non-active sessions, then oldest revisions
  const measure = (list: PlaygroundSession[]) =>
    JSON.stringify({
      activeSessionId: store.activeSessionId,
      sessions: list,
    }).length

  while (sessions.length > 1 && measure(sessions) > MAX_STORE_BYTES) {
    const sorted = [...sessions].sort((a, b) => a.updatedAt - b.updatedAt)
    const victim =
      sorted.find((s) => s.id !== store.activeSessionId) || sorted[0]
    if (!victim) break
    sessions = sessions.filter((s) => s.id !== victim.id)
  }

  while (measure(sessions) > MAX_STORE_BYTES) {
    let trimmed = false
    sessions = sessions.map((session) => {
      if (session.revisions.length <= 1) return session
      // Prefer trimming non-active sessions first
      if (
        session.id === store.activeSessionId &&
        sessions.some((s) => s.id !== store.activeSessionId && s.revisions.length > 1)
      ) {
        return session
      }
      if (session.revisions.length <= 1) return session
      trimmed = true
      const revisions = session.revisions.slice(1)
      let revisionIndex = session.revisionIndex - 1
      if (revisions.length === 0) revisionIndex = -1
      else revisionIndex = Math.max(0, Math.min(revisions.length - 1, revisionIndex))
      return { ...session, revisions, revisionIndex }
    })
    if (!trimmed) {
      // Last resort: drop agentTrace/knowledgeSources from oldest revs
      sessions = sessions.map((session) => ({
        ...session,
        revisions: session.revisions.map((rev, i) =>
          i < session.revisions.length - 1
            ? { ...rev, agentTrace: undefined, knowledgeSources: undefined }
            : rev,
        ),
      }))
      if (measure(sessions) <= MAX_STORE_BYTES) break
      // Still too big — give up further trimming to avoid empty store
      break
    }
  }

  if (!sessions.length) return createEmptyStore()

  let activeSessionId = store.activeSessionId
  if (!sessions.some((s) => s.id === activeSessionId)) {
    activeSessionId = sessions.sort((a, b) => b.updatedAt - a.updatedAt)[0].id
  }

  return { activeSessionId, sessions }
}

export function getActiveSession(
  store: PlaygroundSessionsStore,
): PlaygroundSession {
  return (
    store.sessions.find((s) => s.id === store.activeSessionId) ||
    store.sessions[0] ||
    createEmptySession()
  )
}

export function updateActiveSession(
  store: PlaygroundSessionsStore,
  updater: (session: PlaygroundSession) => PlaygroundSession,
  touch = true,
): PlaygroundSessionsStore {
  const active = getActiveSession(store)
  const next = updater(active)
  const updated: PlaygroundSession = touch
    ? { ...next, updatedAt: Date.now() }
    : next
  const sessions = store.sessions.map((s) =>
    s.id === active.id ? updated : s,
  )
  // If active was missing (empty store edge), ensure it exists
  if (!store.sessions.some((s) => s.id === active.id)) {
    sessions.push(updated)
  }
  return pruneStore({
    activeSessionId: updated.id,
    sessions,
  })
}

export function createAndActivateSession(
  store: PlaygroundSessionsStore,
): PlaygroundSessionsStore {
  const session = createEmptySession()
  return pruneStore({
    activeSessionId: session.id,
    sessions: [...store.sessions, session],
  })
}

export function switchSession(
  store: PlaygroundSessionsStore,
  sessionId: string,
): PlaygroundSessionsStore {
  if (!store.sessions.some((s) => s.id === sessionId)) return store
  return {
    ...store,
    activeSessionId: sessionId,
    sessions: store.sessions.map((s) =>
      s.id === sessionId ? { ...s, updatedAt: Date.now() } : s,
    ),
  }
}

export function deleteSession(
  store: PlaygroundSessionsStore,
  sessionId: string,
): PlaygroundSessionsStore {
  const remaining = store.sessions.filter((s) => s.id !== sessionId)
  if (!remaining.length) return createEmptyStore()
  const activeSessionId =
    store.activeSessionId === sessionId
      ? remaining.sort((a, b) => b.updatedAt - a.updatedAt)[0].id
      : store.activeSessionId
  return pruneStore({ activeSessionId, sessions: remaining })
}

export function clearSessionContent(
  session: PlaygroundSession,
): PlaygroundSession {
  return {
    ...session,
    title: '',
    revisions: [],
    revisionIndex: -1,
    lastFailedAttempt: null,
    updatedAt: Date.now(),
  }
}

/**
 * Append a successful generation revision: truncates redo stack, caps length,
 * auto-titles empty sessions from the first user instruction.
 */
export function pushRevision(
  session: PlaygroundSession,
  revision: Omit<PlaygroundRevision, 'id'> & { id?: string },
): PlaygroundSession {
  const retained = session.revisions.slice(0, session.revisionIndex + 1)
  retained.push({
    ...revision,
    id: revision.id || createRevisionId(),
  })
  const revisions = retained.slice(-MAX_REVISIONS)
  const nextTitle =
    session.title.trim() ||
    (revision.origin !== 'runtime-repair'
      ? titleFromInstruction(revision.instruction)
      : '') ||
    session.title
  return {
    ...session,
    title: nextTitle,
    revisions,
    revisionIndex: revisions.length - 1,
    lastFailedAttempt: null,
    updatedAt: Date.now(),
  }
}

export function loadSessionsStore(): PlaygroundSessionsStore {
  if (typeof window === 'undefined') return createEmptyStore()

  // Prefer v2 localStorage
  try {
    const raw = localStorage.getItem(SESSIONS_V2_KEY)
    if (raw) {
      const parsed = JSON.parse(raw)
      if (
        parsed &&
        typeof parsed === 'object' &&
        Array.isArray(parsed.sessions)
      ) {
        const sessions: PlaygroundSession[] = parsed.sessions
          .map(normalizeSession)
          .filter((s: PlaygroundSession | null): s is PlaygroundSession => s !== null)
        if (sessions.length) {
          let activeSessionId =
            typeof parsed.activeSessionId === 'string'
              ? parsed.activeSessionId
              : sessions[0].id
          if (!sessions.some((s: PlaygroundSession) => s.id === activeSessionId)) {
            activeSessionId = sessions[0].id
          }
          return pruneStore({ activeSessionId, sessions })
        }
      }
    }
  } catch {
    // Fall through to v1 migration / empty
  }

  // One-shot migrate from v1 sessionStorage
  const migrated = loadV1Session()
  if (migrated && (migrated.revisions.length > 0 || migrated.lastFailedAttempt)) {
    const store = pruneStore({
      activeSessionId: migrated.id,
      sessions: [migrated],
    })
    saveSessionsStore(store)
    try {
      sessionStorage.removeItem(SESSION_V1_KEY)
    } catch {
      // ignore
    }
    return store
  }

  return createEmptyStore()
}

export function saveSessionsStore(store: PlaygroundSessionsStore): void {
  if (typeof window === 'undefined') return
  const pruned = pruneStore(store)
  try {
    localStorage.setItem(SESSIONS_V2_KEY, JSON.stringify(pruned))
  } catch {
    // Quota or private mode — try a more aggressive prune once
    try {
      const emergency = pruneStore({
        activeSessionId: pruned.activeSessionId,
        sessions: pruned.sessions
          .filter((s) => s.id === pruned.activeSessionId)
          .map((s) => ({
            ...s,
            revisions: s.revisions.slice(-5).map((r) => ({
              ...r,
              agentTrace: undefined,
              knowledgeSources: undefined,
            })),
            revisionIndex: Math.min(
              s.revisionIndex,
              Math.max(0, Math.min(s.revisions.length, 5) - 1),
            ),
          })),
      })
      localStorage.setItem(SESSIONS_V2_KEY, JSON.stringify(emergency))
    } catch {
      // Give up silently — in-memory state still works for the tab
    }
  }
}
