/** Non-React Tapp bridges must not forward user-scoped federation as a guest (401). */

export type KnownAuthState = boolean | null

let knownAuthState: KnownAuthState = null

export function isKnownGuest(): boolean {
  return knownAuthState === false
}

/** Do not call on 5xx/network. Logout writes false, never null. */
export function setKnownAuthState(state: boolean): void {
  knownAuthState = state
}
