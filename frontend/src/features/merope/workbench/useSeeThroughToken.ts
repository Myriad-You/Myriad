import { useCallback, useEffect, useState } from 'react'
import { userFacingError } from '../../../utils/userFacingError'
import { getSeeThroughStatus, updateSeeThroughToken } from '../api'
import { reportMeropeError } from './workbenchShared'

/** Whether the owner has a Hugging Face token for remote See-through. */
export function useSeeThroughToken(statusFailed: string) {
  const [configured, setConfigured] = useState(false)

  useEffect(() => {
    let cancelled = false
    void getSeeThroughStatus()
      .then((status) => {
        if (!cancelled) setConfigured(status.tokenConfigured)
      })
      .catch((reason) => {
        if (!cancelled) {
          setConfigured(false)
          reportMeropeError(userFacingError(reason, statusFailed))
        }
      })
    return () => {
      cancelled = true
    }
  }, [statusFailed])

  const save = useCallback(async (token: string) => {
    const status = await updateSeeThroughToken(token)
    setConfigured(status.tokenConfigured)
  }, [])

  return { configured, save }
}
