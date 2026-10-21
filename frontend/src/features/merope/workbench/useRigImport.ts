import type {
  RigAssetCompileEvent,
  RigAssetPreflight,
} from '../assets/pipeline'
import type { TurnArchiveUse, TurnKeyformsStatus } from '../turnKeyformsApi'
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
 * The plain rig: an uploaded PSD, its turn worked out from the front drawing
 * (11°). The enhanced one is split by See-through and adds what is
 * generated: turn keys fitted to four turned drawings (about 30°), for a
 * standing figure its body's turn and weight shift, for the bust the
 * AI-redrawn expressions; each can be chosen on its own.
 */
export type RigMode = 'plain' | 'enhanced'

/** What a standing figure's enhancement keys: its head's turn, its body's turn and weight shift. */
export interface EnhanceParts {
  head: boolean
  body: boolean
}

export interface RigImportSource {
  sourceMasterAssetId: string
  sourceGenerationFingerprint?: string
  seeThroughTokenConfigured: boolean
  onSaveSeeThroughToken: (token: string) => Promise<void>
  /**
   * Splits the portrait with turn keys; with `fromArchive`, uses that kept job
   * instead (fitted again, or as it was). A standing figure keys the `parts` chosen.
   */
  onDecomposeRigPsd: (
    onStatus: (status: TurnKeyformsStatus) => void,
    signal: AbortSignal,
    fromArchive?: TurnArchiveUse,
    parts?: EnhanceParts,
  ) => Promise<File>
  /**
   * The split the server is already running for this portrait (the page was
   * reloaded, or opened in another tab), picked up: its file once done, or
   * null when none runs.
   */
  onResumeRigPsd?: (
    onStatus: (status: TurnKeyformsStatus) => void,
    signal: AbortSignal,
  ) => Promise<File | null>
  /** Splits the portrait once without keys: the base the bust's redrawn expressions go on when its turn is not keyed. */
  onDecomposePlainPsd?: (
    signal: AbortSignal,
    onStatus: (status: TurnKeyformsStatus) => void,
  ) => Promise<File>
  /** A standing figure: its body's turn and weight shift can be keyed beside its head. */
  bodyKeysOffered?: boolean
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
  /** Expressions being drawn on the server, begun before a reload or in another tab. */
  aiExpressionsDrawing?: boolean
  /** Resolves once the server draws none of the expressions any more. */
  awaitAiExpressions?: () => Promise<void>
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
  onResumeRigPsd,
  onDecomposePlainPsd,
  bodyKeysOffered = false,
  onPreflightRigPsd,
  onCommitRigPsd,
  onGenerateAiExpressions,
  aiExpressionsReady = false,
  aiExpressionsDrawing = false,
  awaitAiExpressions,
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
  // A resumed split reads it once the expressions drawn meanwhile are in.
  const aiExpressionsReadyRef = useRef(aiExpressionsReady)
  aiExpressionsReadyRef.current = aiExpressionsReady
  const [aiExpressionsError, setAiExpressionsError] = useState<string | null>(
    null,
  )
  const [mode, setMode] = useState<RigMode>('plain')
  // What the enhanced rig adds; the body's keys and the expressions only where they are offered.
  const [enhanceTurn, setEnhanceTurn] = useState(true)
  const [enhanceBody, setEnhanceBody] = useState(true)
  const [enhanceExpressions, setEnhanceExpressions] = useState(Boolean(onGenerateAiExpressions))
  const expressionsOffered = Boolean(onGenerateAiExpressions)
  const enhanceParts: EnhanceParts = { head: enhanceTurn, body: bodyKeysOffered && enhanceBody }
  const enhanceChosen = enhanceParts.head || enhanceParts.body || (expressionsOffered && enhanceExpressions)
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

  /** Preflights what a split made, with the redrawn expressions when they were chosen. */
  const preflightSplit = async (file: File, expressions: boolean, controller: AbortController) => {
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
  }

  const failSplit = (reason: unknown) => {
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
  }

  // A split the server is still running for this portrait shows its progress
  // again and is preflighted when done, as if it had been started here; a
  // resumed split never redraws expressions, it only uses ones already made.
  useEffect(() => {
    if (!onResumeRigPsd || !sourceMasterAssetId) return
    const controller = new AbortController()
    let picked = false
    const pick = () => {
      if (picked || controller.signal.aborted) return
      picked = true
      importAbortRef.current = controller
      setMode('enhanced')
      setOperation('decompose')
    }
    void (async () => {
      try {
        const file = await onResumeRigPsd((status) => {
          if (controller.signal.aborted) return
          pick()
          setDecomposeStatus(status)
        }, controller.signal)
        if (!file || controller.signal.aborted) return
        pick()
        // Expressions still being drawn (begun before the reload) go in once done.
        await awaitAiExpressions?.()
        if (controller.signal.aborted) return
        await preflightSplit(file, expressionsOffered && enhanceExpressions && aiExpressionsReadyRef.current, controller)
      } catch (reason) {
        if (controller.signal.aborted || !picked) return
        failSplit(reason)
      } finally {
        if (picked && importAbortRef.current === controller && !controller.signal.aborted) {
          importAbortRef.current = null
          setOperation(null)
        }
      }
    })()
    // Asked once per portrait: the callbacks are new each render.
    return () => controller.abort()
  }, [sourceGenerationFingerprint, sourceMasterAssetId])

  /**
   * Splits the portrait and preflights it, enhanced: fits the keys chosen (or
   * refits a kept job) and redraws the expressions as chosen, the two at
   * once, then uses what was made. Only the expressions: one plain split.
   */
  const decomposePsd = async (fromArchive?: TurnArchiveUse) => {
    // A kept job loaded as it was needs nothing of the Space.
    if (importing || !sourceMasterAssetId || (!seeThroughTokenConfigured && fromArchive?.refit !== false)) {
      return
    }
    const enhanced = mode === 'enhanced' || fromArchive !== undefined
    const turn = enhanced && (enhanceTurn || fromArchive !== undefined)
    const body = enhanced && enhanceParts.body && fromArchive === undefined
    const expressions = enhanced && expressionsOffered && enhanceExpressions
    if (!enhanced || (!turn && !body && !expressions)) return
    const controller = new AbortController()
    importAbortRef.current = controller
    setOperation('decompose')
    setDecomposeStatus(null)
    setStage(null)
    setSteps({})
    setResult(null)
    setError(null)
    try {
      const split = turn || body || !onDecomposePlainPsd
        ? onDecomposeRigPsd(
            (status) => { if (!controller.signal.aborted) setDecomposeStatus(status) },
            controller.signal,
            fromArchive,
            { head: turn, body },
          )
        : onDecomposePlainPsd(
            controller.signal,
            (status) => { if (!controller.signal.aborted) setDecomposeStatus(status) },
          )
      // Redrawn once per portrait; kept ones are used as they are.
      const redraw = expressions && !aiExpressionsReady && onGenerateAiExpressions
        ? onGenerateAiExpressions()
        : Promise.resolve()
      const [file] = await Promise.all([split, redraw])
      await preflightSplit(file, expressions, controller)
    } catch (reason) {
      if (controller.signal.aborted) return
      failSplit(reason)
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
    enhanceBody,
    setEnhanceBody,
    bodyKeysOffered,
    enhanceParts,
    enhanceChosen,
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
    aiExpressionsBusy: aiExpressionsBusy || aiExpressionsDrawing,
    aiExpressionsError,
    generateAiExpressions,
  }
}

export type RigImport = ReturnType<typeof useRigImport>
