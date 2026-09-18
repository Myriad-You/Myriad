/** Skip login-only fetches when the host already knows this viewer is a guest. */

export function shouldFetchLoginOnly(input: {
  isKnownGuest: boolean
  hasSessionHint?: boolean
}): boolean {
  if (input.hasSessionHint) return true
  return !input.isKnownGuest
}
