import { FigureError } from '@myriad/figure'
import { currentCopy, formatCurrent } from '../../../i18n/localeCopy'

/**
 * A figure error in the site's language; any other error passes through.
 * The player's own errors count as figure errors, and a code the site has no
 * words for keeps the figure's English message.
 */
export function localizeFigureError(error: unknown): unknown {
  if (!(error instanceof FigureError)) return error
  const words: Readonly<Record<string, unknown>> = currentCopy().merope
  const template = words[error.code]
  return new Error(typeof template === 'string' ? formatCurrent(template, error.params) : error.message, { cause: error })
}
