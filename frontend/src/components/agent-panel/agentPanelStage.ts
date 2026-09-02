/**
 * 展开的状态机。
 *
 * 收起 / Quick Overlay / Full 是同一块东西的三个大小，不是三个页面，所以「现在是
 * 哪一档」和「正在变形吗」得分开记：收起动画没播完就把内容卸载会硬切。
 *
 * 没有复用顶部控制面板那套 —— 它是为「顶部小条长成大面板」调的，锚点、方向、
 * 还要和导航岛交接的情况都不一样。但它踩过的坑照搬了两条：
 * - 变形途中冻结悬停效果，否则鼠标扫过会抖
 * - 动画结束事件可能丢，必须有超时兜底，不能让状态卡在变形中
 *
 * 入场靠「先落到 opening，下一帧再 settled」让 CSS transition 真的播出来；
 * 退场在 closing 上等到时长结束才卸 DOM。
 */

/** 展开到第几档。`island` 是收起（不再常驻一枚状态胶囊）。 */
export type AgentPanelStage = 'island' | 'overlay' | 'full'

export type AgentPanelPhase = 'settled' | 'opening' | 'closing'

export interface AgentPanelStageState {
  stage: AgentPanelStage
  phase: AgentPanelPhase
}

export type AgentPanelStageAction =
  | { type: 'open'; stage: Exclude<AgentPanelStage, 'island'> }
  | { type: 'close' }
  | { type: 'toggle'; stage: Exclude<AgentPanelStage, 'island'> }
  /** 动画播完（transitionend 或超时兜底） */
  | { type: 'settle' }

export const INITIAL_AGENT_PANEL_STAGE: AgentPanelStageState = {
  stage: 'island',
  phase: 'settled',
}

/** 入场时长，和 CSS `--agent-move` 对齐。 */
export const AGENT_PANEL_ENTER_MS = 480

/** 退场和入场同一套时长，只是方向倒过来。 */
export const AGENT_PANEL_EXIT_MS = AGENT_PANEL_ENTER_MS

/** 等 transitionend 的宽限。丢事件时靠它把状态推回 settled。 */
export const AGENT_PANEL_SETTLE_SLACK_MS = 140

/** 对话/历史逐张收起：每张错开的间隔，和 CSS `--agent-stagger-step` 对齐。 */
export const AGENT_ROW_STAGGER_MS = 72

export const AGENT_ROW_STAGGER_MAX = 8

/** 单张退场时长，和 CSS `--agent-row-exit` 对齐。 */
export const AGENT_ROW_EXIT_MS = 320

/** 首屏 / 可见卡片的 delay 步数。JS 写成 min(n, MAX)+1，上限 9。 */
export function agentPanelStaggerSteps(count: number): number {
  return Math.min(Math.max(Math.floor(count), 0), AGENT_ROW_STAGGER_MAX + 1)
}

/** 卡片收完：最远那张的 delay + 单张时长。短列表按张数收，不空等满波。 */
export function agentPanelRowWaveMs(
  count = AGENT_ROW_STAGGER_MAX + 1,
): number {
  return AGENT_ROW_EXIT_MS + AGENT_ROW_STAGGER_MS * agentPanelStaggerSteps(count)
}

export function agentPanelStageReducer(
  state: AgentPanelStageState,
  action: AgentPanelStageAction,
): AgentPanelStageState {
  switch (action.type) {
    case 'open': {
      // 已经在这一档就别重播动画 —— 重复长按不该让面板闪一下
      if (state.stage === action.stage && state.phase !== 'closing') {
        return state.phase === 'settled'
          ? state
          : { stage: action.stage, phase: 'opening' }
      }
      return { stage: action.stage, phase: 'opening' }
    }

    case 'toggle':
      return state.stage === action.stage && state.phase !== 'closing'
        ? agentPanelStageReducer(state, { type: 'close' })
        : agentPanelStageReducer(state, { type: 'open', stage: action.stage })

    case 'close': {
      if (state.stage === 'island') return state
      // 收起时先留在原档演动画，settle 之后才卸掉
      return { stage: state.stage, phase: 'closing' }
    }

    case 'settle':
      if (state.phase === 'settled') return state
      return state.phase === 'closing'
        ? INITIAL_AGENT_PANEL_STAGE
        : { stage: state.stage, phase: 'settled' }

    default:
      return state
  }
}

/** 内容是否还该留在 DOM 里（收起动画期间仍然要留）。 */
export function agentPanelShowsStage(
  state: AgentPanelStageState,
  stage: Exclude<AgentPanelStage, 'island'>,
): boolean {
  return state.stage === stage
}

/** 展开着（含正在展开）—— 用来决定要不要接管键盘、点外部收起。 */
export function agentPanelIsOpen(state: AgentPanelStageState): boolean {
  return state.stage !== 'island' && state.phase !== 'closing'
}

/** 超时兜底的等待时长。展开对话/历史时要等卡片收完、输入行再走一步。 */
export function agentPanelSettleTimeoutMs(
  stage: AgentPanelStage = 'overlay',
  count?: number,
): number {
  if (stage !== 'full') return AGENT_PANEL_EXIT_MS + AGENT_PANEL_SETTLE_SLACK_MS
  const steps = agentPanelStaggerSteps(count ?? AGENT_ROW_STAGGER_MAX + 1)
  return (
    AGENT_ROW_EXIT_MS +
    AGENT_ROW_STAGGER_MS * (steps + 1) +
    AGENT_PANEL_SETTLE_SLACK_MS
  )
}
