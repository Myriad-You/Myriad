import type { FrontendAction, FrontendActionType } from '../../services/agent'

export type AgentActionReversibility = 'readonly' | 'undoable' | 'irreversible'

export const ACTION_REVERSIBILITY: Record<
  FrontendActionType,
  AgentActionReversibility
> = {
  query_windows: 'readonly',
  music_get_status: 'readonly',

  navigate: 'undoable',
  brew_open_article: 'undoable',

  // open_window returns true, not a window id
  open_window: 'irreversible',
  // reopen cannot restore inner state
  close_window: 'irreversible',
  // previous foreground window is not recorded
  focus_window: 'irreversible',
  // no snapshot of prior player state
  music_control: 'irreversible',
  music_load_playlist: 'irreversible',
  // no "remove from reading list" action
  reading_list: 'irreversible',
  // no inverse of a page click
  page_interact: 'irreversible',
  // Tapp internals are opaque
  agent_interaction: 'irreversible',
  show_notification: 'readonly',
  // no un-copy
  copy_clipboard: 'irreversible',
  play_audio: 'readonly',
  show_data: 'readonly',
  show_report: 'readonly',
  download_file: 'irreversible',
}

export type UndoableActionType = 'navigate' | 'brew_open_article'

export interface AgentUndoOffer {
  id: string
  actionType: UndoableActionType
  inverse: FrontendAction
  expiresAtMs: number
}

/** Longer than the island linger so long-press can still reach undo. */
export const UNDO_WINDOW_MS = 45_000

export function agentActionReversibility(
  type: FrontendActionType,
): AgentActionReversibility {
  return ACTION_REVERSIBILITY[type] ?? 'irreversible'
}

let undoSequence = 0

/** Inverse from actual path change, not the declared target. */
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
