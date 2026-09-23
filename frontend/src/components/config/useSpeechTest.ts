import { useCallback, useEffect, useRef } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import { testSpeechService } from '../../services/configApi'
import { userFacingError } from '../../utils/userFacingError'

export function useSpeechTest() {
  const { t } = useI18n()
  const speechTestAudioRef = useRef<{
    audio: HTMLAudioElement
    url: string
  } | null>(null)

  const releaseSpeechTestAudio = useCallback(() => {
    const current = speechTestAudioRef.current
    if (!current) return
    speechTestAudioRef.current = null
    current.audio.pause()
    current.audio.removeAttribute('src')
    current.audio.load()
    URL.revokeObjectURL(current.url)
  }, [])

  useEffect(() => () => releaseSpeechTestAudio(), [releaseSpeechTestAudio])

  return useCallback(async (): Promise<{
    success: boolean
    message: string
  }> => {
    try {
      const result = await testSpeechService()
      if (result.audio) {
        try {
          const binary = atob(result.audio)
          const bytes = new Uint8Array(binary.length)
          for (let i = 0; i < binary.length; i += 1) {
            bytes[i] = binary.charCodeAt(i)
          }
          const blob = new Blob([bytes], {
            type:
              bytes.length >= 12 &&
              bytes[0] === 0x52 &&
              bytes[1] === 0x49 &&
              bytes[2] === 0x46 &&
              bytes[3] === 0x46
                ? 'audio/wav'
                : 'audio/mpeg',
          })
          releaseSpeechTestAudio()
          const url = URL.createObjectURL(blob)
          const audio = new Audio(url)
          speechTestAudioRef.current = { audio, url }
          const release = () => {
            if (speechTestAudioRef.current?.url !== url) return
            releaseSpeechTestAudio()
          }
          audio.addEventListener('ended', release, { once: true })
          void audio.play().catch(release)
        } catch {
          // 播放是尽力而为，API 结果仍算数。
        }
      }
      if (result.success) {
        if (result.tts_skipped) {
          return {
            success: true,
            message: userFacingError(
              result.error,
              t.config.speechOpenRouterTtsHint,
            ),
          }
        }
        return { success: true, message: t.config.speechTestSuccess }
      }
      return {
        success: false,
        message: userFacingError(result.error, t.config.speechTestFailed),
      }
    } catch {
      return { success: false, message: t.config.speechTestFailed }
    }
  }, [releaseSpeechTestAudio, t])
}
