import type { RigMotionPort } from '../rig/motionPort'
import type { RigBearing } from './bearing'
import type { MotionFrame } from './intents'
import { applySingingWrite } from './applySnapshot'
import { policyFromOwners } from './policy'

export interface MotionApplyState {
  speechTextSeq: number
  behaviorRevision: number | null
  behaviorPlanId: string | null
  bearing: RigBearing | null
  speechOwnedMouth: boolean
  speechProsodyKey: string | null
}

export function createMotionApplyState(): MotionApplyState {
  return {
    speechTextSeq: 0,
    behaviorRevision: null,
    behaviorPlanId: null,
    bearing: null,
    speechOwnedMouth: false,
    speechProsodyKey: null,
  }
}

/** The only production writer. */
export function applyMotionFrame(
  rig: Pick<
    RigMotionPort,
    | 'setMotionPolicy'
    | 'setBearing'
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
  >,
  frame: MotionFrame,
  state: MotionApplyState,
  onRealizer?: (feedback: MotionRealizerFeedback) => void,
): MotionApplyState {
  // Writes come in three kinds and only one of them is motion.
  applyStanding(rig, frame, state)
  applySignals(rig, frame, state)
  applyBehaviorPlan(rig, frame, state, onRealizer)
  return state
}

function applyStanding(
  rig: Pick<RigMotionPort, 'setMotionPolicy' | 'setBearing' | 'setMood'>,
  frame: MotionFrame,
  state: MotionApplyState,
): void {
  rig.setMotionPolicy(policyFromOwners(frame.snapshot.owners))
  applyBearing(rig, frame, state)
  if (frame.mood) rig.setMood(frame.mood.mood, frame.mood.activity)
}

function applySignals(
  rig: Pick<
    RigMotionPort,
    | 'setSpeechActive'
    | 'setAutoSpeech'
    | 'setSpeechEnergy'
    | 'setSpeechArticulation'
    | 'setSpeechProsody'
    | 'enqueueSpeechText'
    | 'setSinging'
    | 'setSingingTrack'
    | 'setMusicSignal'
  >,
  frame: MotionFrame,
  state: MotionApplyState,
): void {
  applySpeech(rig, frame, state)
  applyMusic(rig, frame)
}

export interface MotionRealizerFeedback {
  planId: string
  behaviorId: string
  result: 'accepted' | 'rejected'
  atMs: number
  reason?: import('./behavior').BehaviorRealizerReport['reason']
}

function applyBearing(
  rig: Pick<RigMotionPort, 'setBearing'>,
  frame: MotionFrame,
  state: MotionApplyState,
): void {
  if (frame.bearing === state.bearing) return
  rig.setBearing(frame.bearing)
  state.bearing = frame.bearing
}

function applySpeech(
  rig: Pick<
    RigMotionPort,
    | 'setSpeechActive'
    | 'setAutoSpeech'
    | 'setSpeechEnergy'
    | 'setSpeechArticulation'
    | 'setSpeechProsody'
    | 'enqueueSpeechText'
  >,
  frame: MotionFrame,
  state: MotionApplyState,
): void {
  const speechOwns = frame.snapshot.owners.mouth === 'speech'
  const speech = frame.speech
  if (speechOwns && speech) {
    rig.setSpeechActive(speech.active)
    rig.setAutoSpeech(speech.autoSpeech)
    if (speech.energy != null) rig.setSpeechEnergy(speech.energy)
    if (speech.articulation) rig.setSpeechArticulation(speech.articulation)
    const prosodyKey = speechProsodyKey(speech.prosody)
    if (prosodyKey !== state.speechProsodyKey) {
      rig.setSpeechProsody(speech.prosody)
      state.speechProsodyKey = prosodyKey
    }
    for (const chunk of speech.queuedText) {
      if (chunk.seq <= state.speechTextSeq) continue
      rig.enqueueSpeechText(chunk.text, chunk.locale)
      state.speechTextSeq = chunk.seq
    }
    state.speechOwnedMouth = true
    return
  }
  if (state.speechOwnedMouth && !speechOwns) {
    rig.setAutoSpeech(false)
    if (frame.snapshot.owners.mouth !== 'music') rig.setSpeechActive(false)
    rig.setSpeechProsody(null)
    state.speechProsodyKey = null
    state.speechOwnedMouth = false
  }
}

function speechProsodyKey(
  prosody: import('../speech/prosody').SpeechProsodyPlan | null,
): string | null {
  if (!prosody) return null
  const accents = prosody.accents
    .map(
      (accent) =>
        `${accent.offsetMs}:${accent.intensity}:${accent.gesture ?? ''}`,
    )
    .join(',')
  return `${prosody.utteranceId}|${prosody.startedAtMs}|${prosody.durationMs}|${accents}`
}

function applyBehaviorPlan(
  rig: Pick<RigMotionPort, 'playBehaviorPlan' | 'stopBehaviorPlan'>,
  frame: MotionFrame,
  state: MotionApplyState,
  onRealizer?: (feedback: MotionRealizerFeedback) => void,
): void {
  const plan = frame.behaviorPlan
  if (plan && plan.behaviors.length > 0) {
    if (
      state.behaviorRevision !== frame.behaviorRevision ||
      state.behaviorPlanId !== plan.id
    ) {
      if (state.behaviorPlanId && state.behaviorPlanId !== plan.id) {
        rig.stopBehaviorPlan(state.behaviorPlanId)
      }
      const reports = rig.playBehaviorPlan(plan)
      state.behaviorRevision = frame.behaviorRevision
      state.behaviorPlanId = plan.id
      for (const report of reports) {
        onRealizer?.({
          planId: plan.id,
          behaviorId: report.behaviorId,
          result: report.result,
          atMs: report.atMs,
          ...(report.reason ? { reason: report.reason } : {}),
        })
      }
    }
    return
  }
  if (state.behaviorPlanId) {
    rig.stopBehaviorPlan(state.behaviorPlanId)
    state.behaviorRevision = null
    state.behaviorPlanId = null
  }
}

function applyMusic(
  rig: Pick<
    RigMotionPort,
    | 'setSinging'
    | 'setSingingTrack'
    | 'setMusicSignal'
    | 'setSpeechArticulation'
    | 'setSpeechActive'
  >,
  frame: MotionFrame,
): void {
  if (!frame.music) return
  applySingingWrite(rig, frame.music.apply, {
    trackId: frame.music.trackId,
    signal: frame.music.signal,
    articulation: frame.music.articulation,
  })
}
