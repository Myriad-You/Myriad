import { frameRms } from './audioWav'

export interface CapturedUtterance {
  pcm: Float32Array[]
  sampleRate: number
}

/**
 * Energy endpointing, not speech recognition. All gates use audio duration,
 * independent of AudioWorklet message size. A model VAD can improve noise
 * rejection later; these bounds and pre-roll are still required underneath it.
 */
export class UtteranceCapture {
  private preRoll: Float32Array[] = []
  private preSamples = 0
  private clip: Float32Array[] = []
  private clipSamples = 0
  private openSamples = 0
  private quietSamples = 0
  private voicedSamples = 0
  private active = false
  private threshold = 0.035

  constructor(readonly sampleRate: number) {}

  push(
    frame: Float32Array,
    playback: boolean,
  ): {
    started: boolean
    ended: boolean
    utterance?: CapturedUtterance
  } {
    if (frame.length === 0) return { started: false, ended: false }
    const rms = frameRms(frame)
    if (!this.active) {
      this.preRoll.push(frame)
      this.preSamples += frame.length
      const preLimit = Math.round(this.sampleRate * 0.32)
      while (this.preSamples > preLimit && this.preRoll.length > 0) {
        const first = this.preRoll[0]!
        const excess = this.preSamples - preLimit
        if (first.length <= excess) {
          this.preRoll.shift()
          this.preSamples -= first.length
        } else {
          this.preRoll[0] = first.slice(excess)
          this.preSamples -= excess
        }
      }
      this.threshold = playback ? 0.14 : 0.035
      this.openSamples =
        rms >= this.threshold ? this.openSamples + frame.length : 0
      // A single loud click must not immediately stop the reply.
      if (this.openSamples < this.sampleRate * (playback ? 0.16 : 0.12)) {
        return { started: false, ended: false }
      }
      this.active = true
      this.clip = this.preRoll
      this.clipSamples = this.preSamples
      this.preRoll = []
      this.preSamples = 0
      this.voicedSamples = this.openSamples
      this.quietSamples = 0
      return { started: true, ended: false }
    }

    this.clip.push(frame)
    this.clipSamples += frame.length
    // The louder threshold protects barge-in only while playback is present.
    // Once interrupted, quiet continuation belongs to this same utterance.
    if (rms >= (playback ? this.threshold : 0.035) * 0.45) {
      this.quietSamples = 0
      this.voicedSamples += frame.length
    } else {
      this.quietSamples += frame.length
    }
    if (
      this.quietSamples < this.sampleRate * 0.55 &&
      this.clipSamples < this.sampleRate * 20
    ) {
      return { started: false, ended: false }
    }
    return { started: false, ended: true, utterance: this.finish() }
  }

  finish(): CapturedUtterance | undefined {
    const utterance =
      this.active && this.voicedSamples >= this.sampleRate * 0.16
        ? { pcm: this.clip, sampleRate: this.sampleRate }
        : undefined
    this.reset()
    return utterance
  }

  reset(): void {
    this.preRoll = []
    this.preSamples = 0
    this.clip = []
    this.clipSamples = 0
    this.openSamples = 0
    this.quietSamples = 0
    this.voicedSamples = 0
    this.active = false
  }
}
