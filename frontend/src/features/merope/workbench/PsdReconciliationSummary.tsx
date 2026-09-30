import type {
  Anime25DPsdDefectKind,
  Anime25DPsdReconciliation,
} from '../rig/psdReconciliation'
import { useEffect, useRef } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  ANIME25D_PSD_DEFECT_COLORS,
  ANIME25D_PSD_REPAIRED_COLOR,
} from '../rig/psdReconciliation'

const KINDS: readonly Anime25DPsdDefectKind[] = [
  'buried',
  'missing',
  'mismatch',
  'spurious',
]

/** Where the split PSD disagreed with its source, and what import repaired. */
export function PsdReconciliationSummary({
  reconciliation,
}: {
  reconciliation: Anime25DPsdReconciliation | null
}) {
  const { t, format } = useI18n()
  const labels = t.merope
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const heatmap = reconciliation?.heatmap ?? null

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !heatmap) return
    canvas.width = heatmap.width
    canvas.height = heatmap.height
    canvas
      .getContext('2d')
      ?.putImageData(
        new ImageData(
          new Uint8ClampedArray(heatmap.data),
          heatmap.width,
          heatmap.height,
        ),
        0,
        0,
      )
  }, [heatmap])

  const percent = (value: number) => Math.round(value * 1000) / 10
  const templates: Record<Anime25DPsdDefectKind, string> = {
    buried: labels.rigReconcileBuried,
    missing: labels.rigReconcileMissing,
    mismatch: labels.rigReconcileMismatch,
    spurious: labels.rigReconcileSpurious,
  }

  let body
  if (!reconciliation) {
    body = (
      <p className="merope-motion-rig__hint">
        {labels.rigReconcileUnavailable}
      </p>
    )
  } else if (reconciliation.status === 'reference-mismatch') {
    body = (
      <p className="merope-motion-rig__hint">
        {format(labels.rigReconcileReferenceMismatch, {
          agreement: percent(reconciliation.agreement),
        })}
      </p>
    )
  } else {
    const groups = KINDS.map((kind) => {
      const regions = reconciliation.regions.filter(
        (region) => region.kind === kind,
      )
      const roles = [
        ...new Set(
          regions.flatMap((region) => (region.role ? [region.role] : [])),
        ),
      ]
      return { kind, count: regions.length, roles }
    }).filter((group) => group.count > 0)
    const { revealed, recovered } = reconciliation.repairs
    const repaired = revealed + recovered > 0
    body = (
      <>
        <p className="merope-motion-rig__hint">
          {format(labels.rigReconcileAgreement, {
            agreement: percent(reconciliation.agreement),
          })}
        </p>
        {repaired ? (
          <p className="merope-motion-rig__hint">
            <span
              className="merope-motion-rig__reconcile-swatch"
              style={{
                background: `rgb(${ANIME25D_PSD_REPAIRED_COLOR.join(' ')})`,
              }}
              aria-hidden="true"
            />
            {format(labels.rigReconcileRepaired, { revealed, recovered })}
          </p>
        ) : null}
        {repaired && groups.length > 0 ? (
          <p className="merope-motion-rig__hint">
            {labels.rigReconcileRemaining}
          </p>
        ) : null}
        {groups.length > 0 ? (
          <ul className="merope-motion-rig__issues merope-motion-rig__reconcile-list">
            {groups.map(({ kind, count, roles }) => (
              <li key={kind}>
                <span
                  className="merope-motion-rig__reconcile-swatch"
                  style={{
                    background: `rgb(${ANIME25D_PSD_DEFECT_COLORS[kind].join(' ')})`,
                  }}
                  aria-hidden="true"
                />
                {format(templates[kind], {
                  count,
                  roles: roles.slice(0, 4).join(', '),
                })}
              </li>
            ))}
          </ul>
        ) : (
          <p className="merope-motion-rig__hint">
            {repaired
              ? labels.rigReconcileRemainingClean
              : labels.rigReconcileClean}
          </p>
        )}
        {reconciliation.backgroundKnown ? null : (
          <p className="merope-motion-rig__hint">
            {labels.rigReconcileBackgroundUnknown}
          </p>
        )}
        {heatmap && (groups.length > 0 || repaired) ? (
          <figure className="merope-motion-rig__reconcile-map">
            <canvas ref={canvasRef} aria-label={labels.rigReconcileHeatmap} />
            <figcaption>{labels.rigReconcileHeatmap}</figcaption>
          </figure>
        ) : null}
      </>
    )
  }

  return (
    <div className="merope-motion-rig__checks">
      <b>{labels.rigReconcileTitle}</b>
      {body}
    </div>
  )
}
