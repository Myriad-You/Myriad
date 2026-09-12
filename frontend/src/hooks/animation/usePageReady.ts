import { useEffect, useState } from 'react'
import { coordinator } from './coordinator'

export function usePageReady(): boolean {
  const [isReady, setIsReady] = useState(() => coordinator.getPageReadyState())

  useEffect(() => {
    if (coordinator.getPageReadyState()) {
      setIsReady(true)
      return
    }

    const unsubscribe = coordinator.onPageReady(() => {
      setIsReady(true)
    })

    return unsubscribe
  }, [])

  return isReady
}

export default usePageReady
