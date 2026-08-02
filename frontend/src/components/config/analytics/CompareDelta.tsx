/**
 * Compact 环比 inline after a KPI tile value (same row; overflow scrolls).
 */

import type { CompareKind, CompareLabels, MetricDelta } from './compareDeltaLogic'
import React from 'react'
import {
  compareColorPalette,

  compareKindLabel,

  compareTone,
  formatCompareValue,

} from './compareDeltaLogic'

export interface CompareDeltaProps {
  kind?: CompareKind | string
  delta?: MetricDelta | null
  labels: CompareLabels
  locale: string
  /** Hide while first skeleton load */
  hidden?: boolean
  formatPrevious?: (n: number) => string
}

export function CompareDelta({
  kind,
  delta,
  labels,
  locale,
  hidden,
  formatPrevious,
}: CompareDeltaProps) {
  if (hidden || !delta) return null

  const tone = compareTone(delta)
  if (tone === 'none') return null

  const kindText = compareKindLabel(kind, labels)
  const valueText = formatCompareValue(delta, locale, labels)
  const palette = compareColorPalette(locale)
  const prev = Number(delta.previous ?? 0)
  const prevText = formatPrevious
    ? formatPrevious(prev)
    : String(prev)
  const title = labels.vsPrevious.replace('{n}', prevText)

  return (
    <span
      className={`site-analytics-compare is-${tone}`}
      data-palette={palette}
      title={title}
    >
      <span className="site-analytics-compare-kind">{kindText}</span>
      <span className="site-analytics-compare-value">{valueText}</span>
    </span>
  )
}

export default CompareDelta
