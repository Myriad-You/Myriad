import type { SpeechProsodyPlan } from '../speech/prosody'
import type { BehaviorPlan, ScheduledBehavior, TimePeg } from './behavior'

/** Future co-speech accents share the same lifecycle as other behaviors. */
export function compileSpeechBehaviorPlan(
  plan: SpeechProsodyPlan,
): BehaviorPlan {
  const planId = `speech:${plan.utteranceId}`
  const pegs: TimePeg[] = []
  const behaviors: ScheduledBehavior[] = []
  const presence = `${planId}:presence`
  const presenceEnd = plan.startedAtMs + Math.max(1, plan.durationMs)
  pegs.push(
    peg(`${presence}:start`, plan.startedAtMs),
    peg(`${presence}:ready`, plan.startedAtMs + 35),
    peg(`${presence}:stroke-start`, plan.startedAtMs + 55),
    peg(`${presence}:stroke-peak`, plan.startedAtMs + 80),
    peg(`${presence}:stroke-end`, plan.startedAtMs + 110),
    peg(
      `${presence}:relax`,
      Math.max(plan.startedAtMs + 110, presenceEnd - 120),
    ),
    peg(`${presence}:end`, presenceEnd),
  )
  behaviors.push({
    id: presence,
    function: 'prepareSpeech',
    kind: 'state',
    source: 'coSpeech',
    resources: ['face.expression', 'body.head', 'body.torso'],
    channels: ['expression', 'headBody'],
    timing: {
      start: `${presence}:start`,
      ready: `${presence}:ready`,
      strokeStart: `${presence}:stroke-start`,
      strokePeak: `${presence}:stroke-peak`,
      strokeEnd: `${presence}:stroke-end`,
      relax: `${presence}:relax`,
      end: `${presence}:end`,
    },
    form: { family: 'co-speech', id: 'presence' },
    intensity: 0.58,
    quality: {
      extent: 0.78,
      tempo: 1,
      power: 0.58,
      fluidity: 0.9,
      directness: 0.62,
      rebound: 0.24,
      asymmetry: 0.24,
      density: 0.72,
    },
    confidence: 1,
  })
  plan.accents.forEach((accent, index) => {
    const prefix = `${planId}:accent-${index}`
    const strokePeakAt = plan.startedAtMs + accent.offsetMs
    pegs.push(
      peg(`${prefix}:start`, strokePeakAt - 140),
      peg(`${prefix}:ready`, strokePeakAt - 82),
      peg(`${prefix}:stroke-start`, strokePeakAt - 44),
      peg(`${prefix}:stroke-peak`, strokePeakAt),
      peg(`${prefix}:stroke-end`, strokePeakAt + 58),
      peg(`${prefix}:relax`, strokePeakAt + 142),
      peg(`${prefix}:end`, strokePeakAt + 270),
    )
    behaviors.push({
      id: prefix,
      function: 'emphasize',
      kind: 'oneShot',
      source: 'coSpeech',
      resources: ['face.expression', 'body.head', 'body.torso'],
      channels: ['expression', 'headBody'],
      timing: {
        start: `${prefix}:start`,
        ready: `${prefix}:ready`,
        strokeStart: `${prefix}:stroke-start`,
        strokePeak: `${prefix}:stroke-peak`,
        strokeEnd: `${prefix}:stroke-end`,
        relax: `${prefix}:relax`,
        end: `${prefix}:end`,
      },
      form: { family: 'co-speech', id: 'accent' },
      intensity: accent.intensity,
      quality: {
        extent: 0.92 + accent.intensity * 0.22,
        tempo: 1,
        power: 0.82 + accent.intensity * 0.24,
        fluidity: 0.78,
        directness: 0.82,
        rebound: 0.42,
        asymmetry: 0.28,
        density: 0.88,
      },
      confidence: 0.92,
    })
  })
  return { id: planId, originMs: plan.startedAtMs, pegs, behaviors }
}

function peg(id: string, atMs: number): TimePeg {
  return { id, atMs: Math.max(0, atMs), revision: 0 }
}
