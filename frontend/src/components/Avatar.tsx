import type { CSSProperties } from 'react'

import { useState } from 'react'
import { localFallbackAvatar, resolveAvatar } from '../utils/avatar'

interface AvatarProps {
  src?: string | null
  name?: string | null
  size?: number
  className?: string
  style?: CSSProperties
  decorative?: boolean
}

export function Avatar({
  src,
  name,
  size,
  className,
  style,
  decorative = false,
}: AvatarProps) {
  const seed = name ?? undefined
  // 记录失败的 src；换 src 时在 render 同步清失败，useEffect 会闪一帧 fallback。
  const [failedForSrc, setFailedForSrc] = useState<string | null | undefined>(
    undefined,
  )
  const failed = failedForSrc === src

  const resolved = failed ? localFallbackAvatar(seed) : resolveAvatar(src, seed)

  return (
    <img
      src={resolved}
      alt={decorative ? '' : (name ?? '')}
      width={size}
      height={size}
      className={className}
      style={style}
      referrerPolicy="no-referrer"
      decoding="async"
      onError={() => setFailedForSrc(src)}
    />
  )
}

export default Avatar
