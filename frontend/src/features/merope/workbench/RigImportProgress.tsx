import type { RigImport } from './useRigImport'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { PsdReconciliationSummary } from './PsdReconciliationSummary'
import {
  RIG_IMPORT_STEP_ORDER,
  rigDiagnosticMessage,
  rigDiagnosticSeverityLabel,
  rigImportStatusLabel,
  rigImportStepCopy,
} from './rigImportCopy'

/** Where a PSD import stands: the steps, the score, what to fix, and commit. */
export function RigImportProgress({ rig }: { rig: RigImport }) {
  const { t, format } = useI18n()
  const labels = t.merope
  const { stage, steps, result, error, preflight } = rig
  if (!stage && !result && !error && !preflight) return null
  return (
    <section
      className="merope-motion-rig__status"
      aria-live="polite"
    >
      <strong>{labels.rigPreflightTitle}</strong>
      <ol className="merope-motion-rig__steps">
        {RIG_IMPORT_STEP_ORDER.map((step, index) => {
          const status = steps[step] ?? 'pending'
          const copy = rigImportStepCopy(labels, step)
          return (
            <li
              key={step}
              className={`merope-motion-rig__step is-${status}`}
              aria-current={
                status === 'started' ? 'step' : undefined
              }
            >
              <span
                className="merope-motion-rig__step-marker"
                aria-hidden="true"
              >
                {status === 'completed' ? '✓' : index + 1}
              </span>
              <span className="merope-motion-rig__step-copy">
                <b>{copy.title}</b>
                <span>{copy.description}</span>
              </span>
              <span className="merope-motion-rig__step-state">
                {rigImportStatusLabel(labels, status)}
              </span>
            </li>
          )
        })}
      </ol>
      {result ? (
        <div className="merope-motion-rig__summary" role="status">
          <b>
            {format(labels.rigPreflightSummary, {
              parts: result.partCount,
              score: result.score,
            })}
          </b>
          <span>
            {result.activated
              ? labels.rigPreflightActivated
              : labels.rigPreflightReady}
          </span>
        </div>
      ) : null}
      {preflight ? (
        <div className="merope-motion-rig__checks">
          <b>{labels.rigPreflightIssuesTitle}</b>
          {preflight.report.issues.length > 0 ? (
            <ul className="merope-motion-rig__issues">
              {preflight.report.issues
                .slice(0, 6)
                .map((item) => (
                  <li
                    key={`${item.code}:${item.clipId || item.boneId || ''}`}
                  >
                    <span
                      className={`merope-motion-rig__severity is-${item.severity}`}
                    >
                      {rigDiagnosticSeverityLabel(
                        labels,
                        item.severity,
                      )}
                    </span>{' '}
                    {rigDiagnosticMessage(
                      labels,
                      item.code,
                      item.message,
                    )}
                  </li>
                ))}
            </ul>
          ) : (
            <p className="merope-motion-rig__hint">
              {labels.rigPreflightNoIssues}
            </p>
          )}
        </div>
      ) : null}
      {preflight ? (
        <PsdReconciliationSummary
          reconciliation={preflight.prepared.reconciliation}
        />
      ) : null}
      {preflight ? (
        <SettingsButton
          type="button"
          size="sm"
          disabled={
            rig.importing ||
            preflight.report.issues.some(
              (item) => item.severity === 'error',
            )
          }
          loading={rig.operation === 'commit'}
          onClick={() => void rig.commitPsd()}
        >
          {labels.motionPsdCommit}
        </SettingsButton>
      ) : null}
    </section>
  )
}
