/**
 * 「它正在做什么」的那几步。
 *
 * 助手多走一步，用户就多等一会儿 —— 等待本身不可怕，不知道在等什么才可怕。所以
 * 这里的目标不是把执行细节全摊开，而是回答两个问题：**现在卡在哪一步、一共花了
 * 多久。**更细的追踪（参数、输出预览、Planner 推理）是调试用的，不进这一层。
 *
 * 只算，不拼字：文案按语言走，交给 i18n。
 */

export type AgentStepStatus = 'pending' | 'running' | 'done' | 'error'

export interface AgentMessageStep {
  id: string
  name: string
  status: AgentStepStatus
  /** 这一步花了多久。还没跑完就没有。 */
  durationMs?: number
  /** 一行补充：重试原因、失败原因、输出摘要 */
  note?: string
}

export interface AgentStepsSummary {
  /** 正在跑的那一步叫什么。都跑完了就是 null。 */
  running: string | null
  done: number
  total: number
  /** 已经跑完的那些加起来花了多久。没有任何一步报过时长就是 null。 */
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
    // 一步都没报过时长时不显示 0.0s —— 那看着像「瞬间完成」，其实是没数据
    elapsedMs: timed ? elapsed : null,
    failed,
  }
}

/** 毫秒转成人读的时长。 */
export function formatStepDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const minutes = Math.floor(ms / 60_000)
  const seconds = Math.round((ms % 60_000) / 1000)
  return `${minutes}m${seconds.toString().padStart(2, '0')}s`
}

/**
 * 这条消息值不值得摆一个思考过程。
 *
 * 单步任务把那一步单独列出来是废话 —— 正文本身就是它的结果。两步起才有「过程」。
 *
 * 两个例外都在「正文说不清楚」的时候：还在跑（不说一声界面上就什么都没有），
 * 以及失败了（那一刻流水账就是答案的一部分，哪一步炸的比结论更有用）。
 */
export function stepsWorthShowing(steps: readonly AgentMessageStep[]): boolean {
  if (steps.length > 1) return true
  return steps.some(
    (step) => step.status === 'running' || step.status === 'error',
  )
}

/** 只有空白不算正文 —— 当答会让思考被卸掉，气泡里剩一圈垫。 */
export function nonemptyContent(text: string): string {
  return text.trim() ? text : ''
}

export function messageHasAnswer(message: {
  content: string
  imageUrls?: readonly string[]
  question?: unknown
  suggestions?: readonly string[]
}): boolean {
  return !!(
    nonemptyContent(message.content) ||
    message.imageUrls?.length ||
    message.question ||
    message.suggestions?.length
  )
}

/** 思考淡出时长。高度用跟目标的平滑跟随，收完才卸思考。 */
export const THINKING_FOLD_MS = 400

/** 高度跟随的时间常数（秒）。越小跟得越紧。收比长慢一截，空垫才不会闪。 */
export const BUBBLE_SHRINK_TAU = 0.12
export const BUBBLE_GROW_TAU = 0.07

/**
 * 还在跑、气泡里还没有正文时才摆过程。
 * 答案一出来，过程就收掉 —— 用户要读的是答。
 */
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

/**
 * 气泡要不要摆思考。聊天档仍然收流，只是不画出来。
 */
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

/**
 * 有的模型把思考链写在正文的 `<think>` 里，不走 reasoning_content。
 * 拆开，思考进过程区，标签外的才是答。未闭合时整段都还在想。
 */
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

/**
 * 正文若把思考链又抄了一遍，把那一段剥掉。
 * 太短的不剥 —— 「好的，」这种开场白经常既是判断也是回答的开头。
 */
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
  if (rest.length === 0 || /^\s/.test(rest)) return rest.replace(/^\s+/, '')
  return c
}
