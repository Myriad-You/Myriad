import type { Anime25DCharacterHandle } from '../../../src/features/merope/anime25drig/Anime25DCharacter'
import type { MotionLeaseHandle } from '../../../src/features/merope/motion/coordinator'
import type { MeropeRigManifest } from '../../../src/features/merope/rig/types'
import { createRef } from 'react'
import { createRoot } from 'react-dom/client'
import Anime25DCharacter from '../../../src/features/merope/anime25drig/Anime25DCharacter'
import { Anime25DPlayer } from '../../../src/features/merope/anime25drig/player'
import { TouchGestureTracker } from '../../../src/features/merope/interaction/touchGesture'
import { applyMotionFrame, createMotionApplyState } from '../../../src/features/merope/motion/applyFrame'
import { RigMotionCoordinator } from '../../../src/features/merope/motion/coordinator'
import { HumanPerformanceRuntime } from '../../../src/features/merope/motion/humanPerformanceRuntime'
import { MusicMotionSource } from '../../../src/features/merope/motion/musicSource'
import { MotionRuntime } from '../../../src/features/merope/motion/runtime'
import { TouchMotionSource } from '../../../src/features/merope/motion/touchSource'
import { dispatchMeropeSpeech, dispatchMeropeSpeechUtterance } from '../../../src/features/merope/speech/speechEvents'

/** Exercise the real component port and renderer; only input/time are synthetic. */
export async function replayCharacterScene(manifest: MeropeRigManifest, scene: 'touch-speech' | 'touch-thinking' | 'default') {
  const withSpeech = scene === 'touch-speech'
  const root = createRoot(document.getElementById('root')!)
  const ref = createRef<Anime25DCharacterHandle>()
  const originalTick = Anime25DPlayer.prototype.tick
  const originalRandom = Math.random
  const originalNow = Object.getOwnPropertyDescriptor(performance, 'now')
  const captured: { player?: Anime25DPlayer } = {}
  let seed = 1749
  Math.random = () => ((seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296)
  Anime25DPlayer.prototype.tick = function (dt) {
    captured.player = this
    originalTick.call(this, dt)
  }
  const coordinator = new RigMotionCoordinator()
  const source = new TouchMotionSource(coordinator, () => {})
  try {
    await new Promise<void>((resolve, reject) => root.render(
      <Anime25DCharacter
        ref={ref}
        manifest={manifest}
        playback={manifest.anime25dPlayback!}
        atlasUrl="/touch-handoff-atlas.png"
        activity={scene === 'default' ? 'idle' : 'thinking'}
        mood={70}
        onPlaybackReady={resolve}
        onPlaybackError={reject}
      />,
    ))
    Anime25DPlayer.prototype.tick = originalTick
    const player = captured.player
    if (!player || !ref.current) throw new Error('The mounted character did not reach its real player')
    const canvas = document.querySelector('canvas')!
    const gl = canvas.getContext('webgl2')!
    const rig = ref.current
    const initialThinking = player.getTarget().thinking
    const start = performance.now()
    let now = start
    Object.defineProperty(performance, 'now', { configurable: true, value: () => now })
    if (scene === 'default') return replayDefaultPerformance(rig, player, canvas, start, value => { now = value })
    const tracker = new TouchGestureTracker()
    const scheduler = new HumanPerformanceRuntime()
    const state = createMotionApplyState()
    let speechLease: MotionLeaseHandle | null = null
    let maxHeadStep = 0; let maxEyeStep = 0; let resetCount = 0
    let mouthDuringTouch = 0; let acceptedFrames = 0; let liftedMaxEye = 0
    let thoughtDuringTouch = false; let thoughtDuringSpeech = false
    let speechMouthOwned = true
    let previous = player.getCurrent()
    const screenshots: { name: string; image: string }[] = []
    for (let frame = 0; frame < 480; frame++) {
      now = start + frame * 1000 / 60
      const point = { pointerId: 1, x: 0.16 * Math.sin(frame / 39), y: 0, atMs: now, region: 'hair' as const }
      const before = JSON.stringify(player.getCurrent())
      if (frame === 60 || frame === 156) {
        source.update('panel', tracker.begin(point)!, now)
      } else if (frame === 144) {
        source.update('panel', tracker.end(point)!, now)
      } else if (frame === 240) {
        source.update('panel', tracker.cancel(now)!, now)
      } else if (frame > 60 && frame < 240) {
        const observation = tracker.update(point)
        if (observation) source.update('panel', observation, now)
      }
      if (withSpeech && frame === 180) speechLease = coordinator.claim('speech', ['mouth'], { nowMs: now })
      if (frame === 360) coordinator.release(speechLease)
      const speaking = withSpeech && frame >= 180 && frame < 360
      const scheduled = scheduler.frame([source.current()], now)
      applyMotionFrame(rig, {
        snapshot: coordinator.snapshot(now), bearing: null, performance: null, music: null,
        mood: { mood: 70, arousal: 48, activity: !withSpeech || frame < 180 ? 'thinking' : 'idle' },
        speech: speaking ? { active: true, autoSpeech: true, energy: null, articulation: null,
          prosody: null, behaviorPlan: null, behaviors: [],
          queuedText: [{ seq: 1, text: '嗯，我想好了。这样摸摸头也不错，我们慢慢说。', locale: 'zh-CN' }] } : null,
        behaviorPlan: scheduled.plan, behaviorRevision: scheduled.revision, behaviors: scheduled.behaviors,
      }, state, feedback => scheduler.reportRealizer(feedback.planId, feedback.behaviorId, feedback.result, feedback.atMs, feedback.reason))
      if (before !== JSON.stringify(player.getCurrent())) resetCount++
      // This loop is synchronous: browser RAF cannot interleave an extra tick.
      player.tick(1 / 60)
      const current = player.getCurrent()
      for (const key of ['angleX', 'angleY', 'angleZ', 'body'] as const) {
        maxHeadStep = Math.max(maxHeadStep, Math.abs(current[key] - previous[key]))
      }
      if (frame >= 135 && frame < 170) {
        maxEyeStep = Math.max(maxEyeStep, Math.abs(current.eyeOpenL - previous.eyeOpenL))
        liftedMaxEye = Math.max(liftedMaxEye, current.eyeOpenL, current.eyeOpenR)
      }
      const presented = player.getPresentedTouch()
      if (presented?.reaction === 'accept') acceptedFrames++
      source.notePresented('panel', presented, now)
      if (speaking && frame >= 195 && frame < 240) {
        mouthDuringTouch = Math.max(mouthDuringTouch, current.mouthOpen)
        speechMouthOwned &&= player.getMotionPolicy().mouth === 'speech'
        thoughtDuringSpeech ||= player.getTarget().thinking
      }
      if (frame === 120) thoughtDuringTouch = player.getTarget().thinking
      if ([45, 135, 165, 215, 330, 465].includes(frame)) {
        screenshots.push({ name: `touch-handoff-${frame}`, image: canvas.toDataURL('image/png').split(',')[1] })
      }
      previous = current
    }
    return { initialThinking, thoughtDuringTouch, thoughtDuringSpeech: withSpeech ? thoughtDuringSpeech : null, resetCount, maxHeadStep,
      maxEyeStep, liftedMaxEye, mouthDuringTouch: withSpeech ? mouthDuringTouch : null, acceptedFrames,
      speechMouthOwned: withSpeech ? speechMouthOwned : null,
      finalTouch: player.getPresentedTouch(), finalOwners: player.getMotionPolicy(),
      finalThinking: player.getTarget().thinking, finalTalk: player.getTarget().talk,
      finalEyeOpen: player.getCurrent().eyeOpenL,
      error: gl.getError(), screenshots }
  } finally {
    source.release()
    root.unmount()
    Anime25DPlayer.prototype.tick = originalTick
    Math.random = originalRandom
    if (originalNow) Object.defineProperty(performance, 'now', originalNow)
    else Reflect.deleteProperty(performance, 'now')
  }
}

function replayDefaultPerformance(
  rig: Anime25DCharacterHandle, player: Anime25DPlayer, canvas: HTMLCanvasElement,
  start: number, setNow: (value: number) => void,
) {
  let now = start
  // Advance the real lifecycle callbacks with the same clock as the renderer.
  // Previously a forced cancel concealed whether a text utterance could finish.
  const originalTimeout = window.setTimeout
  const originalClearTimeout = window.clearTimeout
  const timers = new Map<number, { atMs: number; run: () => void }>()
  let timerId = 0
  window.setTimeout = ((callback: TimerHandler, delay = 0, ...args: unknown[]) => {
    if (typeof callback !== 'function') throw new Error('The replay only accepts function timers')
    const id = --timerId
    timers.set(id, { atMs: now + delay, run: () => callback(...args) })
    return id
  }) as typeof window.setTimeout
  window.clearTimeout = ((id: number | undefined) => {
    if (id !== undefined && id < 0) timers.delete(id)
    else originalClearTimeout(id)
  }) as typeof window.clearTimeout
  const coordinator = new RigMotionCoordinator()
  const audio = { paused: true, ended: false, currentTime: 0 }
  const music = new MusicMotionSource(coordinator, {
    now: () => now, raf: () => 1, caf: () => {},
  }, {
    getCurrentAudio: () => audio,
    getMotionAudioFeatures: () => ({ energy: 0.65, bass: 0.55,
      pulse: Math.exp(-((audio.currentTime * 100 / 60) % 1) * 18) * 0.9, presence: 0.5 }),
    connectAudioToAnalyser: () => true,
  }, { isPageVisible: () => true, onVisibility: () => () => {} })
  const runtime = new MotionRuntime(coordinator, music)
  const release = runtime.retain()
  const tracker = new TouchGestureTracker()
  const state = createMotionApplyState()
  const speech = { source: 'preview' as const, messageId: 'scene-line', utteranceId: 'scene-utterance', locale: 'zh-CN' }
  const phases: Record<string, { minYaw: number; maxYaw: number; minBody: number; maxBody: number; mouth: number; frames: number }> = {}
  const screenshots: { name: string; image: string }[] = []
  let previous = player.getCurrent()
  let maxStep = 0; let resets = 0; let musicFrames = 0; let touchFrames = 0
  let thinkingFrames = 0; let speechFrames = 0; let lateMusicFrames = 0
  let naturalSpeechEnd: number | null = null
  let speechWasActive = false
  music.setTrack({ trackId: 'default-scene', duration: 5 })
  runtime.mood.set(70, 'idle', 48)
  try {
    // Full source/runtime/component replay; motion is sampled at 30 Hz here.
    for (let frame = 0; frame < 26 * 30; frame++) {
      const seconds = frame / 30
      now = start + seconds * 1000
      setNow(now)
      const before = JSON.stringify(player.getCurrent())
      for (let drained = 0; ; drained++) {
        const due = [...timers.entries()].filter(([, timer]) => timer.atMs <= now)
          .sort((a, b) => a[1].atMs - b[1].atMs)[0]
        if (!due) break
        if (drained >= 1000) throw new Error('Runaway lifecycle timer in replay')
        timers.delete(due[0])
        due[1].run()
      }
      if (frame === 90) { audio.paused = false; music.setPlayback(true, false) }
      if (frame === 240) { audio.paused = true; audio.ended = true }
      audio.currentTime = Math.min(5, Math.max(0, seconds - 3))
      if (frame === 330) runtime.mood.set(70, 'thinking', 48)
      if (frame === 420) {
        runtime.mood.set(70, 'idle', 48)
        dispatchMeropeSpeechUtterance({ ...speech, text: '这首歌真好听。我们慢慢聊吧。' })
      }
      const point = { pointerId: 1, atMs: now, x: 0.16 * Math.sin(seconds * 1.5), y: 0, region: 'hair' as const }
      if (frame === 450) runtime.touch.update('panel', tracker.begin(point)!, now)
      else if (frame === 510) runtime.touch.update('panel', tracker.cancel(now)!, now)
      else if (frame > 450 && frame < 510) runtime.touch.update('panel', tracker.update(point)!, now)
      music.sampleNow(now)
      const motion = runtime.frame(now)
      if (motion.snapshot.owners.mouth === 'speech') speechWasActive = true
      else if (speechWasActive && naturalSpeechEnd === null) naturalSpeechEnd = seconds
      applyMotionFrame(rig, motion, state, feedback => runtime.reportBehaviorRealizer(
        feedback.planId, feedback.behaviorId, feedback.result, feedback.atMs, feedback.reason,
      ))
      if (before !== JSON.stringify(player.getCurrent())) resets++
      player.tick(1 / 30)
      const current = player.getCurrent()
      for (const key of ['angleX', 'angleY', 'angleZ', 'body'] as const) maxStep = Math.max(maxStep, Math.abs(current[key] - previous[key]))
      const phase = seconds < 3 ? 'idle' : seconds < 8 ? 'music' : seconds < 11 ? 'afterMusic'
        : seconds < 14 ? 'thinking' : seconds < 20 ? 'speechTouch' : 'finalIdle'
      const record = phases[phase] ??= { minYaw: Infinity, maxYaw: -Infinity, minBody: Infinity, maxBody: -Infinity, mouth: 0, frames: 0 }
      record.minYaw = Math.min(record.minYaw, current.angleX); record.maxYaw = Math.max(record.maxYaw, current.angleX)
      record.minBody = Math.min(record.minBody, current.body); record.maxBody = Math.max(record.maxBody, current.body)
      record.mouth = Math.max(record.mouth, current.mouthOpen); record.frames++
      if (phase === 'music' && motion.snapshot.owners.headBody === 'music' && motion.behaviors.some(b => b.source === 'music') && current.singing) musicFrames++
      if (seconds >= 9 && current.singing) lateMusicFrames++
      if (phase === 'thinking' && player.getTarget().thinking) thinkingFrames++
      if (phase === 'speechTouch' && motion.snapshot.owners.mouth === 'speech' && current.talk) speechFrames++
      if (player.getPresentedTouch()?.reaction === 'accept') touchFrames++
      runtime.touch.notePresented('panel', player.getPresentedTouch(), now)
      if ([75, 195, 315, 405, 495, 615, 765].includes(frame)) {
        screenshots.push({ name: `${phase}-${frame}.png`, image: canvas.toDataURL('image/png').split(',')[1] })
      }
      previous = current
    }
    return { phases, maxStep, resets, musicFrames, lateMusicFrames, thinkingFrames, speechFrames, touchFrames, naturalSpeechEnd,
      finalThinking: player.getTarget().thinking, finalTalk: player.getTarget().talk,
      finalTouch: player.getPresentedTouch(), finalOwners: player.getMotionPolicy(),
      error: canvas.getContext('webgl2')!.getError(), screenshots }
  } finally {
    dispatchMeropeSpeech({ ...speech, phase: 'cancel' })
    release()
    window.setTimeout = originalTimeout
    window.clearTimeout = originalClearTimeout
    timers.clear()
  }
}
