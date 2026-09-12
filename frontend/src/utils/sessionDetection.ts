/** Boolean only; not auth. */

const SESSION_HINT_KEY = 'myriad_session_hint'

export function setSessionHint(): void {
  try {
    localStorage.setItem(SESSION_HINT_KEY, 'true')
  } catch {
  }
}

export function hasSessionHint(): boolean {
  try {
    return localStorage.getItem(SESSION_HINT_KEY) === 'true'
  } catch {
    return false
  }
}

export function clearSessionHint(): void {
  try {
    localStorage.removeItem(SESSION_HINT_KEY)
  } catch {
  }
}
