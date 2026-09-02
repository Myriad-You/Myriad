/**
 * 撤销 —— 「低风险直接执行」的另一半。
 *
 * 直接执行的前提是能回头。所以这里先老实分类：哪些操作根本没改变什么、哪些能
 * 用现有的操作词汇原路退回、哪些退不回去。**退不回去的就不该被归进「直接执行」**，
 * 只能继续走确认。
 *
 * 逆操作本身也是一条 `FrontendAction`，撤销就是再执行一次 —— 不另起一套执行
 * 机制，逆操作能表达的上限就是操作词汇本身的上限。这也是下面那张表里好几项
 * 写着 `irreversible` 的原因：不是做不到，是现在没有能表达「反过来」的那条指令。
 */

import type { FrontendAction, FrontendActionType } from '../../services/agent'

export type AgentActionReversibility = 'readonly' | 'undoable' | 'irreversible'

/**
 * 每种前端操作能不能回头。
 *
 * `readonly`     只是问了一句，什么都没动，没有可撤销的东西
 * `undoable`     能用现有词汇原路退回
 * `irreversible` 退不回去 —— 后面注释写了各自缺什么
 */
export const ACTION_REVERSIBILITY: Record<
  FrontendActionType,
  AgentActionReversibility
> = {
  // 只读
  query_windows: 'readonly',
  music_get_status: 'readonly',

  // 换了路由，退回原路由即可
  navigate: 'undoable',
  brew_open_article: 'undoable',

  // 开窗返回的是 true 不是窗口 id，没法精确关掉刚开的那一扇；
  // 按 tappId 关会误伤同一个 Tapp 的其他窗口
  open_window: 'irreversible',
  // 关掉的窗口重开也回不到原来的内部状态
  close_window: 'irreversible',
  // 没记录之前是哪扇窗在前台
  focus_window: 'irreversible',
  // 播放器要先存一份旧状态才谈得上还原
  music_control: 'irreversible',
  music_load_playlist: 'irreversible',
  // 词汇里没有「从阅读列表移除」这条指令
  reading_list: 'irreversible',
  // 页面里点下去的那一下没有反向操作
  page_interact: 'irreversible',
  // Tapp 内部发生了什么，外面不知道
  agent_interaction: 'irreversible',
  // 只弹了一条 toast
  show_notification: 'readonly',
  // 剪贴板写出去之后没有对应的「取消复制」
  copy_clipboard: 'irreversible',
  // 只是播了一段音频
  play_audio: 'readonly',
  // 把结果摊在对话里 / toast，没有改持久状态
  show_data: 'readonly',
  show_report: 'readonly',
  // 文件已经落到用户磁盘
  download_file: 'irreversible',
}

/** 上面那张表里标着 `undoable` 的那些。文案与它一一对应。 */
export type UndoableActionType = 'navigate' | 'brew_open_article'

export interface AgentUndoOffer {
  /** 一次性 id，避免旧的撤销被重复按下 */
  id: string
  /** 撤销的是哪一类操作，文案在 i18n 里 */
  actionType: UndoableActionType
  /** 撤销时执行的那条指令 */
  inverse: FrontendAction
  expiresAtMs: number
}

/**
 * 撤销机会留多久。
 *
 * 比 `✓` 在岛上停留的时间长得多 —— 撤销要靠长按打开面板才够得着，2 秒钟根本
 * 来不及。到点自己消失，免得过很久之后还能把一件早已忘记的事退回去。
 */
export const UNDO_WINDOW_MS = 45_000

export function agentActionReversibility(
  type: FrontendActionType,
): AgentActionReversibility {
  // 认不出来的类型按退不回去算 —— 提供一个不管用的撤销比不提供更糟
  return ACTION_REVERSIBILITY[type] ?? 'irreversible'
}

let undoSequence = 0

/**
 * 从「做之前在哪」和「做完之后在哪」推出逆操作。
 *
 * 用实际发生的路由变化，而不是指令里声明的目标：指令可能被处理器改写、可能
 * 压根没跳成。没真的变过就没有可撤销的东西。
 */
export function planAgentUndo(input: {
  action: FrontendAction
  beforePath: string
  afterPath: string
  nowMs: number
}): AgentUndoOffer | null {
  const { action, beforePath, afterPath, nowMs } = input
  if (agentActionReversibility(action.type) !== 'undoable') return null
  if (!beforePath || beforePath === afterPath) return null

  undoSequence += 1
  return {
    id: `undo_${nowMs}_${undoSequence}`,
    actionType: action.type as UndoableActionType,
    inverse: {
      type: 'navigate',
      path: beforePath,
      timestamp: nowMs,
    },
    expiresAtMs: nowMs + UNDO_WINDOW_MS,
  }
}
