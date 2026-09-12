/** 不进 skin。 */

import { userFacingError } from '../../utils/userFacingError'

export function reportBrewError(
  err: unknown,
  fallback: string,
  setError: (message: string) => void,
): void {
  if (err instanceof Error && err.name === 'AbortError') return
  console.error(fallback, err)
  setError(userFacingError(err, fallback))
}

export async function showBrewError(
  err: unknown,
  fallback: string,
): Promise<void> {
  if (err instanceof Error && err.name === 'AbortError') return
  console.error(fallback, err)
  try {
    const { showToast } = await import('../../utils/toastManager')
    showToast({
      message: userFacingError(err, fallback),
      type: 'error',
    })
  } catch {
    /* toast optional */
  }
}
