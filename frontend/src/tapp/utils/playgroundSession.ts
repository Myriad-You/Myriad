import type { PlaygroundLastFailedAttempt } from '../components/PlaygroundComposer'
import type {
  PlaygroundAgentStep,
  PlaygroundKnowledgeSource,
  PlaygroundMemoryTurn,
  PlaygroundRevisionOrigin,
  PlaygroundValidationReport,
  TappPlaygroundProject,
} from '../services/TappPlaygroundService'

export const LEGACY_SESSION_STORAGE_KEY = 'myriad:tapp-playground:session:v1'
export const SESSIONS_STORAGE_KEY = 'myriad:tapp-playground:sessions:v2'

export const MAX_SESSIONS = 10
export const MAX_REVISIONS = 20
export const MAX_STORE_BYTES = 4_500_000
export const TITLE_MAX_CHARS = 36

export const MANUAL_REVISION_MERGE_MS = 4_000

export interface PlaygroundRevision {
  id: string
  project: TappPlaygroundProject
  explanation: string
  instruction: string
  warnings: string[]
  createdAt: number
  origin?: PlaygroundRevisionOrigin
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
  // randomUUID needs a secure context; getRandomValues does not (plain-HTTP LAN hosts).
  if (typeof crypto.randomUUID === 'function') {
    return `${prefix}_${crypto.randomUUID()}`
  }
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  return `${prefix}_${Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')}`
}

export function createSessionId(): string {
  return randomId('sess')
}

export function createRevisionId(): string {
  return randomId('rev')
}

export function titleFromInstruction(
  instruction: string,
  maxChars = TITLE_MAX_CHARS,
): string {
  const text = instruction.replaceAll(/\s+/g, ' ').trim()
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
      r.origin === 'user' ||
      r.origin === 'runtime-repair' ||
      r.origin === 'manual'
        ? r.origin
        : undefined,
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
  if (revisions.length === 0) { revisionIndex = -1
}
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
      : revisions.at(-1)?.createdAt || createdAt
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
    const raw = sessionStorage.getItem(LEGACY_SESSION_STORAGE_KEY)
    if (!raw) return null
    const value = JSON.parse(raw)
    if (!value || !Array.isArray(value.revisions)) return null
    const session = normalizeSession({
      id: createSessionId(),
      title: titleFromInstruction(value.revisions[0]?.instruction || ''),
      createdAt: value.revisions[0]?.createdAt || Date.now(),
      updatedAt:
        value.revisions.at(-1)?.createdAt || Date.now(),
      revisions: value.revisions,
      revisionIndex: value.revisionIndex,
      lastFailedAttempt: value.lastFailedAttempt,
    })
    return session
  } catch {
    return null
  }
}

export interface PruneStoreMeta {
  sessionsDropped: number
  revisionsTrimmed: number
  changed: boolean
}

export interface PruneStoreResult {
  store: PlaygroundSessionsStore
  meta: PruneStoreMeta
}

const EMPTY_PRUNE_META: PruneStoreMeta = {
  sessionsDropped: 0,
  revisionsTrimmed: 0,
  changed: false,
}

export function pruneStoreWithMeta(
  store: PlaygroundSessionsStore,
): PruneStoreResult {
  if (!store.sessions.length) {
    return { store: createEmptyStore(), meta: EMPTY_PRUNE_META }
  }

  let sessionsDropped = 0
  let revisionsTrimmed = 0

  let sessions = store.sessions.map((session) => {
    const revisions = session.revisions.slice(-MAX_REVISIONS)
    const dropped = session.revisions.length - revisions.length
    if (dropped > 0) {
      revisionsTrimmed += dropped
    }
    let revisionIndex = session.revisionIndex
    if (revisions.length === 0) {
      revisionIndex = -1
    } else {
      if (dropped > 0) {
        revisionIndex = Math.max(0, revisionIndex - dropped)
      }
      revisionIndex = Math.min(revisions.length - 1, revisionIndex)
    }
    return { ...session, revisions, revisionIndex }
  })

  if (sessions.length > MAX_SESSIONS) {
    const active = sessions.find((s) => s.id === store.activeSessionId)
    const others = sessions
      .filter((s) => s.id !== store.activeSessionId)
      .toSorted((a, b) => b.updatedAt - a.updatedAt)
    const keep = others.slice(0, MAX_SESSIONS - (active ? 1 : 0))
    const next = active ? [active, ...keep] : keep
    sessionsDropped += sessions.length - next.length
    sessions = next
  }

  const measure = (list: PlaygroundSession[]) =>
    JSON.stringify({
      activeSessionId: store.activeSessionId,
      sessions: list,
    }).length

  while (sessions.length > 1 && measure(sessions) > MAX_STORE_BYTES) {
    const sorted = sessions.toSorted((a, b) => a.updatedAt - b.updatedAt)
    const victim =
      sorted.find((s) => s.id !== store.activeSessionId) ?? sorted[0]
    if (!victim) break
    sessions = sessions.filter((s) => s.id !== victim.id)
    sessionsDropped += 1
  }

  while (measure(sessions) > MAX_STORE_BYTES) {
    let trimmed = false
    sessions = sessions.map((session) => {
      if (session.revisions.length <= 1) return session
      if (
        session.id === store.activeSessionId &&
        sessions.some((s) => s.id !== store.activeSessionId && s.revisions.length > 1)
      ) {
        return session
      }
      if (session.revisions.length <= 1) return session
      trimmed = true
      revisionsTrimmed += 1
      const revisions = session.revisions.slice(1)
      let revisionIndex = session.revisionIndex - 1
      if (revisions.length === 0) revisionIndex = -1
      else revisionIndex = Math.max(0, Math.min(revisions.length - 1, revisionIndex))
      return { ...session, revisions, revisionIndex }
    })
    if (!trimmed) {
      sessions = sessions.map((session) => ({
        ...session,
        revisions: session.revisions.map((rev, i) =>
          i < session.revisions.length - 1
            ? { ...rev, agentTrace: undefined, knowledgeSources: undefined }
            : rev,
        ),
      }))
      if (measure(sessions) <= MAX_STORE_BYTES) break
      break
    }
  }

  if (!sessions.length) {
    return { store: createEmptyStore(), meta: EMPTY_PRUNE_META }
  }

  let activeSessionId = store.activeSessionId
  if (!sessions.some((s) => s.id === activeSessionId)) {
    sessions = sessions.toSorted((a, b) => b.updatedAt - a.updatedAt)
    activeSessionId = sessions[0].id
  }

  const nextStore = { activeSessionId, sessions }
  const meta: PruneStoreMeta = {
    sessionsDropped,
    revisionsTrimmed,
    changed: sessionsDropped > 0 || revisionsTrimmed > 0,
  }
  return { store: nextStore, meta }
}

export function pruneStore(store: PlaygroundSessionsStore): PlaygroundSessionsStore {
  return pruneStoreWithMeta(store).store
}

export function getActiveSession(
  store: PlaygroundSessionsStore,
): PlaygroundSession {
  return (
    store.sessions.find((s) => s.id === store.activeSessionId) ??
    store.sessions[0] ??
    createEmptySession()
  )
}

export function updateActiveSessionWithMeta(
  store: PlaygroundSessionsStore,
  updater: (session: PlaygroundSession) => PlaygroundSession,
  touch = true,
): PruneStoreResult {
  const active = getActiveSession(store)
  const next = updater(active)
  const updated: PlaygroundSession = touch
    ? { ...next, updatedAt: Date.now() }
    : next
  const sessions = store.sessions.map((s) =>
    s.id === active.id ? updated : s,
  )
  if (!store.sessions.some((s) => s.id === active.id)) {
    sessions.push(updated)
  }
  return pruneStoreWithMeta({
    activeSessionId: updated.id,
    sessions,
  })
}

export function createAndActivateSessionWithMeta(
  store: PlaygroundSessionsStore,
): PruneStoreResult {
  const session = createEmptySession()
  return pruneStoreWithMeta({
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

export function deleteSessionWithMeta(
  store: PlaygroundSessionsStore,
  sessionId: string,
): PruneStoreResult {
  let remaining = store.sessions.filter((s) => s.id !== sessionId)
  if (!remaining.length) {
    return { store: createEmptyStore(), meta: EMPTY_PRUNE_META }
  }
  let activeSessionId = store.activeSessionId
  if (activeSessionId === sessionId) {
    remaining = remaining.toSorted((a, b) => b.updatedAt - a.updatedAt)
    activeSessionId = remaining[0].id
  }
  return pruneStoreWithMeta({ activeSessionId, sessions: remaining })
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

export function buildPlaygroundMemoryHistory(
  session: PlaygroundSession,
): PlaygroundMemoryTurn[] {
  const upToCurrent =
    session.revisionIndex < 0
      ? []
      : session.revisions.slice(0, session.revisionIndex + 1)

  const turns: PlaygroundMemoryTurn[] = upToCurrent.map((rev) => ({
    instruction: rev.instruction,
    explanation: rev.explanation,
    origin: rev.origin,
    createdAt: rev.createdAt,
    warnings: rev.warnings?.length ? rev.warnings : undefined,
    validation: rev.validation,
    project: rev.project,
  }))

  const failed = session.lastFailedAttempt
  if (failed?.instruction?.trim()) {
    const baseProject = upToCurrent.at(-1)?.project
    turns.push({
      instruction: failed.instruction,
      explanation: '',
      origin: failed.origin,
      createdAt: failed.finishedAt,
      project: baseProject,
      failed: true,
      error: failed.error,
    })
  }

  return turns.slice(-MAX_REVISIONS)
}

export function pushRevision(
  session: PlaygroundSession,
  revision: Omit<PlaygroundRevision, 'id'> & { id?: string },
): PlaygroundSession {
  const revisions = session.revisions.slice(0, session.revisionIndex + 1)
  revisions.push({
    ...revision,
    id: revision.id || createRevisionId(),
  })
  const nextTitle =
    session.title.trim() ||
    (revision.origin !== 'runtime-repair' && revision.origin !== 'manual'
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

export function pushManualEditRevision(
  session: PlaygroundSession,
  nextProject: TappPlaygroundProject,
  fileLabel: string,
  labels?: {
    instruction?: string
    explanation?: string
  },
  now = Date.now(),
): PlaygroundSession {
  const current = session.revisions[session.revisionIndex]
  if (!current) return session

  try {
    if (JSON.stringify(current.project) === JSON.stringify(nextProject)) {
      return session
    }
  } catch {
  }

  const instruction = labels?.instruction ?? `Manual edit: ${fileLabel}`
  const explanation = labels?.explanation ?? `Updated ${fileLabel}`
  const last = session.revisions[session.revisionIndex]
  const canMerge =
    last &&
    last.origin === 'manual' &&
    session.revisionIndex === session.revisions.length - 1 &&
    now - last.createdAt <= MANUAL_REVISION_MERGE_MS

  if (canMerge) {
    const revisions = session.revisions.slice()
    revisions[session.revisionIndex] = {
      ...last,
      project: nextProject,
      instruction,
      explanation,
      createdAt: now,
      origin: 'manual',
      warnings: [],
    }
    return {
      ...session,
      revisions,
      lastFailedAttempt: null,
      updatedAt: now,
    }
  }

  return pushRevision(session, {
    project: nextProject,
    explanation,
    instruction,
    warnings: [],
    createdAt: now,
    origin: 'manual',
  })
}

export function loadSessionsStore(): PlaygroundSessionsStore {
  if (typeof window === 'undefined') return createEmptyStore()

  try {
    const raw = localStorage.getItem(SESSIONS_STORAGE_KEY)
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
  }

  const migrated = loadV1Session()
  if (migrated && (migrated.revisions.length > 0 || migrated.lastFailedAttempt)) {
    const store = pruneStore({
      activeSessionId: migrated.id,
      sessions: [migrated],
    })
    saveSessionsStore(store)
    try {
      sessionStorage.removeItem(LEGACY_SESSION_STORAGE_KEY)
    } catch {
    }
    return store
  }

  return createEmptyStore()
}

export function saveSessionsStore(
  store: PlaygroundSessionsStore,
): PruneStoreMeta {
  if (typeof window === 'undefined') return EMPTY_PRUNE_META
  const { store: pruned, meta } = pruneStoreWithMeta(store)
  try {
    localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(pruned))
    return meta
  } catch {
    try {
      const beforeIds = new Set(pruned.sessions.map((s) => s.id))
      const beforeRevCount = pruned.sessions.reduce(
        (n, s) => n + s.revisions.length,
        0,
      )
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
      localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(emergency))
      const afterRevCount = emergency.sessions.reduce(
        (n, s) => n + s.revisions.length,
        0,
      )
      const dropped = Iterator.from(beforeIds)
        .filter((id) => !emergency.sessions.some((s) => s.id === id))
        .toArray().length
      const trimmed = Math.max(0, beforeRevCount - afterRevCount)
      return {
        sessionsDropped: meta.sessionsDropped + dropped,
        revisionsTrimmed: meta.revisionsTrimmed + trimmed,
        changed: meta.changed || dropped > 0 || trimmed > 0,
      }
    } catch {
      return meta
    }
  }
}
