import { useEffect, useState } from 'react'
import { RemoteStoreService } from '../../../tapp/services/RemoteStoreService'

/** Fail-open until the web-process store policy answers, matching the gate. */
export function useFederationEnabled(): boolean {
  const [enabled, setEnabled] = useState(true)
  useEffect(() => {
    let cancelled = false
    void RemoteStoreService.fetchPolicy()
      .then(({ federationEnabled }) => {
        if (!cancelled) setEnabled(federationEnabled)
      })
      .catch(() => {
        if (!cancelled) setEnabled(true)
      })
    return () => {
      cancelled = true
    }
  }, [])
  return enabled
}
