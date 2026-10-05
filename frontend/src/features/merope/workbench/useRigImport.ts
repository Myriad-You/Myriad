import type {
  RigAssetCompileEvent,
  RigAssetPreflight,
} from '../assets/pipeline'
import { useEffect, useRef, useState } from 'react'
import { generationFailureMessage } from '../../../components/agent/onboarding/generationError'
import { useI18n } from '../../../contexts/I18nContext'
import { showStickyToast } from '../../../utils/toastManager'
import { userFacingError } from '../../../utils/userFacingError'

export type RigImportStepState = Partial<
  Record<RigAssetCompileEvent['stage'], RigAssetCompileEvent['status']>
>

export interface RigImportSummary {
  partCount: number
  score: number
  activated: boolean
}

export interface RigImportSource {
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  onDecomposeRigPsd: () => Promise<File>
  onPreflightRigPsd: (
    file: File,
    onStage: (event: RigAssetCompileEvent) => void,
    signal?: AbortSignal,
  ) => Promise<RigAssetPreflight>
  onCommitRigPsd: (
    preflight: RigAssetPreflight,
    onStage: (event: RigAssetCompileEvent) => void,
  ) => Promise<{ partCount: number; score: number }>
  onGenerateAiExpressions?: () => Promise<void>
}

/**
 * A PSD's way into the rig: split remotely or uploaded, preflighted, then
 * committed. The state outlives tab switches, so it lives above the panel.
 */
export function useRigImport({
  sourceMasterAssetId,
  sourceGenerationFingerprint,
  seeThroughTokenConfigured,
  onSaveSeeThroughToken,
  onDecomposeRigPsd,
  onPreflightRigPsd,
  onCommitRigPsd,
  onGenerateAiExpressions,
}: RigImportSource) {
  const { t } = useI18n()
  const labels = t.merope
  const seeThroughErrors = {
    see_through_token_required: labels.motionSeeThroughTokenRequired,
    see_through_busy: labels.motionSeeThroughBusy,
    see_through_auth_failed: labels.motionSeeThroughAuthFailed,
    see_through_quota_unavailable: labels.motionSeeThroughQuota,
    see_through_timeout: labels.motionSeeThroughTimeout,
    see_through_upstream_failed: labels.motionSeeThroughUpstream,
    see_through_invalid_input: labels.motionSeeThroughUpstream,
    see_through_space_waking: t.errors.byCode.see_through_space_waking,
    see_through_space_unavailable: t.errors.byCode.see_through_space_unavailable,
  }
  const [stage, setStage] = useState<RigAssetCompileEvent | null>(null)
  const [steps, setSteps] = useState<RigImportStepState>({})
  const [result, setResult] = useState<RigImportSummary | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [preflight, setPreflight] = useState<RigAssetPreflight | null>(null)
  const [tokenDraft, setTokenDraft] = useState('')
  const [tokenError, setTokenError] = useState<string | undefined>()
  const [operation, setOperation] = useState<
    'decompose' | 'manual' | 'commit' | null
  >(null)
  const importing = operation !== null
  const [aiExpressionsBusy, setAiExpressionsBusy] = useState(false)
  const [aiExpressionsError, setAiExpressionsError] = useState<string | null>(
    null,
  )
  // The PSD last preflighted here, so fresh expressions can be tried on it at once.
  const lastRigPsdRef = useRef<File | null>(null)
  const importAbortRef = useRef<AbortController | null>(null)
  const failRigImport = (message: string) => {
    setError(message)
    showStickyToast({
      message,
      type: 'error',
      replaceKey: 'merope-rig-import',
    })
  }

  useEffect(() => {
    importAbortRef.current?.abort()
    importAbortRef.current = null
    setOperation(null)
    setPreflight(null)
    setStage(null)
    setSteps({})
    setResult(null)
    setError(null)
    return () => { importAbortRef.current?.abort() }
  }, [sourceGenerationFingerprint, sourceMasterAssetId])

  const saveToken = async (token: string) => {
    if (!token || token.includes('•') || token.includes('*')) return
    setTokenError(undefined)
    try {
      await onSaveSeeThroughToken(token)
      setTokenDraft('')
    } catch (reason) {
      setTokenError(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.motionSeeThroughTokenFailed,
            labels.motionSeeThroughTimeout,
          ),
          labels.motionSeeThroughTokenFailed,
        ),
      )
      throw reason
    }
  }

  const editToken = (value: string) => {
    setTokenDraft(value)
    setTokenError(undefined)
  }

  const recordStage = (event: RigAssetCompileEvent) => {
    setStage(event)
    setSteps((current) => ({
      ...current,
      [event.stage]: event.status,
    }))
  }

  const preflightPsd = async (file: File) => {
    if (importing || !sourceMasterAssetId) return
    lastRigPsdRef.current = file
    const controller = new AbortController()
    importAbortRef.current = controller
    setOperation('manual')
    setStage(null)
    setSteps({})
    setResult(null)
    setError(null)
    try {
      const imported = await onPreflightRigPsd(file,
        (event) => {
          if (!controller.signal.aborted) recordStage(event)
        },
        controller.signal,
      )
      controller.signal.throwIfAborted()
      setPreflight(imported)
      setResult({
        partCount: imported.partCount,
        score: imported.report.score,
        activated: false,
      })
    } catch (reason) {
      if (controller.signal.aborted) return
      failRigImport(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.rigImportFailed,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.rigImportFailed,
        ),
      )
    } finally {
      if (importAbortRef.current === controller && !controller.signal.aborted) {
        importAbortRef.current = null
        setOperation(null)
      }
    }
  }

  const decomposePsd = async () => {
    if (importing || !sourceMasterAssetId || !seeThroughTokenConfigured) {
      return
    }
    const controller = new AbortController()
    importAbortRef.current = controller
    setOperation('decompose')
    setStage(null)
    setSteps({})
    setResult(null)
    setError(null)
    try {
      const file = await onDecomposeRigPsd()
      controller.signal.throwIfAborted()
      lastRigPsdRef.current = file
      const imported = await onPreflightRigPsd(file,
        (event) => {
          if (!controller.signal.aborted) recordStage(event)
        },
        controller.signal,
      )
      controller.signal.throwIfAborted()
      setPreflight(imported)
      setResult({
        partCount: imported.partCount,
        score: imported.report.score,
        activated: false,
      })
    } catch (reason) {
      if (controller.signal.aborted) return
      failRigImport(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.motionSeeThroughUpstream,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.motionSeeThroughUpstream,
        ),
      )
    } finally {
      if (importAbortRef.current === controller && !controller.signal.aborted) {
        importAbortRef.current = null
        setOperation(null)
      }
    }
  }

  /** Stop an upload's preflight; the PSD stays unimported. */
  const cancelPreflight = () => {
    importAbortRef.current?.abort()
    importAbortRef.current = null
    setOperation(null)
    setStage(null)
    setSteps({})
  }

  const generateAiExpressions = async () => {
    if (aiExpressionsBusy || !onGenerateAiExpressions) return
    setAiExpressionsBusy(true)
    setAiExpressionsError(null)
    try {
      await onGenerateAiExpressions()
      const file = lastRigPsdRef.current
      if (file && !importing) void preflightPsd(file)
    } catch (reason) {
      setAiExpressionsError(userFacingError(reason, labels.aiExpressionsFailed))
    } finally {
      setAiExpressionsBusy(false)
    }
  }

  const commitPsd = async () => {
    if (importing || !preflight) return
    setOperation('commit')
    setStage(null)
    setError(null)
    try {
      const imported = await onCommitRigPsd(preflight, recordStage)
      setResult({
        partCount: imported.partCount,
        score: imported.score,
        activated: true,
      })
      setPreflight(null)
    } catch (reason) {
      failRigImport(
        userFacingError(
          generationFailureMessage(
            reason,
            labels.rigCommitFailed,
            labels.motionSeeThroughTimeout,
            seeThroughErrors,
          ),
          labels.rigCommitFailed,
        ),
      )
    } finally {
      setOperation(null)
    }
  }

  return {
    stage,
    steps,
    result,
    error,
    preflight,
    operation,
    importing,
    tokenDraft,
    tokenError,
    editToken,
    saveToken,
    preflightPsd,
    decomposePsd,
    cancelPreflight,
    commitPsd,
    aiExpressionsBusy,
    aiExpressionsError,
    generateAiExpressions,
  }
}

export type RigImport = ReturnType<typeof useRigImport>
