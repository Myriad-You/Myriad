export type AgentStepStatus = 'pending' | 'running' | 'done' | 'error'

export interface AgentMessageStep {
  id: string
  name: string
  status: AgentStepStatus
  durationMs?: number
  note?: string
}

export interface AgentStepsSummary {
  running: string | null
  done: number
  total: number
  elapsedMs: number | null
  failed: boolean
}

export function summarizeAgentSteps(
  steps: readonly AgentMessageStep[],
): AgentStepsSummary {
  let running: string | null = null
  let done = 0
  let elapsed = 0
  let timed = false
  let failed = false

  for (const step of steps) {
    if (step.status === 'running' && running === null) running = step.name
    if (step.status === 'done' || step.status === 'error') done += 1
    if (step.status === 'error') failed = true
    if (typeof step.durationMs === 'number') {
      elapsed += step.durationMs
      timed = true
    }
  }

  return {
    running,
    done,
    total: steps.length,
    elapsedMs: timed ? elapsed : null,
    failed,
  }
}

export function formatStepDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const minutes = Math.floor(ms / 60_000)
  const seconds = Math.round((ms % 60_000) / 1000)
  return `${minutes}m${seconds.toString().padStart(2, '0')}s`
}

/** Multi-step, or a single step that is still running / failed. */
export function stepsWorthShowing(steps: readonly AgentMessageStep[]): boolean {
  if (steps.length > 1) return true
  return steps.some(
    (step) => step.status === 'running' || step.status === 'error',
  )
}

export function nonemptyContent(text: string): string {
  return text.trim() ? text : ''
}

export function messageHasAnswer(message: {
  content: string
  imageUrls?: readonly string[]
  question?: unknown
  suggestions?: readonly string[]
  workOffer?: unknown
}): boolean {
  return !!(
    nonemptyContent(message.content) ||
    message.imageUrls?.length ||
    message.question ||
    message.suggestions?.length ||
    message.workOffer
  )
}

export const THINKING_FOLD_MS = 400

export const BUBBLE_SHRINK_TAU = 0.12
export const BUBBLE_GROW_TAU = 0.07

export function thinkingVisible(
  steps: readonly AgentMessageStep[],
  live: boolean,
  thought?: string,
  hasAnswer = false,
): boolean {
  if (hasAnswer) return false
  if (live) return true
  if (thought?.trim()) return true
  return stepsWorthShowing(steps)
}

export function messageShowsThinking(input: {
  role: string
  hasAnswer: boolean
  streaming: boolean
  hasProcess: boolean
  hideThinking?: boolean
}): boolean {
  if (input.role !== 'assistant' || input.hasAnswer || input.hideThinking) {
    return false
  }
  return input.hasProcess || input.streaming
}

const THINK_OPEN = /<think>/i
const THINK_CLOSE = /<\/think>/i

/** Split `<think>` from the reply; unclosed means still thinking. */
export function splitThinkContent(raw: string): {
  thought: string
  content: string
} {
  if (!raw) return { thought: '', content: '' }
  const openIdx = raw.search(THINK_OPEN)
  if (openIdx < 0) return { thought: '', content: raw }

  const openLen = raw.match(THINK_OPEN)?.[0].length ?? 7
  const afterOpen = raw.slice(openIdx + openLen)
  const before = raw.slice(0, openIdx).trim()
  const closeIdx = afterOpen.search(THINK_CLOSE)
  if (closeIdx < 0) {
    return { thought: afterOpen, content: before }
  }
  const closeLen = afterOpen.match(THINK_CLOSE)?.[0].length ?? 8
  const thought = afterOpen.slice(0, closeIdx).trim()
  const after = afterOpen.slice(closeIdx + closeLen).trim()
  const content = [before, after].filter(Boolean).join('\n\n')
  return { thought, content }
}

/** Strip a copied thought prefix; leave short openers. */
export function peelThoughtFromContent(
  content: string,
  thought: string,
): string {
  const t = thought.trim()
  if (!t || t.length < 16) return content
  const c = content
  if (c.trim() === t) return ''
  if (!c.startsWith(t)) return c
  const rest = c.slice(t.length)
  if (rest.length === 0 || /^\s/.test(rest)) return rest.replaceAll(/^\s+/g, '')
  return c
}
