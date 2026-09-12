import { useEffect, useRef } from 'react'
import { nextReaderDialogTab } from './readerPanels'

const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'

export function readerDialogFocusables(root: ParentNode): HTMLElement[] {
  return Iterator.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).toArray()
}

/** Move focus to close on open, wrap Tab, restore the opener on close. */
export function useReaderDialogFocus(open: boolean, dialogId: string) {
  const closeRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!open) return
    const previous = document.activeElement
    closeRef.current?.focus()
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return
      const root = document.getElementById(dialogId)
      if (!root) return
      const next = nextReaderDialogTab(
        readerDialogFocusables(root),
        document.activeElement,
        event.shiftKey,
      )
      if (!next) return
      event.preventDefault()
      next.focus()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      if (previous instanceof HTMLElement) previous.focus()
    }
  }, [open, dialogId])
  return closeRef
}
