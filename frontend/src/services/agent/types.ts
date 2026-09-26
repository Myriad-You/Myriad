export type RigMotionStyle = 'restrained' | 'even' | 'open'
export type RigMusicEnergy = 'quiet' | 'soft' | 'present' | 'strong'
export type RigBeatPhase = 'rest' | 'downbeat' | 'pulse' | 'hold'
export type RigBehaviorFunction =
  | 'orient'
  | 'attend'
  | 'acknowledge'
  | 'uncertain'
  | 'prepareSpeech'
  | 'emphasize'
  | 'surprise'
  | 'celebrate'
  | 'relief'
  | 'entrain'
  | 'express'
export type RigBehaviorPhase =
  | 'planned'
  | 'preparing'
  | 'committed'
  | 'holding'
  | 'recovering'
  | 'complete'
  | 'rejected'

export interface RigStateSummary {
  expression: PerformanceBaseline['expression'] | 'steady'
  posture: PerformanceBaseline['posture']
  acting: {
    intent: PerformanceCue['intent'] | null
    phase: PerformancePhase | 'idle'
    function: RigBehaviorFunction | null
    lifecycle: RigBehaviorPhase | null
    remainingMs: number
  }
  activeBehaviors: {
    function: RigBehaviorFunction
    lifecycle: RigBehaviorPhase
    source: string
    resources: string[]
    remainingMs: number
  }[]
  owners: {
    mouth: string
    expression: string
    gaze: string
    headBody: string
  }
  speaking: boolean
  singing: boolean
  musicPlaying: boolean
  music?: {
    energy: RigMusicEnergy
    beat: RigBeatPhase
  }
  capabilities: string[]
  recentIntents: PerformanceCue['intent'][]
  motionStyle: RigMotionStyle
  pageVisible: boolean
  faceVisible: boolean
}

export interface ProcessContext {
  mode?: 'work' | 'chat'
  currentRoute?: string
  activePlatforms?: string[]
  sessionId?: string
  customData?: Record<string, unknown>
  intentionId?: string
  rigState?: RigStateSummary
}

export interface ProcessRequest {
  input: string
  context?: ProcessContext
}

export interface ClarifyRequest {
  originalInput: string
  clarificationId: string
  answer: string
  context?: ProcessContext
}

export type TaskStatus =
  | 'pending'
  | 'running'
  | 'waiting_for_input'
  | 'paused'
  | 'completed'
  | 'failed'
  | 'cancelled'

export interface TaskPendingQuestion {
  questionId: string
  questionType: string
  question: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  required?: boolean
  defaultValue?: string
}

export interface TaskStepHistoryItem {
  stepId: string
  capabilityName: string
  status: string
  durationMs?: number
  outputSummary?: string
  error?: string
  imageUrl?: string
  isDynamic?: boolean
}

export interface TaskInfo {
  taskId: string
  status: TaskStatus
  progress: number
  error?: string
  pendingQuestion?: TaskPendingQuestion
  stepHistory?: TaskStepHistoryItem[]
}

export interface TaskDetail {
  taskId: string
  recipeId: string
  status: string
  progress: number
  startedAt: string
  completedAt?: string
  results?: Record<string, unknown>
  pendingQuestion?: TaskPendingQuestion
  runId?: string
  stepHistory?: TaskStepHistoryItem[]
}

export type ClarificationType =
  'time_range' | 'target' | 'action' | 'missing_parameter' | 'ambiguity'

export interface ClarificationPoint {
  id: string
  type: ClarificationType
  question: string
  options: string[]
  default?: string
}

export type AgentResponseType =
  | 'answer'
  | 'clarification'
  | 'task_created'
  | 'task_progress'
  | 'task_completed'
  | 'error'

export type PerformancePhase =
  'reaction' | 'delivery' | 'outcome' | 'proactive' | 'mood'

export interface PerformanceBaseline {
  expression: 'withdrawn' | 'subdued' | 'steady' | 'warm' | 'tense'
  posture: 'closed' | 'neutral' | 'open'
  motionEnergy: number
  attention: number
}

export interface PerformanceCue {
  intent:
    | 'greet'
    | 'respond'
    | 'question'
    | 'delight'
    | 'emphasize'
    | 'listen'
    | 'notify'
    | 'think'
    | 'dizzy'
    | 'cry'
    | 'angry'
    | 'speechless'
    | 'maniac'
    | 'silly'
    | 'lovestruck'
  atMs: number
  intensity: number
  tempo: number
  fadeInMs: number
  fadeOutMs: number
  holdMs?: number
  interrupt: 'replace' | 'queue' | 'if-lower'
}

export interface PerformanceDirective {
  phrases?: SpeechPhrase[]
  phase: PerformancePhase
  moodRevision: number
  motionStyle: RigMotionStyle
  plan: {
    baseline?: PerformanceBaseline
    cues: PerformanceCue[]
  }
}

export interface SpeechPhrase {
  text: string
  intent:
    'ask' | 'hesitate' | 'tease' | 'explain' | 'check-in' | 'laugh' | 'none'
}

export type MoodBandName = 'floor' | 'sad' | 'tense' | 'calm' | 'excited'

export interface MoodTransition {
  before: number
  after: number
  arousalBefore?: number
  arousalAfter?: number
  bandBefore: MoodBandName
  bandAfter: MoodBandName
  delta: number
  cause: string
  revision: number
}

export interface AgentResponse {
  success: boolean
  responseType: AgentResponseType
  message: string
  data?: unknown
  dataDisplay?: DataDisplayHint
  suggestions: string[]
  task?: TaskInfo
  frontendAction?: FrontendAction
  performance?: PerformanceDirective
  sessionId?: string
}

/** Resume via runId; do not create a new task. */
export interface RunStartedEvent {
  type: 'run_started'
  runId: string
  sessionId?: string
}

export interface TaskCreatedEvent {
  type: 'task_created'
  taskId: string
  message: string
  totalSteps: number
  stepDescriptions?: string[]
  queuePosition?: number
  skillId?: string
  skillName?: string
}

export interface StepStartedEvent {
  type: 'step_started'
  stepId: string
  stepIndex: number
  totalSteps: number
  capabilityName: string
  description: string
  capabilityCategory?: string
  retryAttempt?: number
}

export interface StepCompletedEvent {
  type: 'step_completed'
  stepId: string
  stepIndex: number
  success: boolean
  durationMs: number
  outputSummary?: string
  tierUsed?: 'pro' | 'standard'
  degraded?: boolean
  imageUrl?: string
  /** Run immediately; do not wait for the recipe. */
  frontendActions?: FrontendAction[]
}

export interface StepRetryingEvent {
  type: 'step_retrying'
  stepId: string
  stepIndex: number
  retryCount: number
  maxRetries: number
  reason: string
}

export interface ProgressUpdateEvent {
  type: 'progress'
  progress: number
  completedSteps: number
  totalSteps: number
  message: string
}

export interface TaskCompletedEvent {
  type: 'task_completed'
  taskId: string
  success: boolean
  response: AgentResponse
}

export interface WaitingForInputEvent {
  type: 'waiting_for_input'
  taskId: string
  questionId: string
  questionType: string
  question: string
  context?: string
  options?: Array<{ value: string; label: string; description?: string }>
  required: boolean
  defaultValue?: string
}

export interface ErrorEvent {
  type: 'error'
  taskId?: string
  message: string
  code: string
}

export interface SummaryTokenEvent {
  type: 'summary_token'
  token: string
  done: boolean
}

export interface ThinkingTokenEvent {
  type: 'thinking_token'
  token: string
  done: boolean
}

export interface PerformancePlanEvent {
  type: 'performance_plan'
  performance: PerformanceDirective
}

export interface MeropeStateChangedEvent {
  type: 'merope_state_changed'
  mood: MoodTransition
  activity: string
}

export interface OutfitOverlayEvent {
  type: 'outfit_overlay'
  outfitId: string | null
}

export interface MusicControlEvent {
  type: 'music_control'
  action: string
}

/** What she is doing on her own right now (`GET /agent/doing`). */
export type MeropeThing =
  | {
      kind: 'song'
      id: string
      source: string
      name: string
      artist: string
      album: string
      cover: string
      durationMs: number
    }
  | { kind: 'note'; itemId: number; title: string }

export interface MeropeDoingResponse {
  doing: { thing: MeropeThing; started: string; ends: string } | null
  /** Server clock, so where she is in it does not depend on this device's clock. */
  now: string
}

export interface TaskAssignedEvent {
  type: 'task_assigned'
  taskId: string
  assignment: TaskAssignment
}

export interface PlannerDecisionEvent {
  type: 'planner_decision'
  status: string
  reasoning?: string
  confidence: number
  steps: Array<{
    id: string
    capabilityId: string
    action: string
    params?: Record<string, unknown>
  }>
  userRequest: string
}

export interface StepDebugEvent {
  type: 'step_debug'
  stepId: string
  phase: 'start' | 'complete'
  capabilityId: string
  directive?: string
  userRequest?: string
  params?: Record<string, unknown>
  outputPreview?: string
  isDynamic: boolean
  durationMs?: number
  success?: boolean
  error?: string
}

export interface TaskAssignment {
  agents: AgentAssignment[]
  totalAgents: number
  isMultiAgent: boolean
  tierMix: 'pro' | 'standard' | 'mixed'
}

export interface AgentAssignment {
  role: AgentRole
  agentId: string
  displayName: string
  icon: string
  tier: 'Pro' | 'Standard'
  capabilities: string[]
}

export type AgentRole =
  | 'orchestrator'
  | 'data_worker'
  | 'content_worker'
  | 'creative_worker'
  | 'system_worker'

export interface SessionCreatedEvent {
  type: 'session_created'
  sessionId: string
}

export interface SessionTitleUpdatedEvent {
  type: 'session_title_updated'
  title: string
}

export type ProgressEvent =
  | WorkPlanUpdatedEvent
  | RunStartedEvent
  | TaskCreatedEvent
  | TaskAssignedEvent
  | StepStartedEvent
  | StepCompletedEvent
  | StepRetryingEvent
  | ProgressUpdateEvent
  | TaskCompletedEvent
  | WaitingForInputEvent
  | ErrorEvent
  | SessionCreatedEvent
  | SessionTitleUpdatedEvent
  | SummaryTokenEvent
  | ThinkingTokenEvent
  | PerformancePlanEvent
  | MeropeStateChangedEvent
  | OutfitOverlayEvent
  | MusicControlEvent
  | PlannerDecisionEvent
  | StepDebugEvent

export type ProgressCallback = (event: ProgressEvent) => void

export interface WorkPlanItem {
  description: string
  status: 'pending' | 'in_progress' | 'completed'
}

export interface WorkPlanUpdatedEvent {
  type: 'work_plan_updated'
  taskId: string
  steps: WorkPlanItem[]
}

export interface ColumnDef {
  field: string
  title: string
  width?: number
  sortable: boolean
}

export type DataDisplayHint =
  | { type: 'table'; columns: ColumnDef[]; dataPath?: string }
  | { type: 'chart'; chartType: string; xField: string; yField: string }
  | {
      type: 'card_list'
      titleField: string
      descriptionField?: string
      imageField?: string
    }
  | { type: 'markdown' }
  | { type: 'key_value' }
  | { type: 'timeline'; timeField: string; contentField: string }
  | { type: 'raw' }

export type FrontendActionType =
  | 'query_windows'
  | 'open_window'
  | 'close_window'
  | 'focus_window'
  | 'agent_interaction'
  | 'navigate'
  | 'page_interact'
  | 'phantasi_open_article'
  | 'music_control'
  | 'music_get_status'
  | 'music_load_playlist'
  | 'reading_list'
  | 'show_notification'
  | 'copy_clipboard'
  | 'play_audio'
  | 'show_data'
  | 'download_file'
  | 'show_report'

export interface WindowTarget {
  windowId?: string
  tappId?: string
  tappName?: string
  position?: 'active' | 'left' | 'right' | 'next' | 'previous' | 'all'
}

export interface PageElementTarget {
  selector?: string
  testId?: string
  text?: string
  ariaLabel?: string
  role?: string
  index?: number
}

export interface ScrollOptions {
  direction?: 'top' | 'bottom' | 'left' | 'right'
  offset?: number
  smooth?: boolean
}

export interface WaitCondition {
  visible?: boolean
  timeout?: number
}

export interface ReadingListPayload {
  items?: Array<{
    id: number
    title: string
    source_name: string
    published_at: string
    reason?: string
  }>
  name?: string
}

export interface FrontendAction {
  type: FrontendActionType
  target?: WindowTarget | PageElementTarget
  tappId?: string
  windowId?: string
  interactionId?: string
  script?: string
  timestamp: number
  size?: { width?: number; height?: number }
  position?: { x?: number; y?: number }
  data?: Record<string, unknown>
  path?: string
  params?: Record<string, unknown>
  query?: Record<string, unknown>
  fullPath?: string
  action?: string
  value?: string | number | boolean
  replace?: boolean
  playlistId?: string
  source?: string
  autoPlay?: boolean
  selector?: string
  scrollOptions?: ScrollOptions
  waitFor?: WaitCondition
  payload?: ReadingListPayload
  criteria?: string
}

export interface Capability {
  id: string
  name: string
  description: string
  category: string
  actions: string[]
  requiresAi: boolean
}

export type PresetType = 'favorite' | 'history'

export interface TaskPreset {
  id: number
  input: string
  presetType: PresetType
  parsedSteps?: unknown
  intentSummary?: string
  title?: string
  conversationData?: ConversationMessage[]
  hasConversation?: boolean
  lastUsedAt: string
  useCount: number
  createdAt: string
}

export interface TaskPresetListResponse {
  favorites: TaskPreset[]
  history: TaskPreset[]
}

export interface CreatePresetRequest {
  input: string
  presetType: PresetType
  parsedSteps?: unknown
  intentSummary?: string
  title?: string
  conversationData?: ConversationMessage[]
}

export interface ConversationMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
  metadata?: Record<string, unknown>
  createdAt: string
}

export interface SessionInfo {
  id: string
  mode?: 'work' | 'chat'
  title: string | null
  messageCount: number
  archived: boolean
  createdAt: string
  lastActiveAt: string
}

export interface SessionMessage {
  id: number
  role: 'user' | 'assistant' | 'system'
  content: string
  taskId?: string
  metadata?: Record<string, unknown>
  createdAt: string
}

export interface QueueStatus {
  total_lanes: number
  max_concurrent: number
  available_permits: number
  waiting?: number
}

export interface HeartbeatTask {
  id: string
  name: string
  schedule: string
  action: string
  enabled: boolean
  lastRun?: string
  lastResult?: string
}

export interface StepTrace {
  stepId: string
  capabilityId: string
  tierUsed: string
  durationMs: number
  tokensIn?: number
  tokensOut?: number
  success: boolean
  error?: string
  action?: string
  params?: Record<string, unknown>
  outputPreview?: string
  isDynamic?: boolean
}

export interface ExecutionTrace {
  traceId: string
  totalDurationMs: number
  tierUsage: Record<string, number>
  steps: StepTrace[]
  plannerDecision?: {
    status: string
    reasoning?: string
    confidence: number
    plannedSteps: Array<{
      id: string
      capabilityId: string
      action: string
      params?: Record<string, unknown>
    }>
  }
}

/** A memory as the site admin sees it: whose it is and where it came from. */
export interface ManagedMemory {
  id: string
  content: string
  memoryType: string
  source: string
  /** Hers (she lived it), a person's (learned in private), or a group's. */
  scope: 'her' | 'person' | 'group'
  /** Where it came from, finer than scope (`experience`, `bit`, `stranger`, …). */
  category: string
  personId?: number | null
  personName?: string | null
  /** The group, as `platform:chat` (`telegram:-100123`). */
  group?: string | null
  /** Her note on someone from outside the community: who. */
  strangerName?: string | null
  createdAt: string
  updatedAt?: string
}

export interface SkillInfo {
  id: string
  name: string
  description: string
  category: string
  origin: 'manual' | 'agent_generated' | 'agent_improved'
  successCount?: number
  failureCount?: number
  tierHint?: 'pro' | 'standard'
}
