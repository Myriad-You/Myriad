import type { TranslationKeys } from '../../../i18n'
import type { TurnKeyformsStatus } from '../turnKeyformsApi'
import type { EnhancePlan, EnhanceStep, EnhanceStepKey } from './enhanceEstimate'
import { useRef } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { estimateEnhance } from './enhanceEstimate'
import { rigImportStatusLabel } from './rigImportCopy'

type Labels = TranslationKeys['merope']

/** Where an enhancement job stands: its steps, how long it has run and about how long is left. */
export function EnhanceProgress({ plan, status }: { plan: EnhancePlan; status: TurnKeyformsStatus }) {
  const { t, format } = useI18n()
  const labels = t.merope
  // How long the current unit has run, as far as this page has seen it.
  const unit = useRef<{ key: string; at: number }>({ key: '', at: Date.now() })
  const unitKey = `${status.phase}:${status.stage}:${status.done}`
  if (unit.current.key !== unitKey) unit.current = { key: unitKey, at: Date.now() }
  const unitSeconds = (Date.now() - unit.current.at) / 1000
  const estimate = estimateEnhance(plan, status, unitSeconds)
  const minutes = (seconds: number) =>
    seconds < 60 ? labels.rigEnhanceUnderMinute : format(labels.rigEnhanceMinutes, { minutes: String(Math.ceil(seconds / 60)) })
  return (
    <section className="merope-motion-rig__status merope-enhance-progress" aria-live="polite">
      <strong>{labels.rigEnhanceTitle}</strong>
      <div className="merope-enhance-progress__summary">
        <span>{format(labels.rigEnhanceElapsed, { time: minutes(estimate.elapsedSeconds) })}</span>
        <span>
          {estimate.remainingSeconds < 60
            ? labels.rigEnhanceAlmost
            : format(labels.rigEnhanceRemaining, { time: minutes(estimate.remainingSeconds) })}
        </span>
      </div>
      <div
        className="merope-enhance-progress__bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(estimate.fraction * 100)}
      >
        <span style={{ width: `${(estimate.fraction * 100).toFixed(1)}%` }} />
      </div>
      <ol className="merope-motion-rig__steps">
        {estimate.steps.map((step, index) => (
          <li
            key={step.key}
            className={`merope-motion-rig__step is-${step.state}${step.parallel ? ' is-parallel' : ''}`}
            aria-current={step.state === 'started' ? 'step' : undefined}
          >
            <span className="merope-motion-rig__step-marker" aria-hidden="true">
              {step.state === 'completed' ? '✓' : step.parallel ? '∥' : index + 1}
            </span>
            <span className="merope-motion-rig__step-copy">
              <b>{stepTitle(labels, step.key)}</b>
              <span>{stepDescription(labels, step.key)}</span>
              {step.key === 'body' && step.state === 'started' && status.body ? (
                <span className="merope-enhance-progress__now">
                  {`${status.body.motion === 'weight' ? labels.rigEnhanceBodyWeight : labels.rigEnhanceBodyTurn} · ${bodyNow(labels, status.body, format)}`}
                </span>
              ) : null}
              {step.key === 'body' && step.state === 'failed' ? (
                <span className="merope-enhance-progress__now">{labels.rigEnhanceBodyFailed}</span>
              ) : null}
            </span>
            <span className="merope-motion-rig__step-state">{stepState(labels, step, format)}</span>
          </li>
        ))}
      </ol>
      <p className="merope-motion-rig__hint">{labels.rigEnhanceNext}</p>
    </section>
  )
}

function stepState(labels: Labels, step: EnhanceStep, format: (template: string, values: Record<string, string>) => string): string {
  const state = rigImportStatusLabel(labels, step.state)
  if (step.state !== 'started' || step.total <= 1 || step.parallel) return state
  return format(labels.rigEnhanceStepCount, { state, done: String(step.done), total: String(step.total) })
}

function bodyNow(
  labels: Labels,
  body: NonNullable<TurnKeyformsStatus['body']>,
  format: (template: string, values: Record<string, string>) => string,
): string {
  const counts = { done: String(body.done), total: String(body.total) }
  if (body.stage === 'generating') return format(labels.rigEnhanceBodyDrawing, counts)
  if (body.stage === 'decomposing') return format(labels.motionTurnKeysDecomposing, counts)
  return labels.rigEnhanceBodyFitting
}

function stepTitle(labels: Labels, key: EnhanceStepKey): string {
  return {
    upscale: labels.rigEnhanceStepUpscale,
    figureDecompose: labels.rigEnhanceStepFigureDecompose,
    plainDecompose: labels.rigEnhanceStepFigureDecompose,
    headGenerate: labels.rigEnhanceStepHeadGenerate,
    headDecompose: labels.rigEnhanceStepHeadDecompose,
    headFit: labels.rigEnhanceStepHeadFit,
    stitch: labels.rigEnhanceStepStitch,
    body: labels.rigEnhanceStepBody,
    refit: labels.rigEnhanceStepRefit,
  }[key]
}

function stepDescription(labels: Labels, key: EnhanceStepKey): string {
  return {
    upscale: labels.rigEnhanceStepUpscaleDescription,
    figureDecompose: labels.rigEnhanceStepFigureDecomposeDescription,
    plainDecompose: labels.rigEnhanceStepPlainDecomposeDescription,
    headGenerate: labels.rigEnhanceStepHeadGenerateDescription,
    headDecompose: labels.rigEnhanceStepHeadDecomposeDescription,
    headFit: labels.rigEnhanceStepHeadFitDescription,
    stitch: labels.rigEnhanceStepStitchDescription,
    body: labels.rigEnhanceStepBodyDescription,
    refit: labels.rigEnhanceStepRefitDescription,
  }[key]
}
