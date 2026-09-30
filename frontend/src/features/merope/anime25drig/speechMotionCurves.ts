import type { SpeechViseme } from '../rig/articulation'
import type { AutoSpeechPose } from './speechMotion'

export function speechPhraseAmplitudeScale(progress: number): number {
  const bounded = unitInterval(progress)
  const onset = mix(0.88, 1, smootherstep(bounded / 0.16))
  const ending = mix(1, 0.82, smootherstep((bounded - 0.72) / 0.28))
  return onset * ending
}

export function speechPhraseIntervalScale(progress: number): number {
  const bounded = unitInterval(progress)
  return mix(1, 1.18, smootherstep((bounded - 0.72) / 0.28))
}

export function visemeValue(
  viseme: SpeechViseme,
  channel: 'open' | 'wide' | 'round' | 'narrow' | 'seal',
): number {
  if (channel === 'seal') return viseme === 'closed' ? 1 : 0
  if (channel === 'open') {
    if (viseme === 'open') return 0.78
    if (viseme === 'wide') return 0.54
    if (viseme === 'round') return 0.64
    if (viseme === 'narrow') return 0.34
    return 0
  }
  return viseme === channel ? 1 : 0
}

export function closestViseme(pose: Readonly<AutoSpeechPose>): SpeechViseme {
  const candidates: SpeechViseme[] = [
    'rest',
    'closed',
    'open',
    'wide',
    'round',
    'narrow',
  ]
  let closest: SpeechViseme = 'rest'
  let closestDistance = Number.POSITIVE_INFINITY
  for (const candidate of candidates) {
    const distance =
      Math.abs(pose.mouthOpen - visemeValue(candidate, 'open')) +
      Math.abs(pose.mouthWide - visemeValue(candidate, 'wide')) +
      Math.abs(pose.mouthRound - visemeValue(candidate, 'round')) +
      Math.abs(pose.mouthNarrow - visemeValue(candidate, 'narrow')) +
      Math.abs(pose.mouthSeal - visemeValue(candidate, 'seal'))
    if (distance < closestDistance) {
      closest = candidate
      closestDistance = distance
    }
  }
  return closest
}

export function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

export function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

export function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function unitInterval(value: number): number {
  return Number.isFinite(value) ? clamp(value, 0, 1) : 0
}

export function attackReleasePulse(
  elapsed: number,
  delay: number,
  attack: number,
  release: number,
): number {
  const shifted = elapsed - delay
  if (!Number.isFinite(shifted) || shifted < 0) return 0
  if (shifted < attack) return smootherstep(shifted / attack)
  if (shifted < attack + release) {
    return 1 - smootherstep((shifted - attack) / release)
  }
  return 0
}
