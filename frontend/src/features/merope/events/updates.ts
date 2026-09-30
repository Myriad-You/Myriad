export const FACE_UPDATED_EVENT = 'arael-face-updated'
export const PERSONA_UPDATED_EVENT = 'arael-persona-updated'

export function notifyPersonaUpdated(): void {
  window.dispatchEvent(new CustomEvent(PERSONA_UPDATED_EVENT))
}

export function notifyFaceUpdated(): void {
  window.dispatchEvent(new CustomEvent(FACE_UPDATED_EVENT))
  notifyPersonaUpdated()
}
