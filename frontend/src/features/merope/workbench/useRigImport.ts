import type {
  RigAssetCompileEvent,
  RigAssetPreflight,
} from '../assets/pipeline'
import type { TurnKeyformsStatus } from '../turnKeyformsApi'
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

/**
 * The plain rig: an uploaded PSD or one decomposition, its turn worked out
 * from the front drawing (11°). The enhanced one adds what is generated:
 * turn keys fitted to four turned drawings (about 30°) and, for the bust, the
 * AI-redrawn expressions; each can be chosen on its own.
 */
export type RigMode = 'plain' | 'enhanced'

export interface RigImportSource {
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  /** Splits the portrait with turn keys; with `fromArchive`, fits that kept job again instead. */
  onDecomposeRigPsd: (
    onStatus: (status: TurnKeyformsStatus) => void,
    signal: AbortSignal,
    fromArchive?: string,
  ) => Promise<File>
  /** Splits the portrait once, without turn keys. */
  onDecomposePlainPsd: (signal: AbortSignal) => Promise<File>
  onPreflightRigPsd: (
    file: File,
    onStage: (event: RigAssetCompileEvent) => void,
    signal?: AbortSignal,
    options?: { aiExpressions?: boolean },
  ) => Promise<RigAssetPreflight>
  onCommitRigPsd: (
    preflight: RigAssetPreflight,
    onStage: (event: RigAssetCompileEvent) => void,
  ) => Promise<{ partCount: number; score: number }>
  /** Redraws the expressions with the image model; absent where they are not offered (full body). */
  onGenerateAiExpressions?: () => Promise<void>
  /** Whether redrawn expressions exist for the current portrait already. */
  aiExpressionsReady?: boolean
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
  onDecomposePlainPsd,
  onPreflightRigPsd,
  onCommitRigPsd,
  onGenerateAiExpressions,
  aiExpressionsReady = false,
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
  // Where a decomposition with keyed turns stands, while it runs.
  const [decomposeStatus, setDecomposeStatus] = useState<TurnKeyformsStatus | null>(null)
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
  const [mode, setMode] = useState<RigMode>('plain')
  // What the enhanced rig adds; expressions only where they are offered.
  const [enhanceTurn, setEnhanceTurn] = useState(true)
  const [enhanceExpressions, setEnhanceExpressions] = useState(Boolean(onGenerateAiExpressions))
  const expressionsOffered = Boolean(onGenerateAiExpressions)
  // The PSD last preflighted here, and whether with the redrawn expressions,
  // so fresh expressions can be tried on it at once.
  const lastRigPsdRef = useRef<File | null>(null)
  const lastWithExpressionsRef = useRef(false)
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

  const preflightPsd = async (file: File, aiExpressions = false) => {
    if (importing || !sourceMasterAssetId) return
    lastRigPsdRef.current = file
    lastWithExpressionsRef.current = aiExpressions
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
        { aiExpressions },
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

  /**
   * Splits the portrait and preflights it. The plain rig splits it once; the
   * enhanced one fits turn keys (or refits a kept job) and redraws the
   * expressions as chosen, the two at once, then uses what was made.
   */
  const decomposePsd = async (fromArchive?: string) => {
    if (importing || !sourceMasterAssetId || !seeThroughTokenConfigured) {
      return
    }
    const enhanced = mode === 'enhanced' || fromArchive !== undefined
    const turn = enhanced && (enhanceTurn || fromArchive !== undefined)
    const expressions = enhanced && expressionsOffered && enhanceExpressions
    if (enhanced && !turn && !expressions) return
    const controller = new AbortController()
    importAbortRef.current = controller
    setOperation('decompose')
    setDecomposeStatus(null)
    setStage(null)
    setSteps({})
    setResult(null)
    setError(null)
    try {
      const split = turn
        ? onDecomposeRigPsd(
            (status) => { if (!controller.signal.aborted) setDecomposeStatus(status) },
            controller.signal,
            fromArchive,
          )
        : onDecomposePlainPsd(controller.signal)
      // Redrawn once per portrait; kept ones are used as they are.
      const redraw = expressions && !aiExpressionsReady && onGenerateAiExpressions
        ? onGenerateAiExpressions()
        : Promise.resolve()
      const [file] = await Promise.all([split, redraw])
      setDecomposeStatus(null)
      controller.signal.throwIfAborted()
      lastRigPsdRef.current = file
      lastWithExpressionsRef.current = expressions
      const imported = await onPreflightRigPsd(file,
        (event) => {
          if (!controller.signal.aborted) recordStage(event)
        },
        controller.signal,
        { aiExpressions: expressions },
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
      // Tried at once on the PSD last preflighted with expressions.
      const file = lastRigPsdRef.current
      if (file && lastWithExpressionsRef.current && !importing) void preflightPsd(file, true)
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
    sourceMasterAssetId,
    mode,
    setMode,
    enhanceTurn,
    setEnhanceTurn,
    enhanceExpressions,
    setEnhanceExpressions,
    expressionsOffered,
    stage,
    decomposeStatus,
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
