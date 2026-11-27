// Every declaration the entries name, with what they refer to. Behaviour types keep only the
// members the site uses; data types are whole. Rendering types come from @myriad-you/figure-player.
import type { Anime25DDriver, Anime25DPlayback, Anime25DPlaybackDocument, Anime25DPlaybackV7, Anime25DTouchRegion, MeropeRigManifest, Anime25DDebugSnapshot as StageDebugSnapshot, VisibleTouchHit } from '@myriad-you/figure-player'
import type { Anime25DTurnKeyforms, ArmVariantGesture, CharacterAssetProfile, RigBone, RigMotionProfile, RigOutfitProfile, RigPoint, RigSemanticAnchor, RigSemantics, RigSize, RigSpatialProfile } from '@myriad-you/figure-player/internals'
import type { PoseCorrection, PoseCorrectionStageSource, PoseCorrectionSurface } from '@myriad-you/figure-player/workbench'

export declare function activityExpressionDriverPatch(thinking: boolean): Readonly<ActivityExpressionDriver>

/** The stage (Anime25DStage from @myriad-you/figure-player) driven by her performance: speech, song, directed poses, behaviors and touch. */
export declare class Anime25DPlayer {
  getPresentedTouch(): PresentedTouchReaction | null
  constructor(canvas: HTMLCanvasElement, playback: Anime25DPlaybackDocument, rigManifest?: MeropeRigManifest, trace?: FigureTrace)
  /**
   * The last outfit keeps drawing until the next one is ready, and stays exactly as it was if the next one fails
   * (Anime25DStage.replaceLivePackage).
   */
  replaceLivePackage(playback: Anime25DPlaybackDocument, rigManifest: MeropeRigManifest | undefined, atlasUrl: string): Promise<void>
  /** Workbench-only override. Never changes the immutable playback package. */
  previewPoseCorrections(corrections: readonly PoseCorrection[] | null): void
  /** Workbench-only: what correction picking reads of the live frame; null once disposed. */
  poseStage(): PoseCorrectionStageSource | null
  hitTestTouch(clientX: number, clientY: number): VisibleTouchHit | null
  getMotionCapabilities(): readonly string[]
  setTarget(partial: Partial<Anime25DDriver>): void
  replaceTarget(driver: Anime25DDriver): void
  playFace(face: string | null, shift: string | null, via?: readonly string[]): void
  blinkNow(): void
  setMouse(x: number, y: number, inside: boolean): void
  setMotionPolicy(policy: MotionChannelPolicy): void
  getMotionPolicy(): MotionChannelPolicy
  setSpeechActive(active: boolean): void
  setSinging(active: boolean): void
  setSingingTrack(trackId: string | null): void
  setMusicSignal(drive: MusicMotionSignal | null): void
  setScore(score: ResolvedScore | null, nowMs: number): void
  setSpeechProsody(plan: SpeechProsodyPlan | null): void
  enqueueSpeechText(text: string, locale?: string): void
  clearSpeechText(): void
  setBehaviorMotionUnits(units: readonly Anime25DMotionUnit[], nowMs: number): void
  clearBehaviorMotionUnits(): void
  setBearing(bearing: PerformanceBaseline | null): void
  debugSnapshot(): Anime25DDebugSnapshot
  resize(cssWidth: number, cssHeight: number, devicePixelRatio: number): void
  tick(deltaSeconds: number): void
  dispose(): void
}

export declare function anime25DRuntimeKey(sourceMasterAssetId: string | null | undefined, contractVersion: number | null | undefined, atlasUrl: string): string

/** Anime2.5D-only authoring and diagnostics */
export interface Anime25DWorkbenchPort {
  previewPoseCorrections: (corrections: readonly PoseCorrection[] | null) => void
  pickPoseCorrectionPoint: (clientX: number, clientY: number) => PoseCorrectionStagePick | null
  projectPoseCorrectionPatch: (correction: Pick<PoseCorrection, 'surface' | 'at'>, patch: Readonly<PoseCorrection['patches'][number]>) => PoseCorrectionStagePoint | null
  poseCorrectionDelta: (surface: PoseCorrectionSurface, clientDx: number, clientDy: number) => { dx: number; dy: number } | null
  /** The part of the stage on show, on screen. */
  poseCorrectionStageRect: () => DOMRect | null
  /** Zoom the stage onto the head while fixing poses; a large head stays as is. */
  zoomPoseCorrectionHead: (on: boolean) => void
  setDriver: (partial: Partial<Anime25DDriver>) => void
  replaceDriver: (driver: Anime25DDriver) => void
  blinkNow: () => void
  /**
   * Her face takes a look (the contract's facePresets; null for none) the way
   * the director's does, by `shift` (faceShifts), passing through `via` on the way.
   */
  playFace: (face: string | null, shift: string | null, via?: readonly string[]) => void
  debugSnapshot: () => Anime25DDebugSnapshot | null
}

/** A full base refresh must not take mouth ownership during active speech. */
export declare function idleSpeechDriverPatch(mood: number, speechActive: boolean): Partial<SpeechOwnedDriver>

export declare function intersectionKeepsAnime25DVisible(entry: {
  isIntersecting: boolean
  boundingClientRect: { width: number; height: number }
}): boolean

export declare function realizeAnime25DBehaviorPlan(plan: BehaviorPlan, nowMs: number): Anime25DBehaviorRealization

export declare function shouldAnimateAnime25D(state: Anime25DAnimationState): boolean

export declare function shouldUseAnime25DRuntime(input: {
  hasManifest: boolean
  hasPlayback: boolean
  atlasUrl: string
  runtimeKey: string
  failedRuntimeKey: string | null
}): boolean

export declare function speechArticulationDriverPatch(articulation: SpeechArticulation): EnergyDriverPatch

/** Keep preview speech disabled so the player never combines it with an unrelated random mouth. */
export declare function speechEnergyDriverPatch(energy: number | null): EnergyDriverPatch

/** They typed a character to her; she attends while it goes on. */
export declare function dispatchMeropeListening(channel: FigureChannel): void

export declare function dispatchMeropePerformance(channel: FigureChannel, detail: unknown): void

export declare function dispatchMeropeState(channel: FigureChannel, value: unknown): void

export declare const MEROPE_STATE_EVENT: 'merope-state'

export interface MeropePerformanceEventDetail {
  text: string
  source: 'reply' | 'proactive' | 'interaction' | 'preview'
  messageId?: string
  runId?: string
  generation?: number
  motionIntentId?: string
  performance?: PerformanceDirective
}

export declare function meropeStateEventDetail(value: unknown): MeropeStateEventDetail | null

export interface MeropeStateEventDetail {
  mood: MoodTransition
  activity: string
}

/** A slow GET must not undo a newer live event */
export declare function resolveLoadedMeropeAffect(channel: FigureChannel, snapshot: {
  mood?: number
  arousal?: number
  moodRevision?: number
  activity?: string
}): { mood: number; arousal: number }

export declare function sanitizePerformanceDirective(value: unknown): PerformanceDirective | null

export declare function bindCharacterTouch(canvas: HTMLCanvasElement, hit: (x: number, y: number) => { region: TouchRegion | null } | null, emit: (touch: TouchObservation, now: number) => void, cancelReaction: () => void): CharacterTouchBinding

/** One bounded request per semantic revision, never one per pointer frame. */
export declare class TouchAppraisal {
  constructor(deps: Dependencies)
  observe(touch: TouchObservation, revision: number): void
  dispose(): void
  cancel(): void
}

export declare class TouchEncounter {
  constructor(publish: (summary: TouchCompletion) => void, now?: () => number)
  observe(touch: TouchObservation, displayedReaction?: TouchReaction | null): void
  cancel(): void
}

/** The only production writer. */
export declare function applyMotionFrame(rig: Pick<
    RigMotionPort,
    | 'setMotionPolicy'
    | 'setBearing'
    | 'setScore'
    | 'setMood'
    | 'setSpeechActive'
    | 'setAutoSpeech'
    | 'setSpeechEnergy'
    | 'setSpeechArticulation'
    | 'setSpeechProsody'
    | 'enqueueSpeechText'
    | 'playBehaviorPlan'
    | 'stopBehaviorPlan'
    | 'setSinging'
    | 'setSingingTrack'
    | 'setMusicSignal'
  >, frame: MotionFrame, state: MotionApplyState, onRealizer?: (feedback: MotionRealizerFeedback) => void): MotionApplyState

export interface BehaviorPlan {
  id: string
  originMs: number
  metadata?: Readonly<Record<string, string | number | boolean>>
  pegs: readonly TimePeg[]
  behaviors: readonly ScheduledBehavior[]
}

export declare function captureRigStateSummary(runtime: MotionRuntime, nowMs?: number): RigStateSummary

export declare function createLiveMotionRuntime(coordinator: RigMotionCoordinator, musicSource: MusicMotionSource | null, channel: FigureChannel): MotionRuntime

export declare function createMotionApplyState(): MotionApplyState

/** Its own coordinator, so a preview never takes the live figure's channels; it hears the channel it is given. */
export declare function createPreviewMotionRuntime(channel?: FigureChannel): MotionRuntime

export interface LiveFaceConsumer {
  update: (state: LiveFaceConsumerState) => void
  release: () => void
}

export interface MotionChannelPolicy {
  mouth: MotionSourceId
  expression: MotionSourceId
  gaze: MotionSourceId
  headBody: MotionSourceId
}

/** @behaviour Only the members the site uses. */
export declare class MotionRuntime {
  readonly speech: SpeechMotionSource
  readonly performance: PerformanceMotionSource
  readonly mood: MoodMotionSource
  readonly touch: TouchMotionSource
  constructor(coordinator: RigMotionCoordinator, musicSource?: MusicMotionSource | null, channel?: FigureChannel)
  readonly channel: FigureChannel
  retain(): () => void
  setCapabilities(capabilities: readonly string[]): void
  attachLiveFaceConsumer(initial: LiveFaceConsumerState): LiveFaceConsumer
  summaryFacts(): RigSummaryFacts
  reportBehaviorRealizer(planId: string, behaviorId: string, result: 'accepted' | 'rejected', nowMs?: number, reason?: 'unsupported-form' | 'invalid-timing' | 'superseded' | undefined): void
  subscribe(listener: MotionFrameListener): () => void
}

export interface MusicMotionAudio {
  getCurrentAudio: () => {
    paused: boolean
    currentTime: number
    ended?: boolean
  } | null
  getMotionAudioFeatures: (audio: {
    paused: boolean
    currentTime: number
  }) => Readonly<MotionAudioFeatures> | null
  connectAudioToAnalyser: (audio: { paused: boolean }) => boolean
}

export interface MusicMotionClock {
  now: () => number
  raf: (callback: (time: number) => void) => number
  caf: (id: number) => void
}

export declare class MusicMotionSource {
  constructor(coordinator: RigMotionCoordinator, clock: MusicMotionClock, audio: MusicMotionAudio, visibility: MusicMotionVisibility, compileTimeline?: SingingTimelineCompiler)
  setPlayback(playing: boolean, switching: boolean): void
  markSwitching(): void
  setTrack(track: MusicTrackInput): void
}

export interface MusicMotionVisibility {
  isPageVisible: () => boolean
  onVisibility: (callback: (visible: boolean) => void) => () => void
}

/** The beats of the current score that have found their moment so far. */
export interface ResolvedScore {
  id: number
  beats: readonly ResolvedScoreBeat[]
  /**
   * Her standing acting, for while nothing else is going on: it plays as
   * strongly as she is free for it, and with `loopMs` plays again each period.
   */
  standing?: { loopMs: number }
}

export interface RigBearing extends PerformanceBaseline {
  revision: number
}

export declare class RigMotionCoordinator {
  constructor(trace?: FigureTrace)
  leaseCount(): number
}

/**
 * A separate candidate plan, not a replacement of the reply director plan.
 * @behaviour Only the members the site uses.
 */
export declare class TouchMotionSource {
  /** Freeze the evidence actually sent for this completion, not a later contact. */
  expectSpeech(owner: string, reaction: TouchReaction | null, nowMs: number): void
  cancelExpectedSpeech(owner: string): void
  notePresented(owner: string, receipt: PresentedTouchReaction | null, nowMs: number): void
  displayedReaction(owner: string, contactId: number): TouchReaction | null
  constructor(coordinator: RigMotionCoordinator, changed: () => void)
  current(): BehaviorPlan | null
  /** Continue a sampled attitude, never an unrendered model result or a new caress. */
  accompanySpeech(messageId: string, nowMs: number): void
  version(): number
  /** A delayed co-speech decoration cannot turn this same refusal into a joke. */
  acceptsSpeechRefinement(messageId: string, directive: PerformanceDirective, nowMs: number): boolean
  refine(owner: string, revision: number, reaction: TouchReaction, nowMs: number): void
  update(owner: string, touch: TouchObservation, nowMs: number): void
  release(owner?: string, preserveEncounter?: boolean): void
}

export type BodyControl = 'lovestruck' | 'sparkles' | 'blush' | 'dizzy' | 'cry' | 'angry' | 'speechless' | 'maniac' | 'silly' | 'headNod' | 'headTurn' | 'gazeHorizontal' | 'gazeVertical' | 'eyeOpenLeft' | 'eyeOpenRight' | 'leftArmRaise' | 'rightArmRaise' | 'torsoRise' | 'browLift' | 'eyeWide' | 'headTilt' | 'torsoTurn' | 'torsoLean' | 'torsoPitch' | 'leftArmSwing' | 'rightArmSwing' | 'eyeSmile' | 'browTiltLeft' | 'browTiltRight' | 'mouthSmile' | 'mouthOpen' | 'mouthWide' | 'mouthRound' | 'mouthNarrow' | 'mouthSeal' | 'hairSway' | 'chestSway' | 'musicNotes' | 'questionMark' | 'sleepZ' | 'leftHandWave' | 'leftHandPoint' | 'leftHandChest' | 'leftHandChin' | 'rightHandWave' | 'rightHandPoint' | 'rightHandChest' | 'rightHandChin'

export interface BodyPose {
  targets: Partial<Record<BodyControl, number>>
  transitionMs: number
  /** Zero keeps the pose until revised or explicitly released. */
  holdMs: number
}

export type MeropeActivity = 'idle' | 'thinking' | 'talking'

export declare function moodBand(mood: number | undefined, arousal?: number | undefined): MoodBand

export type MoodBand = 'floor' | 'sad' | 'tense' | 'calm' | 'excited'

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

export interface PerformanceBaseline {
  expression: 'withdrawn' | 'subdued' | 'steady' | 'warm' | 'tense'
  posture: 'closed' | 'neutral' | 'open'
  motionEnergy: number
  attention: number
  pose?: BodyPose
  /** The contract's look her face holds (facePresets), whose life the body plays. */
  face?: string
  /** How she gets from the look before to this one (faceShifts); naturally without. */
  shift?: string
  /** Looks passed through on the way, in order and without stopping (facePresets names, or 'plain'). */
  via?: string[]
}

export interface PerformanceCue {
  intent: 'lovestruck' | 'greet' | 'respond' | 'question' | 'delight' | 'emphasize' | 'listen' | 'notify' | 'think' | 'dizzy' | 'cry' | 'angry' | 'speechless' | 'maniac' | 'silly'
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
  /** Beats on the reply's words or on time: acting that unfolds beat by beat. */
  score?: ScoreBeat[]
  /** Standing acting only: the score plays again each this many milliseconds. */
  loopMs?: number
  phase: PerformancePhase
  moodRevision: number
  motionStyle: RigMotionStyle
  plan: {
    baseline?: PerformanceBaseline
    cues: PerformanceCue[]
  }
}

export type PerformancePhase = 'reaction' | 'delivery' | 'outcome' | 'proactive' | 'mood' | 'presence'

export type RigBeatPhase = 'rest' | 'downbeat' | 'pulse' | 'hold'

export type RigBehaviorFunction = | 'orient'
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

export type RigBehaviorPhase = | 'planned'
  | 'preparing'
  | 'committed'
  | 'holding'
  | 'recovering'
  | 'complete'
  | 'rejected'

export type RigMotionStyle = 'restrained' | 'even' | 'open'

export type RigMusicEnergy = 'quiet' | 'soft' | 'present' | 'strong'

export interface RigStateSummary {
  expression: 'withdrawn' | 'subdued' | 'steady' | 'warm' | 'tense'
  posture: 'closed' | 'neutral' | 'open'
  acting: { intent: PerformanceCue['intent'] | null; phase: PerformancePhase | 'idle'; function: RigBehaviorFunction | null; lifecycle: RigBehaviorPhase | null; remainingMs: number; }
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
  recentIntents: ('lovestruck' | 'greet' | 'respond' | 'question' | 'delight' | 'emphasize' | 'listen' | 'notify' | 'think' | 'dizzy' | 'cry' | 'angry' | 'speechless' | 'maniac' | 'silly')[]
  motionStyle: RigMotionStyle
  pageVisible: boolean
  faceVisible: boolean
  /** The look her face holds now (facePresets), for the director to change from. */
  face?: string
}

/**
 * One beat of the director's score: on words of the reply as they are
 * spoken (`text`, shifted by `offsetMs`), or `atMs` after the score arrives.
 */
export interface ScoreBeat {
  text?: string
  atMs: number
  offsetMs: number
  /** Targets reached from this moment and held until revised or `holdMs` passes. */
  pose?: BodyPose
  move?: ScoreMove
}

export type ScoreDirection = 'left' | 'right' | 'up-left' | 'up' | 'down' | 'up-right' | 'down-left' | 'down-right'

/** A movement that goes and comes back by itself, such as a nod. */
export interface ScoreMove {
  kind: ScoreMoveKind
  side?: ScoreSide
  direction?: ScoreDirection
  amount: number
  count: number
  tempo: number
}

export type ScoreMoveKind = 'startle' | 'nod' | 'blink' | 'glance' | 'bounce' | 'sigh' | 'shake' | 'wink' | 'beat' | 'shrug'

export type ScoreSide = 'left' | 'right' | 'both'

export interface SpeechPhrase {
  text: string
  intent: 'ask' | 'hesitate' | 'tease' | 'explain' | 'check-in' | 'laugh' | 'none'
}

/**
 * A gesture arm the image model drew on the portrait: `side` is the arm it
 * changed on the picture, `hand` points on that hand and `keypoints` the pose
 * model's joints on the whole picture (COCO-WholeBody `[x, y, score]`), in
 * the portrait's pixels.
 */
export interface ArmVariantSource {
  gesture: ArmVariantGesture
  side: 'left' | 'right'
  url: string
  hand?: ReadonlyArray<readonly [number, number]>
  keypoints?: ReadonlyArray<readonly [number, number, number]>
}

/** Remembers the keys that go with a decomposition file until it is imported. */
export declare function attachTurnKeyforms(file: Blob, keys: DocumentTurnKeyforms): void

/**
 * Expression variants of the portrait redrawn by an image model: the same
 * picture with only the face changed. Each one is cut against the face layer
 * into the replacement parts the rig already fades between. A redraw replaces
 * procedural glyphs and the generic closed eyes the rigger ships; only parts
 * the artist drew in the PSD win over it.
 */
export declare const AUTHORED_EXPRESSION_KINDS: readonly ['cry', 'squeeze', 'close']

export type AuthoredExpressionKind = 'cry' | 'squeeze' | 'close'

/**
 * The joints the backend found on a master portrait (DWPose, COCO-WholeBody):
 * `[x, y, score]` per keypoint, in that portrait's own pixels.
 */
export interface DetectedSkeleton {
  sourceMasterAssetId: string
  model: string
  width: number
  height: number
  keypoints: ReadonlyArray<readonly [number, number, number]>
}

export declare function diagnoseRig(manifest: MeropeRigManifest, motionExposure?: readonly MotionExposureFinding[]): RigDiagnosticReport

export declare function isDocumentTurnKeyforms(value: unknown): value is DocumentTurnKeyforms

export declare function isLiveMeropeManifest(value: unknown): value is MeropeRigManifest

export declare function isRigManifest(value: unknown): value is MeropeRigManifest

export interface MeropeRigImportSource {
  rigIrVersion?: number
  characterAssetContractVersion: number
  /** Absent on a bust. */
  profile?: CharacterAssetProfile
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  canvas: RigSize
  atlas: {
    id: string
    width: number
    height: number
  }
  bones: RigBone[]
  layers: RigLayerSource[]
  motionProfile?: RigMotionProfile
  outfitProfile?: RigOutfitProfile
  semanticAnchors?: Record<string, RigSemanticAnchor>
  semantics?: RigSemantics
  spatialProfile?: RigSpatialProfile
  anime25dPlayback?: Anime25DPlaybackV7
}

export type PreparedRigPsdImport = PreparedAnime25DRigImport

export declare function prepareRigPsdImport(file: File, arg1: RigPsdImportOptions): Promise<PreparedRigPsdImport>

export interface RigMotionPort {
  setMotionPolicy: (policy: MotionChannelPolicy) => void
  setBearing: (bearing: RigBearing | null) => void
  /** The director's score, placed on the clock; a rig without one ignores it. */
  setScore?: (score: ResolvedScore | null) => void
  setMood: (mood: number, activity: MeropeActivity) => void
  setSpeechActive: (active: boolean) => void
  setAutoSpeech: (active: boolean) => void
  setSpeechEnergy: (energy: number | null) => void
  setSpeechArticulation: (articulation: SpeechArticulation) => void
  setSpeechProsody: (prosody: SpeechProsodyPlan | null) => void
  enqueueSpeechText: (text: string, locale?: string) => void
  setSinging: (active: boolean) => void
  setSingingTrack: (trackId: string | null) => void
  setMusicSignal: (drive: MusicMotionSignal | null) => void
  playBehaviorPlan: (plan: BehaviorPlan) => readonly BehaviorRealizerReport[]
  stopBehaviorPlan: (planId?: string) => void
}

export interface RigPsdImportOptions {
  sourceMasterAssetId: string
  /** Where the API serving the master portrait lives; empty is the page's own origin. */
  apiUrl: string
  onStage?: (stage: 'validated' | 'packing') => void
  sourceGenerationFingerprint?: string
  signal?: AbortSignal
  expressions?: ReadonlyArray<{ kind: AuthoredExpressionKind; url: string }>
  /** Gesture arms drawn on the portrait; only a bust takes them. */
  armVariants?: ReadonlyArray<ArmVariantSource>
  /** The bust by default. */
  profile?: CharacterAssetProfile
  skeleton?: DetectedSkeleton
  /** The picture the decomposition was cut from when it is not the master itself (an enlarged figure's). */
  referenceUrl?: string
}

/** Same atlas + contract → keep the mounted player instead of tearing WebGL down. */
export declare function sameLiveFaceRuntime(current: MeropeRigManifest | null | undefined, next: MeropeRigManifest | null | undefined): boolean

export interface SpeechArticulation {
  energy: number | null
  viseme: SpeechViseme
  amount: number
}

export type SpeechViseme = 'rest' | 'closed' | 'open' | 'wide' | 'round' | 'narrow'

export declare function analyzeMotionAudio(waveform: Float32Array, decibels: Float32Array, sampleRate: number, output: MotionAudioFeatures, playbackVolume?: number): MotionAudioFeatures

export interface LyricLine {
  time: number
  text: string
  translation?: string
}

export interface MotionAudioFeatures {
  energy: number
  bass: number
  pulse: number
  presence: number
}

/** Live evidence only. */
export interface MusicMotionSignal {
  sampleTimeSeconds: number
  audio: Readonly<MotionAudioFeatures> | null
  beatFrame: Readonly<BeatFrame>
  phrase: Readonly<MusicPhrase> | null
}

export interface WordLyricLine {
  time: number
  duration: number
  text: string
  words: WordLyricToken[]
  translation?: string
}

export interface WordLyricToken {
  time: number
  duration: number
  text: string
}

/**
 * The text's predicted accents on the audio that says them. With `place`
 * (where each text offset is said, in seconds) an accent lands where its
 * words are spoken; without it, at its share of the audio's length.
 */
export declare function alignTextProsody(input: TextProsodyInput, audio: SpeechProsodyTimeline, place?: ((textOffset: number) => number) | null): SpeechProsodyTimeline

export declare function alignVisemeTimeline(cues: readonly TextVisemeCue[], audioDurationSeconds: number): VisemeSpan[]

/**
 * The text's mouth shapes laid on the audio as it was spoken: the mouth
 * starts with the voice, stops with it, and the text's pauses (its commas
 * and full stops) fall in the audio's own pauses, in order; between those
 * anchors the shapes are spread evenly. Without voicing, spread over all.
 */
export declare function alignVisemeTimelineToAudio(cues: readonly TextVisemeCue[], audioDurationSeconds: number, voicing: AudioVoicing | null): VisemeSpan[]

/** Voiced start and end, and the pauses between, from the decoded samples. */
export declare function audioVoicing(samples: Float32Array, sampleRate: number): AudioVoicing | null

/** Converts streamed display text to a compact visual-only speech timeline. */
export declare function compileTextVisemes(text: string, locale?: string): Promise<TextVisemeCue[]>

export declare function dispatchMeropeSpeech(channel: FigureChannel, detail: unknown): void

export declare function dispatchMeropeSpeechUtterance(channel: FigureChannel, utterance: SpeechUtteranceInput): void

/** How long a reply shown without audio keeps her mouth moving. */
export declare function estimateAutoSpeechDurationMs(text: string, locale?: string): number

export declare const MEROPE_SPEECH_EVENT: 'merope-speech'

export declare function meropeSpeechEventDetail(value: unknown): MeropeSpeechEventDetail | null

export type MeropeSpeechEventDetail = | (SpeechEventBase & { phase: 'start' | 'end' })
  | (SpeechEventBase & { phase: 'chunk'; text: string })
  | (SpeechEventBase & { phase: 'energy'; energy: number })
  | (SpeechEventBase & {
      phase: 'articulation'
      articulation: SpeechArticulation
    })
  | (SpeechEventBase & {
      phase: 'prosody'
      prosody: SpeechProsodyPlan
      text?: string
    })
  | (Omit<SpeechEventBase, 'utteranceId'> & {
      phase: 'cancel'
      utteranceId?: string
    })

export type MeropeSpeechSource = 'reply' | 'proactive' | 'interaction' | 'preview'

export interface SpeechProsodyPlan extends SpeechProsodyTimeline {
  utteranceId: string
  startedAtMs: number
}

export declare function speechProsodyTimeline(spans: readonly VisemeSpan[], durationSeconds: number): SpeechProsodyTimeline

export interface SpeechProsodyTimeline {
  durationMs: number
  accents: readonly SpeechAccentAnchor[]
}

export interface SpeechUtteranceInput {
  messageId: string
  source: MeropeSpeechSource
  text: string
  utteranceId: string
  locale?: string
  generation?: number
}

/**
 * Where a place in the text (a code unit offset) is said, in seconds of its
 * audio: the text starts with the voice and ends with it, and its pauses
 * (punctuation) fall in the audio's pauses, in order; between those anchors
 * by its share of the characters. Null without voicing.
 */
export declare function textTimeOnAudio(text: string, voicing: AudioVoicing | null): ((textOffset: number) => number) | null

export interface TextVisemeCue {
  viseme: SpeechViseme
  duration: number
  emphasis: boolean
  /**
   * How far this vowel opens against its shape's usual opening: a u is a
   * smaller round than an o, an i a flatter spread than an e. Default 1.
   */
  openness?: number
}

export declare const VISEME_SILENCE_ENERGY: 0.06

export declare function visemeAmount(energy: number, emphasis: boolean): number

export declare function visemeAt(spans: readonly VisemeSpan[], seconds: number): VisemeSpan | null

/** Loudness is not a mouth shape. */
export interface VisemeSpan {
  viseme: SpeechViseme
  endsAt: number
  emphasis: boolean
}

export declare const ANGRY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

export declare const ANIME25D_PSD_DEFECT_COLORS: Readonly<
  Record<Anime25DPsdDefectKind, readonly [number, number, number]>
>

export declare const ANIME25D_PSD_REPAIRED_COLOR: readonly [34, 197, 94]

/** The stage's snapshot (Anime25DStage.debugSnapshot), with what her motion envelope moved between channels. */
export interface Anime25DDebugSnapshot extends StageDebugSnapshot {
  motionEnvelope: { highCollar: boolean; armMotion: boolean; pitchLimit: number; torsoLimit: number; armLimit: number; } & { transferredEnergy: number; }
}

/**
 * Why the rest composite disagrees with the source illustration at a spot:
 * - `buried`: a lower layer holds the right art but another layer covers it.
 * - `missing`: the illustration shows art there that no layer carries.
 * - `spurious`: a layer paints where the illustration shows only background.
 * - `mismatch`: the visible layer holds different art and none below fits.
 */
export type Anime25DPsdDefectKind = 'buried' | 'missing' | 'spurious' | 'mismatch'

export interface Anime25DPsdReconciliation {
  status: 'reconciled' | 'reference-mismatch'
  /** Share of opaque composite pixels whose colour matches the source. */
  agreement: number
  /** Opaque composite pixels compared against the source. */
  contentArea: number
  /** False when the source background is not flat enough to tell art from it. */
  backgroundKnown: boolean
  regions: Anime25DPsdDefectRegion[]
  /** Regions import already fixed; `regions` lists what remains after that. */
  repairs: { revealed: number; recovered: number }
  /** Source-shaded overview of the content with defect pixels coloured. */
  heatmap: { width: number; height: number; data: Uint8ClampedArray } | null
}

export declare function appendPoseCorrectionPatch(current: readonly PoseCorrection[], candidate: PoseCorrection): { corrections: PoseCorrection[]; index: number; patch: number } | null

export declare const BODY_CONTROL_DRIVERS: { readonly headTurn: 'headYaw'; readonly headNod: 'headPitch'; readonly headTilt: 'headRoll'; readonly torsoTurn: 'torsoTurn'; readonly torsoLean: 'bodyLean'; readonly torsoRise: 'bodyLift'; readonly torsoPitch: 'bodyPitch'; readonly gazeHorizontal: 'gazeX'; readonly gazeVertical: 'gazeY'; readonly leftArmRaise: 'armRaiseL'; readonly rightArmRaise: 'armRaiseR'; readonly leftArmSwing: 'armSwingL'; readonly rightArmSwing: 'armSwingR'; readonly eyeOpenLeft: 'lidL'; readonly eyeOpenRight: 'lidR'; readonly eyeSmile: 'eyeSmile'; readonly eyeWide: 'eyeWide'; readonly browLift: 'browLift'; readonly browTiltLeft: 'browTiltL'; readonly browTiltRight: 'browTiltR'; readonly mouthSmile: 'lipCurve'; readonly mouthOpen: 'jaw'; readonly mouthWide: 'mouthWide'; readonly mouthRound: 'mouthRound'; readonly mouthNarrow: 'mouthNarrow'; readonly mouthSeal: 'mouthSeal'; readonly angry: 'anger'; readonly speechless: 'speechless'; readonly maniac: 'maniac'; readonly silly: 'silly'; readonly lovestruck: 'lovestruck'; readonly cry: 'eyeCry'; readonly dizzy: 'eyeDizzy'; readonly hairSway: 'fhAmp'; readonly chestSway: 'bust'; readonly musicNotes: 'accentNotes'; readonly questionMark: 'accentQuestion'; readonly sleepZ: 'accentSleep'; readonly sparkles: 'accentSparkle'; readonly blush: 'accentBlush'; readonly leftHandWave: 'handWaveL'; readonly leftHandPoint: 'handPointL'; readonly leftHandChest: 'handChestL'; readonly leftHandChin: 'handChinL'; readonly rightHandWave: 'handWaveR'; readonly rightHandPoint: 'handPointR'; readonly rightHandChest: 'handChestR'; readonly rightHandChin: 'handChinR'; }

/** Patch bounds the package accepts. */
export declare function clampPoseCorrectionPatch(patch: { x: number; y: number; radiusX: number; radiusY: number; dx: number; dy: number; }): { x: number; y: number; radiusX: number; radiusY: number; dx: number; dy: number; }

/** Patches added, removed or changed since the saved package. */
export declare function countPoseCorrectionChanges(baseline: readonly PoseCorrection[], draft: readonly PoseCorrection[]): number

export declare const CRY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

export declare const DIZZY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

/** Whole looks of her face: each control at one of its named tiers. */
export declare const FACE_PRESETS: Record<string, { description: string; targets: Record<string, string>; life?: RawFaceLife; }>

export declare const LOVESTRUCK_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

export declare const MANIAC_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

/**
 * Semantic up is positive; the renderer's image-space Y points down. A
 * directed head tilt says how much it should show, as a share of the full
 * tilt (HEAD_TILT_RADIANS): the director's slight (0.35) is about 8°, clear
 * (0.65) about 15° and strong the full 24°. Everyday motion keeps the low,
 * slow end of headRoll to itself.
 */
export declare function mappedControlValue(key: BodyControl, value: number): number

/** The named region a patch sits on, when one is close enough to name it by. */
export declare function nearestPoseCorrectionRegion(playback: Anime25DPlayback, surface: Surface, point: Point): PoseCorrectionRegion | null

/** A still patch at a point, its reach sized to the feature it sits on. */
export declare function newPoseCorrectionPatch(playback: Anime25DPlayback, surface: Surface, point: Point): { x: number; y: number; radiusX: number; radiusY: number; dx: number; dy: number; }

export declare const POSE_CORRECTION_REGIONS: readonly ['leftEye', 'rightEye', 'mouth', 'chin', 'frontCrown', 'rearCrown']

/**
 * The pose a correction made here keys on: the head turn and nod held now,
 * plus the eye or mouth the patch sits on when that is closed or open. A
 * hair patch never waits for a blink.
 */
export declare function poseCorrectionCorner(playback: Anime25DPlayback, driver: Anime25DDriver, surface: Surface, point: Point): { at: PoseCorrection['at']; } | { problem: PoseCorrectionProblem; }

export declare function poseCorrectionPreviewDriver(correction: PoseCorrection, current?: Anime25DDriver): Anime25DDriver

export type PoseCorrectionRegion = 'leftEye' | 'rightEye' | 'mouth' | 'chin' | 'frontCrown' | 'rearCrown'

/** Where a named region sits on this asset, in head radii, if the asset has it. */
export declare function poseCorrectionRegionPoint(playback: Anime25DPlayback, region: PoseCorrectionRegion): (Point & { surface: Surface }) | null

/** A correctable point under the pointer: its surface and rest place in head radii. */
export interface PoseCorrectionStagePick {
  surface: PoseCorrectionSurface
  x: number
  y: number
}

/** The pose part way from facing front to a correction's own pose. */
export declare function poseCorrectionTransitionDriver(correction: PoseCorrection, current: Anime25DDriver, t: number): Anime25DDriver

/** Preview outranks every live source on this coordinator only */
export declare class PreviewMotionScope {
  take(channels?: readonly MotionChannel[]): void
  policy(): MotionChannelPolicy
}

export declare const SILLY_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

export declare const SPEECHLESS_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

export declare const SQUEEZE_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

export declare const THINKING_EXPRESSION_PRESET: Readonly<Partial<Anime25DDriver>>

/** A named tier ("clear", "-slight") as a control value, the way the director's are read. */
export declare function tierExtent(control: BodyControl, tier: string): number | null

/**
 * The page's line to a live figure. The page dispatches performances, speech,
 * her state and their typing on it; the figure's motion sources listen on it.
 * It also knows which turn is live, her last state, and where the figure
 * reports what it did with a turn. The page keeps one for the figure it hosts
 * and hands it to that figure's runtime; a runtime given none has its own,
 * which nothing outside it can reach.
 */
export declare class FigureChannel extends EventTarget {
  constructor(trace?: FigureTrace)
  readonly trace: FigureTrace
  setGeneration(generation: number): void
  generation(): number
  /** Unstamped events belong to whatever turn is live. */
  isLive(generation: number | undefined): boolean
  newIntentId(): string
  resetState(): void
}

/**
 * The figure's error. What the stage throws (its two codes, both in FIGURE_ERROR_TEXT) is one too:
 * `error instanceof FigureError` holds for either, with the same `name`, `code` and `params`.
 */
export declare class FigureError extends Error {
  constructor(code: FigureErrorCode, params?: FigureErrorParams)
  readonly code: FigureErrorCode
  readonly params: FigureErrorParams
}

export type ActivityExpressionDriver = Pick<
  Anime25DDriver,
  | 'headYaw'
  | 'headPitch'
  | 'headRoll'
  | 'lidL'
  | 'lidR'
  | 'eyeDizzy'
  | 'eyeSqueeze'
  | 'eyeCry'
  | 'anger'
  | 'speechless'
  | 'maniac'
  | 'silly'
  | 'lovestruck'
  | 'gazeX'
  | 'gazeY'
  | 'irisScale'
  | 'browLift'
  | 'browTiltL'
  | 'browTiltR'
  | 'browTilt'
>

/** A sampled response submitted to the renderer, not a requested plan. */
export interface PresentedTouchReaction {
  behaviorId: string
  reaction: TouchReaction
  atMs: number
}

/**
 * Where the figure reports what it did with a turn, for the page's own trace.
 * Whoever builds a player, coordinator or runtime hands it one; without one
 * the figure keeps that to itself.
 */
export interface FigureTrace {
  mark: (span: string, extra?: Record<string, string | number | boolean>) => void
  drop: (reason: FigureTraceDrop) => void
  leaseExpiry: (count: number) => void
  frame: (sample: FigureFrameSample) => void
}

export interface Anime25DMotionUnit {
  touch?: { x: number; y: number; strokeX: number; strokeY: number; caress?: number }
  behaviorId: string
  family: 'co-speech' | 'music' | 'performance' | 'touch'
  form: string
  kind: BehaviorKind
  timing: {
    startMs: number
    readyMs: number
    strokeStartMs: number
    strokePeakMs: number
    strokeEndMs: number
    relaxMs: number | null
    endMs: number | null
  }
  intensity: number
  quality: BehaviorQuality
}

/**
 * A patch on screen: where its spot is drawn without it, where it pushes the
 * spot, and how long one head radius is there.
 */
export interface PoseCorrectionStagePoint {
  originX: number
  originY: number
  pushedX: number
  pushedY: number
  /** The share of the correction this pose applies; the push shows at it. */
  weight: number
  unitX: number
  unitY: number
}

export type SpeechOwnedDriver = Pick<
  Anime25DDriver,
  'eyeSmile' | 'lipCurve' | 'jaw' | 'talking'
>

export interface Anime25DBehaviorRealization {
  units: readonly Anime25DMotionUnit[]
  reports: readonly BehaviorRealizerReport[]
}

export interface Anime25DAnimationState {
  atlasReady: boolean
  pageVisible: boolean
  inViewport: boolean
  cancelled: boolean
}

export type EnergyDriverPatch = Pick<
  Anime25DDriver,
  | 'jaw'
  | 'mouthWide'
  | 'mouthRound'
  | 'mouthNarrow'
  | 'mouthSeal'
  | 'talking'
>

/** Pointer evidence only */
export type TouchRegion = Anime25DTouchRegion

export interface TouchObservation {
  /** Canvas-relative contact, local only */
  position?: { x: number; y: number }
  id: number
  phase: 'start' | 'update' | 'end' | 'cancel'
  gesture: TouchGesture
  region: TouchRegion
  durationMs: number
  distance: number
  speed: number
  repeatCount: number
  x: number
  y: number
}

export interface CharacterTouchBinding {
  /** Drop the touch in progress, as when the page's reason to be touched is gone. */
  cancel: () => void
  unbind: () => void
}

export interface Dependencies {
  request: (summary: TouchSummary, signal: AbortSignal) => Promise<unknown>
  apply: (revision: number, reaction: TouchReaction) => void
  now: () => number
}

export type TouchCompletion = TouchSummary & { displayedReaction: TouchReaction | null }

export type TouchReaction = 'hesitate' | 'notice' | 'accept' | 'withdraw'

export interface MotionFrame {
  snapshot: MotionSnapshot
  bearing: RigBearing | null
  /** The director's score, placed on the clock so far. */
  score: ResolvedScore | null
  /** Her standing acting, as the director last gave it. */
  standingScore: ResolvedScore | null
  speech: SpeechIntent | null
  performance: PerformanceIntent | null
  music: SingingFrame | null
  mood: MoodIntent | null
  behaviorPlan: BehaviorPlan | null
  behaviorRevision: number
  behaviors: readonly BehaviorSnapshot[]
}

export interface MotionApplyState {
  speechTextSeq: number
  behaviorRevision: number | null
  behaviorPlanId: string | null
  bearing: RigBearing | null
  score: ResolvedScore | null
  standingScore: ResolvedScore | null
  speechOwnedMouth: boolean
  speechProsodyKey: string | null
}

export interface MotionRealizerFeedback {
  planId: string
  behaviorId: string
  result: 'accepted' | 'rejected'
  atMs: number
  reason?: 'unsupported-form' | 'invalid-timing' | 'superseded' | undefined
}

export interface TimePeg {
  id: string
  atMs: number
  revision: number
  confidence?: number
}

export interface ScheduledBehavior {
  id: string
  function: BehaviorFunction
  kind: BehaviorKind
  source: MotionSourceId
  resources: readonly BehaviorResource[]
  channels: readonly MotionChannel[]
  timing: BehaviorTiming
  anticipation?: string
  form: BehaviorForm
  intensity: number
  quality?: Partial<BehaviorQuality>
  confidence?: number
}

export interface LiveFaceConsumerState {
  ready: boolean
  mood: number
  arousal: number
  activity: MeropeActivity
  capabilities: readonly string[]
  priority?: number
}

export type MotionSourceId = 'mood' | 'idle' | 'preview' | 'speech' | 'performance' | 'music' | 'coSpeech' | 'ambient'

export declare class SpeechMotionSource {
  constructor(coordinator: RigMotionCoordinator, onChange: (intent: SpeechIntent) => void, activeBehaviors?: () => readonly BehaviorSnapshot[], scheduler?: SpeechLifecycleScheduler, channel?: FigureChannel)
  hasPlayback(scope: {
    messageId: string
    generation?: number
    source: string
  }): boolean
  upcomingText(scope: {
    messageId: string
    generation?: number
    source: string
  }): string
}

export declare class PerformanceMotionSource {
  constructor(coordinator: RigMotionCoordinator, onChange: (intent: PerformanceIntent) => void, externalBehaviors?: () => readonly BehaviorSnapshot[], onDirective?: (
      directive: PerformanceDirective,
      event: MeropePerformanceEventDetail | undefined,
      plan: BehaviorPlan | null,
    ) => void, channel?: FigureChannel)
  handle(detail: MeropePerformanceEventDetail): void
}

/** Expression only; thinking is gated as ambient. */
export declare class MoodMotionSource {
  constructor(coordinator: RigMotionCoordinator, onChange: (
      intent: MoodIntent,
      bandChanged: boolean,
    ) => void)
  set(mood: number, activity: MeropeActivity, arousal?: number): void
}

export interface RigSummaryFacts {
  capabilities: string[]
  recentIntents: ('lovestruck' | 'greet' | 'respond' | 'question' | 'delight' | 'emphasize' | 'listen' | 'notify' | 'think' | 'dizzy' | 'cry' | 'angry' | 'speechless' | 'maniac' | 'silly')[]
  motionStyle: RigMotionStyle
  faceVisible: boolean
}

export type MotionFrameListener = (frame: MotionFrame) => void

export type SingingTimelineCompiler = (input: SingingTimelineInput) => Promise<SingingCue[]>

export interface MusicTrackInput {
  trackId: string
  duration?: number
  verbatim?: readonly WordLyricLine[] | undefined
  lines?: readonly LyricLine[] | undefined
}

/** A beat placed on the clock (`performance.now()` milliseconds). */
export interface ResolvedScoreBeat {
  id: string
  atMs: number
  pose?: BodyPose
  move?: ScoreMove
}

/** Keys measured on a decomposition, in its document's pixels. */
export interface DocumentTurnKeyforms {
  canvas: [number, number]
  keyforms: Anime25DTurnKeyforms
  /** A standing figure's body turn keys (bodyTurnKeyforms), on the same document. */
  bodyKeyforms?: Anime25DTurnKeyforms
}

export interface MotionExposureFinding {
  check: MotionExposureCheck
  side: EyeSide | null
  /** Hole pixels the motion uncovers, as a share of the moving part's paint. */
  share: number
  /** Where the hole opens, in layer (canvas) pixels. */
  bounds: { x: number; y: number; width: number; height: number }
}

export interface RigDiagnosticReport {
  profile?: 'face-rig' | 'anime25d'
  score: number
  issues: RigDiagnostic[]
  capabilities: {
    facial: boolean
    lipSync: boolean
    gaze: boolean
    secondaryMotion: boolean
    facialVariants: boolean
    deformableSkinning: boolean
    outfitAware: boolean
    collisionAware: boolean
    presentationCoverage: boolean
  }
}

export interface RigLayerSource {
  id: string
  textureId: string
  textureBounds: RigRect
  zIndex: number
  opacity: number
  slot?: string
  variant?: string
  contours: RigPoint[][]
  mesh?: RigLayerMeshSource
  boneHandles: RigBoneHandle[]
}

export interface PreparedAnime25DRigImport {
  atlas: Blob
  analysisReference: Blob
  source: MeropeRigImportSource
  partCount: number
  /** Diagnosis against the source illustration; absent without one. */
  reconciliation: Anime25DPsdReconciliation | null
  /** Holes a blink or a spoken vowel would open in the face. */
  motionExposure: MotionExposureFinding[]
  /** The document and where the playback frame cuts it, for keys measured on the document. */
  documentFrame: { width: number; height: number; x: number; y: number }
}

export interface BehaviorRealizerReport {
  behaviorId: string
  result: 'accepted' | 'rejected'
  atMs: number
  reason?: 'unsupported-form' | 'invalid-timing' | 'superseded'
}

export interface BeatFrame {
  beatPhase: number
  beatCount: number
  bpm: number
  confidence: number
  beatCrossed: boolean
  onset: boolean
}

export interface MusicPhrase {
  start: number
  end: number
  confidence: number
}

export interface TextProsodyInput {
  utteranceId: string
  text: string
  locale?: string
  startedAtMs: number
  /** Do not invent a movable midpoint while more text can still arrive. */
  streaming?: boolean
}

/** Where an utterance's audio is voiced, and where it pauses within (seconds). */
export interface AudioVoicing {
  voiceStart: number
  voiceEnd: number
  pauses: readonly (readonly [number, number])[]
}

export interface SpeechEventBase {
  messageId: string
  source: MeropeSpeechSource
  utteranceId: string
  locale?: string
  generation?: number
}

export interface SpeechAccentAnchor {
  textOffset?: number
  gesture?: SpeechGesture | 'none'
  offsetMs: number
  intensity: number
}

export interface Anime25DPsdDefectRegion {
  kind: Anime25DPsdDefectKind
  /** Pixels in PSD document space. */
  area: number
  box: RigRect
  /** The covered layer for `buried`, else the visible layer, if any. */
  role: string | null
}

export interface RawFaceLife {
  periodMs: number
  beats: Array<{ atMs: number; pose?: { targets: Record<string, string> }; move?: Record<string, unknown> }>
}

export type Surface = 'head' | 'front-hair' | 'back-hair'

export interface Point {
  x: number
  y: number
}

export type PoseCorrectionProblem = 'needsTurn' | 'needsSecond'

export type MotionChannel = 'expression' | 'mouth' | 'gaze' | 'headBody'

export type FigureErrorCode = 'anime25dPlaybackFailed' | 'anime25dWebglFailed' | 'psdNeedAsset' | 'psdTooLarge' | 'anime25dMissingFace' | 'anime25dMissingLayer' | 'anime25dContractMissing' | 'anime25dPartCount' | 'anime25dBoneLimit' | 'anime25dLayerTooWide' | 'anime25dAtlasOverflow' | 'rigAtlasFailed' | 'canvasUnsupported' | 'psdPreviewFailed' | 'psdLayerCountInvalid' | 'psdSpecInvalid'

export type FigureErrorParams = Record<string, string | number>

/** Reasons the figure drops a motion the page sent it. */
export type FigureTraceDrop = 'stale_generation' | 'foreign_speech_frame'

/** One drawn frame, for the page's frame-time trace. */
export interface FigureFrameSample {
  dropped: boolean
  cpuMs?: number
}

export type BehaviorKind = 'oneShot' | 'state' | 'rhythmic' | 'tracking' | 'secondary'

export interface BehaviorQuality {
  extent: number
  tempo: number
  power: number
  fluidity: number
  directness: number
  rebound: number
  asymmetry: number
  density: number
}

export type TouchGesture = 'contact' | 'hold' | 'stroke' | 'tap'

export type TouchSummary = Pick<TouchObservation, 'region' | 'gesture' | 'durationMs' | 'repeatCount'>

export interface MotionSnapshot {
  generation: number
  owners: Record<MotionChannel, MotionSourceId>
  leases: readonly MotionLease[]
}

export interface SpeechIntent {
  active: boolean
  autoSpeech: boolean
  energy: number | null
  articulation: SpeechArticulation | null
  prosody: SpeechProsodyPlan | null
  behaviorPlan: BehaviorPlan | null
  behaviors: readonly BehaviorSnapshot[]
  queuedText: readonly SpeechTextChunk[]
}

export interface PerformanceIntent {
  directive: PerformanceDirective | null
  startedAtMs: number
  motionIntentId?: string | null
  generation?: number
  behaviorPlan?: BehaviorPlan | null
  behaviors?: readonly BehaviorSnapshot[]
}

export interface SingingFrame {
  apply: SingingApply
  signal: MusicMotionSignal | null
  articulation: SpeechArticulation
  trackId: string | null
  behaviorPlan: BehaviorPlan | null
  behaviors: readonly BehaviorSnapshot[]
}

export interface MoodIntent {
  mood: number
  arousal: number
  activity: MeropeActivity
}

export interface BehaviorSnapshot {
  id: string
  function: BehaviorFunction
  kind: BehaviorKind
  source: MotionSourceId
  resources: readonly BehaviorResource[]
  channels: readonly MotionChannel[]
  form: BehaviorForm
  phase: BehaviorPhase
  startedAtMs: number
  readyAtMs: number
  strokeStartAtMs: number
  strokePeakAtMs: number
  strokeEndAtMs: number
  relaxAtMs: number | null
  endsAtMs: number | null
  remainingMs: number | null
  anticipatedAtMs?: number
  anticipationConfidence?: number
}

export type BehaviorFunction = RigBehaviorFunction

export type BehaviorResource = | 'face.mouth'
  | 'face.expression'
  | 'face.gaze'
  | 'body.head'
  | 'body.torso'
  | 'body.arm.left'
  | 'body.arm.right'
  | 'body.hand.left'
  | 'body.hand.right'
  | 'secondary.hair'
  | 'secondary.clothing'
  | 'secondary.bust'

export interface BehaviorTiming {
  start: string
  ready: string
  strokeStart: string
  strokePeak: string
  strokeEnd: string
  relax: string | null
  end: string | null
}

export interface BehaviorForm {
  family: string
  id: string
  parameters?: Readonly<Record<string, string | number | boolean>>
}

export interface SpeechLifecycleScheduler {
  now: () => number
  setTimeout: (callback: () => void, delayMs: number) => unknown
  clearTimeout: (timer: unknown) => void
}

export interface SingingTimelineInput {
  verbatim?: readonly WordLyricLine[]
  lines?: readonly LyricLine[]
  locale?: string
  songDuration?: number
}

export interface SingingCue {
  start: number
  end: number
  viseme: SpeechViseme
  emphasis: boolean
}

export declare function compileSingingTimeline(input: SingingTimelineInput): Promise<SingingCue[]>

/**
 * What a fading part uncovers. A decomposed portrait composites back to the
 * original at rest, yet a blink or a spoken vowel fades a part off the skin
 * meant to lie below it; wherever that skin was never painted, the rig opens a
 * hole. Holes are transparent pixels the skin's own paint closes in on, so the
 * silhouette is never counted, and hair behind the face cannot hide a face
 * left unpainted. (A glance needs no check: irises are clipped to the white.)
 */
export type MotionExposureCheck = 'face-under-eyes' | 'face-under-mouth'

export type EyeSide = 'left' | 'right'

export interface RigDiagnostic {
  code: string
  severity: RigDiagnosticSeverity
  message: string
  clipId?: string
  boneId?: string
}

export type RigRect = RigPoint & RigSize

/** Legacy contour-only sources remain unchanged. */
export interface RigLayerMeshSource {
  vertices: RigPoint[]
  indices: number[]
}

export interface RigBoneHandle {
  boneId: string
  start: RigPoint
  end: RigPoint
  falloff: number
}

export type SpeechGesture = 'hesitate' | 'tease' | 'check-in' | 'laugh' | 'question' | 'contrast'

export interface MotionLease {
  leaseId: string
  source: MotionSourceId
  channels: readonly MotionChannel[]
  generation: number
  expiresAtMs: number | null
}

export interface SpeechTextChunk {
  seq: number
  text: string
  locale?: string
}

export interface SingingApply {
  release: boolean
  /** Keep singing=true / signal so the player groove continues. */
  writeGroove: boolean
  writeMouth: boolean
  /** Rest the mouth without releasing groove. */
  restMouth: boolean
}

export type BehaviorPhase = RigBehaviorPhase

export type RigDiagnosticSeverity = 'error' | 'warning' | 'info'

/** The look with no preset: her own face. */
export declare const PLAIN_FACE: 'plain'

/** The moves a score is written in, and the controls each one moves. */
export declare const SCORE_MOVES: Record<ScoreMoveKind, { controls: BodyControl[]; description: string; }>
