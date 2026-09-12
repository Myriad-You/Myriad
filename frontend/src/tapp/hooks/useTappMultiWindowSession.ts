import { useEffect, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { useBreakpoints } from '../../hooks/useSharedEventListener'
import { isWebKit } from '../../utils/platformDetect'

export function useTappMultiWindowSession(): boolean {
  const [searchParams] = useSearchParams()
  const { isMobile } = useBreakpoints()
  const multiParam = searchParams.get('multi') === 'true' && !isWebKit

  // 对调会销毁 iframe、丢掉 Tapp 状态。
  const [multiSessionActive, setMultiSessionActive] = useState(false)
  useEffect(() => {
    if (multiParam && !isMobile) {
      setMultiSessionActive(true)
    }
    if (!multiParam) {
      setMultiSessionActive(false)
    }
  }, [multiParam, isMobile])

  return multiParam && (!isMobile || multiSessionActive)
}
