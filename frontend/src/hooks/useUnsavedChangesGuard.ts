import { useEffect } from 'react'

/** Requests the browser's native warning on reload, close, or document navigation. */
export function useUnsavedChangesGuard(when: boolean, _message: string) {
  useEffect(() => {
    if (!when) return
    const beforeUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', beforeUnload, { capture: true })
    return () =>
      window.removeEventListener('beforeunload', beforeUnload, { capture: true })
  }, [when])
}
