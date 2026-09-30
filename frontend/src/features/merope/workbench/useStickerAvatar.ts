import { useCallback, useState } from 'react'
import { notifyAvatarChanged } from '../../../services/avatarSourceApi'
import { invalidatePublicConfigCache } from '../../../utils/requestDedup'
import { userFacingError } from '../../../utils/userFacingError'
import { generateStickerAvatar } from '../api'
import { refreshPersonaStickerAvatar } from '../personaAvatar'
import { reportMeropeError } from './workbenchShared'

/** Tell every avatar slot the sticker changed. */
function announceStickerChange() {
  // 公开配置缓存 30s，必须作废才能换通知图标。
  invalidatePublicConfigCache()
  void refreshPersonaStickerAvatar()
  // 其它头像位可能仍戴上一张贴纸。
  notifyAvatarChanged()
}

/** The Q-sticker derived from the master portrait. */
export function useStickerAvatar(portraitUrl: string | null, failed: string) {
  const [url, setUrl] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  /** 主立绘可能在本页打开后被清掉。 */
  const make = useCallback(async () => {
    if (busy || !portraitUrl) return
    setBusy(true)
    reportMeropeError('')
    try {
      const result = await generateStickerAvatar()
      setUrl(result.avatarUrl)
      announceStickerChange()
    } catch (reason) {
      reportMeropeError(userFacingError(reason, failed))
    } finally {
      setBusy(false)
    }
  }, [busy, failed, portraitUrl])

  /** A new master portrait voids the old sticker in the same write. */
  const voided = useCallback(() => {
    setUrl(null)
    announceStickerChange()
  }, [])

  return { url, setUrl, busy, make, voided }
}
