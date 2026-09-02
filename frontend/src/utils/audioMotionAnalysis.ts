/** Audio evidence, not a claim that the mix contains a singer. */
export interface MotionAudioFeatures {
  energy: number
  bass: number
  pulse: number
  presence: number
}

/**
 * Read physical frequency ranges instead of the visualizer's eight equal-width
 * bars (whose first bar includes most of the voice). No source separation or
 * pitch detection is implied by `presence`. The caller owns reusable buffers.
 */
export function analyzeMotionAudio(
  waveform: Float32Array,
  decibels: Float32Array,
  sampleRate: number,
  output: MotionAudioFeatures,
  playbackVolume = 1,
): MotionAudioFeatures {
  // MediaElementAudioSource includes the element's volume. Choreography follows
  // the recording's dynamics, not the listener's speaker-volume preference.
  const gain =
    Number.isFinite(playbackVolume) && playbackVolume > 0
      ? 1 / playbackVolume
      : 1
  let power = 0
  for (const value of waveform)
    power += Number.isFinite(value) ? value * value : 0
  output.energy = unit(
    Math.sqrt(power / Math.max(1, waveform.length)) * 3 * gain,
  )
  const binHz = sampleRate / (decibels.length * 2)
  const bass = bandAmplitude(decibels, binHz, 40, 220)
  const low = bandAmplitude(decibels, binHz, 220, 700)
  output.bass = unit(bass * 4 * gain)
  output.pulse = unit((bass * 0.8 + low * 0.2) * 4 * gain)
  output.presence = unit(bandAmplitude(decibels, binHz, 700, 4_000) * 4 * gain)
  return output
}

function bandAmplitude(
  bins: Float32Array,
  binHz: number,
  low: number,
  high: number,
): number {
  if (!(binHz > 0)) return 0
  let power = 0
  const start = Math.max(1, Math.ceil(low / binHz))
  const end = Math.min(bins.length, Math.ceil(high / binHz))
  for (let i = start; i < end; i++) {
    const db = bins[i]
    if (Number.isFinite(db)) power += 10 ** (db / 10)
  }
  return Math.sqrt(power)
}

function unit(value: number): number {
  return Math.max(0, Math.min(1, value))
}
