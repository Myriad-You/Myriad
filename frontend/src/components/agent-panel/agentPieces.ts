import type {
  FrontendAction,
  ProgressEvent,
  SessionMessage,
  WorkPlanItem,
} from '../../services/agent/types'
import type { AgentMessageQuestion } from './agentMessages'
import type { ChatMessage } from './engineTypes'
import { agentService } from '../../services/agent'
import { createStore, useStore } from '../../utils/store'
import { readConfirmationDetail } from './confirmationDetail'

/**
 * The pieces she handed off in the panel's one conversation, each followed on
 * its own Work run: its plan, its question or confirmation (answered on the
 * piece), its result. The backend keeps them; this follows what it lists.
 */
export type PieceState = 'doing' | 'waiting' | 'done' | 'stopped' | 'failed'

export interface PieceQuestion extends AgentMessageQuestion {
  taskId: string
  questionType: string
}

export interface PieceView {
  number: number
  asked: string
  startedAt: number
  sessionId: string
  runId?: string
  state: PieceState
  progress?: number
  plan?: WorkPlanItem[]
  question?: PieceQuestion
  result?: string
}

interface Pieces {
  chat: string | null
  pieces: readonly PieceView[]
}

const store = createStore<Pieces>({ chat: null, pieces: [] })
const followed = new Set<string>()

/** Runs a run's page actions: the engine's own queue, set while it is mounted. */
type ActionRunner = (key: string, actions: readonly FrontendAction[]) => void
let runActions: ActionRunner | null = null

export function setPieceActionRunner(runner: ActionRunner | null): void {
  runActions = runner
}

/**
 * Takes her lines about how pieces ended into conversation `chat`: the
 * engine's own messages, set while it is mounted. Each is kept in the
 * transcript, so a reload reads it back with the rest.
 */
type LineSink = (chat: string, lines: readonly SessionMessage[]) => void
let tellLines: LineSink | null = null

export function setPieceLineSink(sink: LineSink | null): void {
  tellLines = sink
}

function toldLine(raw: unknown): SessionMessage | null {
  if (!raw || typeof raw !== 'object') return null
  const line = raw as Record<string, unknown>
  if (typeof line.id !== 'number' || typeof line.content !== 'string') return null
  return {
    id: line.id,
    role: 'assistant',
    content: line.content,
    metadata:
      line.metadata && typeof line.metadata === 'object'
        ? (line.metadata as Record<string, unknown>)
        : undefined,
    createdAt: text(line.createdAt),
  }
}

/**
 * Her lines about how pieces ended, put among the messages shown by when
 * each ended: those not shown yet, and not older than every message shown
 * (that part of the conversation is not loaded). Exported for tests.
 */
export function withToldLines(
  messages: readonly ChatMessage[],
  lines: readonly ChatMessage[],
): ChatMessage[] {
  const shown = new Set(messages.map((message) => message.id))
  const oldest = messages[0]?.createdAt.getTime() ?? -Infinity
  const added = lines.filter(
    (line) =>
      !shown.has(line.id) &&
      !Number.isNaN(line.createdAt.getTime()) &&
      line.createdAt.getTime() >= oldest,
  )
  if (added.length === 0) return messages as ChatMessage[]
  const next = [...messages]
  for (const line of added.sort((a, b) => a.createdAt.getTime() - b.createdAt.getTime())) {
    const at = next.findIndex((message) => message.createdAt.getTime() > line.createdAt.getTime())
    next.splice(at === -1 ? next.length : at, 0, line)
  }
  return next
}

/** One event of a piece's run: what it says about the piece, and its page actions run. */
function heard(chat: string, number: number, run: string, event: ProgressEvent): void {
  if (store.get().chat !== chat) return
  if (event.type === 'step_completed' && event.frontendActions?.length) {
    // Keyed by the run: the queue skips an action it already ran under a key.
    runActions?.(`piece-${run}`, event.frontendActions)
  }
  const change = pieceChange(event)
  if (change) patch(number, change)
}

export function usePieces(): Pieces {
  return useStore(store)
}

export function getPieces(): Pieces {
  return store.get()
}

function patch(number: number, change: Partial<PieceView>): void {
  store.set((current) => ({
    ...current,
    pieces: current.pieces.map((piece) =>
      piece.number === number ? { ...piece, ...change } : piece,
    ),
  }))
}

type Listed = Record<string, unknown>

function text(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

function questionOf(taskId: unknown, raw: unknown): PieceQuestion | undefined {
  if (!raw || typeof raw !== 'object' || typeof taskId !== 'string') return undefined
  const question = raw as Record<string, unknown>
  const options = Array.isArray(question.options)
    ? (question.options as Array<Record<string, unknown>>).map((option) => ({
        value: text(option.value),
        label: text(option.label) || text(option.value),
        description: text(option.description) || undefined,
      }))
    : undefined
  return {
    id: text(question.question_id ?? question.questionId),
    taskId,
    questionType: text(question.question_type ?? question.questionType) || 'free_text',
    text: text(question.question),
    context: text(question.context) || undefined,
    options,
    detail: readConfirmationDetail(question.detail),
  }
}

/** Her reply that handed a piece off is written before the piece starts. */
const HANDED_OFF_SLACK_MS = 5_000

/**
 * Where each piece shows in the conversation: after the last message that
 * was there when it started (her line handing it off), in start order. A
 * piece older than every shown message goes first (`''`); one newer than
 * every message hangs on the last.
 */
export function placePieces(
  pieces: readonly PieceView[],
  messages: readonly { id: string; at?: number }[],
): Map<string, PieceView[]> {
  const placed = new Map<string, PieceView[]>()
  const ordered = [...pieces].sort(
    (a, b) => a.startedAt - b.startedAt || a.number - b.number,
  )
  for (const piece of ordered) {
    const startedMs = piece.startedAt * 1000 + HANDED_OFF_SLACK_MS
    let after = ''
    for (const message of messages) {
      if (message.at === undefined || message.at <= startedMs) after = message.id
      else break
    }
    const list = placed.get(after) ?? []
    list.push(piece)
    placed.set(after, list)
  }
  return placed
}

/** A piece as the backend lists it. Exported for tests. */
export function readListed(raw: Listed): PieceView | null {
  const number = Number(raw.number)
  if (!Number.isInteger(number) || number < 1) return null
  const state = text(raw.state)
  const detail = (raw.detail ?? {}) as Record<string, unknown>
  return {
    number,
    asked: text(raw.asked),
    startedAt: Number(raw.startedAt) || 0,
    sessionId: text(raw.sessionId),
    runId: text(raw.runId) || undefined,
    state:
      state === 'waiting' || state === 'done' || state === 'stopped' || state === 'failed'
        ? state
        : 'doing',
    progress: typeof detail.progress === 'number' ? detail.progress : undefined,
    question: state === 'waiting' ? questionOf(detail.taskId, detail.question) : undefined,
    result: text(detail.result) || undefined,
  }
}

/**
 * Show the pieces of conversation `chat` as listed, keeping what a followed
 * run already told of each, and follow those still under way; her lines
 * about how they ended go into the conversation.
 */
export function showPieces(
  chat: string,
  listed: readonly unknown[],
  told: readonly unknown[] = [],
): void {
  const known = new Map(
    (store.get().chat === chat ? store.get().pieces : []).map((piece) => [piece.number, piece]),
  )
  const pieces = listed
    .map((raw) => (raw && typeof raw === 'object' ? readListed(raw as Listed) : null))
    .filter((piece): piece is PieceView => piece !== null)
    .map((piece) => {
      const before = known.get(piece.number)
      return before
        ? {
            ...piece,
            plan: before.plan,
            result: piece.result ?? before.result,
            question: piece.question ?? (piece.state === 'waiting' ? before.question : undefined),
          }
        : piece
    })
  store.set({ chat, pieces })
  const lines = told.map(toldLine).filter((line): line is SessionMessage => line !== null)
  if (lines.length > 0) tellLines?.(chat, lines)
  for (const piece of pieces) {
    if (piece.runId && (piece.state === 'doing' || piece.state === 'waiting')) {
      follow(chat, piece.number, piece.runId)
    }
  }
}

export async function loadPieces(chat: string | null): Promise<void> {
  if (!chat) {
    store.set({ chat: null, pieces: [] })
    return
  }
  if (store.get().chat !== chat) store.set({ chat, pieces: [] })
  try {
    const { pieces, told } = await agentService.listPieces(chat)
    showPieces(chat, pieces, told)
  } catch {
    // Kept as shown; the next turn lists them again.
  }
}

/** What one event of a piece's run says about it. Exported for tests. */
export function pieceChange(event: ProgressEvent): Partial<PieceView> | null {
  switch (event.type) {
    case 'work_plan_updated':
      return { plan: event.steps, state: 'doing' }
    case 'progress':
      return { progress: event.progress, state: 'doing' }
    case 'waiting_for_input':
      return {
        state: 'waiting',
        question: {
          id: event.questionId,
          taskId: event.taskId,
          questionType: event.questionType,
          text: event.question,
          context: event.context,
          options: event.options,
          detail: readConfirmationDetail(event.detail),
        },
      }
    case 'task_completed':
      return {
        state: event.success ? 'done' : 'failed',
        question: undefined,
        result: event.response?.message || undefined,
      }
    case 'error':
      return { state: 'failed', result: event.message }
    default:
      return null
  }
}

function follow(chat: string, number: number, runId: string): void {
  if (followed.has(runId)) return
  followed.add(runId)
  const onEvent = (event: ProgressEvent) => heard(chat, number, runId, event)
  void agentService
    .subscribeRun(runId, onEvent, 'work')
    .catch(() => {})
    .finally(() => {
      followed.delete(runId)
      // How it ended, as kept: done, stopped, or waiting for them.
      if (store.get().chat === chat) void loadPieces(chat)
    })
}

/** Answer a piece's question on the piece itself. */
export async function answerPiece(number: number, value: string): Promise<boolean> {
  const { chat, pieces } = store.get()
  const piece = pieces.find((one) => one.number === number)
  const question = piece?.question
  if (!chat || !piece || !question) return false
  patch(number, { question: { ...question, answered: value }, state: 'doing' })
  try {
    await agentService.answerQuestionWithProgress(
      question.taskId,
      question.id,
      value,
      (event) => heard(chat, number, `${question.taskId}:${question.id}`, event),
    )
    return true
  } catch {
    patch(number, { question, state: 'waiting' })
    return false
  } finally {
    if (store.get().chat === chat) void loadPieces(chat)
  }
}

/** Stop one piece, and only it. */
export async function stopPiece(number: number): Promise<void> {
  const { chat } = store.get()
  if (!chat) return
  try {
    if (await agentService.stopPiece(chat, number)) patch(number, { state: 'stopped' })
  } finally {
    void loadPieces(chat)
  }
}
