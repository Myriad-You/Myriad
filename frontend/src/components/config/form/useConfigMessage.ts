import type { ToastType } from '../../Toast'
import type { ShowMessage } from './types'
import { useCallback, useState } from 'react'

export function useConfigMessage() {
  const [message, setMessage] = useState('')
  const [messageType, setMessageType] = useState<ToastType>('info')

  const showMessage: ShowMessage = useCallback(
    (nextMessage, nextType = 'info', duration = 3000) => {
      setMessageType(nextType)
      setMessage(nextMessage)
      if (duration > 0) {
        window.setTimeout(setMessage, duration, '')
      }
    },
    [],
  )

  const clearMessage = useCallback(() => setMessage(''), [])

  return { message, messageType, showMessage, clearMessage }
}
