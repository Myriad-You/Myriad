import type * as FigureRoot from '@myriad/figure'
import type * as InterfaceRoot from '@myriad/figure-interface'
import type * as InterfaceAnime25drig from '@myriad/figure-interface/anime25drig'
import type * as InterfaceEvents from '@myriad/figure-interface/events'
import type * as InterfaceInteraction from '@myriad/figure-interface/interaction'
import type * as InterfaceMotion from '@myriad/figure-interface/motion'
import type * as InterfacePerformance from '@myriad/figure-interface/performance'
import type * as InterfaceRig from '@myriad/figure-interface/rig'
import type * as InterfaceSinging from '@myriad/figure-interface/singing'
import type * as InterfaceSpeech from '@myriad/figure-interface/speech'
import type * as InterfaceWorkbench from '@myriad/figure-interface/workbench'
// @myriad/figure provides what @myriad/figure-interface declares, checked by the type checker:
// every value an interface entry declares is in the same figure entry with a type that
// fits the interface's, every data type it names is interchangeable with figure's, and
// every behaviour type (the interface keeps only the members the site uses) is one that
// figure's stands in for. Nothing imports this file; `tsc` reads it with the rest of src/.
import type * as FigureAnime25drig from '@myriad/figure/anime25drig'
import type * as FigureEvents from '@myriad/figure/events'
import type * as FigureInteraction from '@myriad/figure/interaction'
import type * as FigureMotion from '@myriad/figure/motion'
import type * as FigurePerformance from '@myriad/figure/performance'
import type * as FigureRig from '@myriad/figure/rig'
import type * as FigureSinging from '@myriad/figure/singing'
import type * as FigureSpeech from '@myriad/figure/speech'
import type * as FigureWorkbench from '@myriad/figure/workbench'

// Parameters compare both ways, as methods' do: the interface may describe an argument
// by fewer members than figure's implementation declares. A method signature is what
// makes them compare both ways, so it stays one.
// eslint-disable-next-line ts/method-signature-style
type Bivariant<T> = T extends (...args: infer A) => infer R ? { method(...args: A): R }['method'] : T
type Values<M> = { [K in keyof M]: Bivariant<M[K]> }
type Provides<Impl extends Iface, Iface> = [Impl, Iface]
type Accepts<Iface extends Impl, Impl> = [Iface, Impl]

export type FigureProvidesInterface = [
  Provides<typeof FigureAnime25drig, Values<typeof InterfaceAnime25drig>>,
  Provides<FigureAnime25drig.Anime25DPlaybackDocument, InterfaceAnime25drig.Anime25DPlaybackDocument>,
  Accepts<InterfaceAnime25drig.Anime25DPlaybackDocument, FigureAnime25drig.Anime25DPlaybackDocument>,
  Provides<FigureAnime25drig.Anime25DWorkbenchPort, InterfaceAnime25drig.Anime25DWorkbenchPort>,
  Accepts<InterfaceAnime25drig.Anime25DWorkbenchPort, FigureAnime25drig.Anime25DWorkbenchPort>,
  Provides<FigureAnime25drig.PoseCorrection, InterfaceAnime25drig.PoseCorrection>,
  Accepts<InterfaceAnime25drig.PoseCorrection, FigureAnime25drig.PoseCorrection>,
  Provides<typeof FigureEvents, Values<typeof InterfaceEvents>>,
  Provides<FigureEvents.MeropePerformanceEventDetail, InterfaceEvents.MeropePerformanceEventDetail>,
  Accepts<InterfaceEvents.MeropePerformanceEventDetail, FigureEvents.MeropePerformanceEventDetail>,
  Provides<FigureEvents.MeropeStateEventDetail, InterfaceEvents.MeropeStateEventDetail>,
  Accepts<InterfaceEvents.MeropeStateEventDetail, FigureEvents.MeropeStateEventDetail>,
  Provides<typeof FigureRoot, Values<typeof InterfaceRoot>>,
  Provides<typeof FigureInteraction, Values<typeof InterfaceInteraction>>,
  Provides<typeof FigureMotion, Values<typeof InterfaceMotion>>,
  Provides<FigureMotion.BehaviorPlan, InterfaceMotion.BehaviorPlan>,
  Accepts<InterfaceMotion.BehaviorPlan, FigureMotion.BehaviorPlan>,
  Provides<FigureMotion.LiveFaceConsumer, InterfaceMotion.LiveFaceConsumer>,
  Accepts<InterfaceMotion.LiveFaceConsumer, FigureMotion.LiveFaceConsumer>,
  Provides<FigureMotion.MotionChannelPolicy, InterfaceMotion.MotionChannelPolicy>,
  Accepts<InterfaceMotion.MotionChannelPolicy, FigureMotion.MotionChannelPolicy>,
  Provides<FigureMotion.MotionRuntime, InterfaceMotion.MotionRuntime>,
  Provides<FigureMotion.MusicMotionAudio, InterfaceMotion.MusicMotionAudio>,
  Accepts<InterfaceMotion.MusicMotionAudio, FigureMotion.MusicMotionAudio>,
  Provides<FigureMotion.MusicMotionClock, InterfaceMotion.MusicMotionClock>,
  Accepts<InterfaceMotion.MusicMotionClock, FigureMotion.MusicMotionClock>,
  Provides<FigureMotion.MusicMotionVisibility, InterfaceMotion.MusicMotionVisibility>,
  Accepts<InterfaceMotion.MusicMotionVisibility, FigureMotion.MusicMotionVisibility>,
  Provides<FigureMotion.ResolvedScore, InterfaceMotion.ResolvedScore>,
  Accepts<InterfaceMotion.ResolvedScore, FigureMotion.ResolvedScore>,
  Provides<FigureMotion.RigBearing, InterfaceMotion.RigBearing>,
  Accepts<InterfaceMotion.RigBearing, FigureMotion.RigBearing>,
  Provides<FigureMotion.TouchMotionSource, InterfaceMotion.TouchMotionSource>,
  Provides<typeof FigurePerformance, Values<typeof InterfacePerformance>>,
  Provides<FigurePerformance.BodyControl, InterfacePerformance.BodyControl>,
  Accepts<InterfacePerformance.BodyControl, FigurePerformance.BodyControl>,
  Provides<FigurePerformance.BodyPose, InterfacePerformance.BodyPose>,
  Accepts<InterfacePerformance.BodyPose, FigurePerformance.BodyPose>,
  Provides<FigurePerformance.MeropeActivity, InterfacePerformance.MeropeActivity>,
  Accepts<InterfacePerformance.MeropeActivity, FigurePerformance.MeropeActivity>,
  Provides<FigurePerformance.MoodBand, InterfacePerformance.MoodBand>,
  Accepts<InterfacePerformance.MoodBand, FigurePerformance.MoodBand>,
  Provides<FigurePerformance.MoodBandName, InterfacePerformance.MoodBandName>,
  Accepts<InterfacePerformance.MoodBandName, FigurePerformance.MoodBandName>,
  Provides<FigurePerformance.MoodTransition, InterfacePerformance.MoodTransition>,
  Accepts<InterfacePerformance.MoodTransition, FigurePerformance.MoodTransition>,
  Provides<FigurePerformance.PerformanceBaseline, InterfacePerformance.PerformanceBaseline>,
  Accepts<InterfacePerformance.PerformanceBaseline, FigurePerformance.PerformanceBaseline>,
  Provides<FigurePerformance.PerformanceCue, InterfacePerformance.PerformanceCue>,
  Accepts<InterfacePerformance.PerformanceCue, FigurePerformance.PerformanceCue>,
  Provides<FigurePerformance.PerformanceDirective, InterfacePerformance.PerformanceDirective>,
  Accepts<InterfacePerformance.PerformanceDirective, FigurePerformance.PerformanceDirective>,
  Provides<FigurePerformance.PerformancePhase, InterfacePerformance.PerformancePhase>,
  Accepts<InterfacePerformance.PerformancePhase, FigurePerformance.PerformancePhase>,
  Provides<FigurePerformance.RigBeatPhase, InterfacePerformance.RigBeatPhase>,
  Accepts<InterfacePerformance.RigBeatPhase, FigurePerformance.RigBeatPhase>,
  Provides<FigurePerformance.RigBehaviorFunction, InterfacePerformance.RigBehaviorFunction>,
  Accepts<InterfacePerformance.RigBehaviorFunction, FigurePerformance.RigBehaviorFunction>,
  Provides<FigurePerformance.RigBehaviorPhase, InterfacePerformance.RigBehaviorPhase>,
  Accepts<InterfacePerformance.RigBehaviorPhase, FigurePerformance.RigBehaviorPhase>,
  Provides<FigurePerformance.RigMotionStyle, InterfacePerformance.RigMotionStyle>,
  Accepts<InterfacePerformance.RigMotionStyle, FigurePerformance.RigMotionStyle>,
  Provides<FigurePerformance.RigMusicEnergy, InterfacePerformance.RigMusicEnergy>,
  Accepts<InterfacePerformance.RigMusicEnergy, FigurePerformance.RigMusicEnergy>,
  Provides<FigurePerformance.RigStateSummary, InterfacePerformance.RigStateSummary>,
  Accepts<InterfacePerformance.RigStateSummary, FigurePerformance.RigStateSummary>,
  Provides<FigurePerformance.ScoreBeat, InterfacePerformance.ScoreBeat>,
  Accepts<InterfacePerformance.ScoreBeat, FigurePerformance.ScoreBeat>,
  Provides<FigurePerformance.ScoreDirection, InterfacePerformance.ScoreDirection>,
  Accepts<InterfacePerformance.ScoreDirection, FigurePerformance.ScoreDirection>,
  Provides<FigurePerformance.ScoreMove, InterfacePerformance.ScoreMove>,
  Accepts<InterfacePerformance.ScoreMove, FigurePerformance.ScoreMove>,
  Provides<FigurePerformance.ScoreMoveKind, InterfacePerformance.ScoreMoveKind>,
  Accepts<InterfacePerformance.ScoreMoveKind, FigurePerformance.ScoreMoveKind>,
  Provides<FigurePerformance.ScoreSide, InterfacePerformance.ScoreSide>,
  Accepts<InterfacePerformance.ScoreSide, FigurePerformance.ScoreSide>,
  Provides<FigurePerformance.SpeechPhrase, InterfacePerformance.SpeechPhrase>,
  Accepts<InterfacePerformance.SpeechPhrase, FigurePerformance.SpeechPhrase>,
  Provides<typeof FigureRig, Values<typeof InterfaceRig>>,
  Provides<FigureRig.ArmVariantGesture, InterfaceRig.ArmVariantGesture>,
  Accepts<InterfaceRig.ArmVariantGesture, FigureRig.ArmVariantGesture>,
  Provides<FigureRig.ArmVariantSource, InterfaceRig.ArmVariantSource>,
  Accepts<InterfaceRig.ArmVariantSource, FigureRig.ArmVariantSource>,
  Provides<FigureRig.AuthoredExpressionKind, InterfaceRig.AuthoredExpressionKind>,
  Accepts<InterfaceRig.AuthoredExpressionKind, FigureRig.AuthoredExpressionKind>,
  Provides<FigureRig.DetectedSkeleton, InterfaceRig.DetectedSkeleton>,
  Accepts<InterfaceRig.DetectedSkeleton, FigureRig.DetectedSkeleton>,
  Provides<FigureRig.MeropeRigImportSource, InterfaceRig.MeropeRigImportSource>,
  Accepts<InterfaceRig.MeropeRigImportSource, FigureRig.MeropeRigImportSource>,
  Provides<FigureRig.MeropeRigManifest, InterfaceRig.MeropeRigManifest>,
  Accepts<InterfaceRig.MeropeRigManifest, FigureRig.MeropeRigManifest>,
  Provides<FigureRig.PreparedRigPsdImport, InterfaceRig.PreparedRigPsdImport>,
  Accepts<InterfaceRig.PreparedRigPsdImport, FigureRig.PreparedRigPsdImport>,
  Provides<FigureRig.RigMotionPort, InterfaceRig.RigMotionPort>,
  Accepts<InterfaceRig.RigMotionPort, FigureRig.RigMotionPort>,
  Provides<FigureRig.RigPsdImportOptions, InterfaceRig.RigPsdImportOptions>,
  Accepts<InterfaceRig.RigPsdImportOptions, FigureRig.RigPsdImportOptions>,
  Provides<FigureRig.SpeechArticulation, InterfaceRig.SpeechArticulation>,
  Accepts<InterfaceRig.SpeechArticulation, FigureRig.SpeechArticulation>,
  Provides<FigureRig.SpeechViseme, InterfaceRig.SpeechViseme>,
  Accepts<InterfaceRig.SpeechViseme, FigureRig.SpeechViseme>,
  Provides<typeof FigureSinging, Values<typeof InterfaceSinging>>,
  Provides<FigureSinging.LyricLine, InterfaceSinging.LyricLine>,
  Accepts<InterfaceSinging.LyricLine, FigureSinging.LyricLine>,
  Provides<FigureSinging.MotionAudioFeatures, InterfaceSinging.MotionAudioFeatures>,
  Accepts<InterfaceSinging.MotionAudioFeatures, FigureSinging.MotionAudioFeatures>,
  Provides<FigureSinging.MusicMotionSignal, InterfaceSinging.MusicMotionSignal>,
  Accepts<InterfaceSinging.MusicMotionSignal, FigureSinging.MusicMotionSignal>,
  Provides<FigureSinging.WordLyricLine, InterfaceSinging.WordLyricLine>,
  Accepts<InterfaceSinging.WordLyricLine, FigureSinging.WordLyricLine>,
  Provides<FigureSinging.WordLyricToken, InterfaceSinging.WordLyricToken>,
  Accepts<InterfaceSinging.WordLyricToken, FigureSinging.WordLyricToken>,
  Provides<typeof FigureSpeech, Values<typeof InterfaceSpeech>>,
  Provides<FigureSpeech.MeropeSpeechEventDetail, InterfaceSpeech.MeropeSpeechEventDetail>,
  Accepts<InterfaceSpeech.MeropeSpeechEventDetail, FigureSpeech.MeropeSpeechEventDetail>,
  Provides<FigureSpeech.MeropeSpeechSource, InterfaceSpeech.MeropeSpeechSource>,
  Accepts<InterfaceSpeech.MeropeSpeechSource, FigureSpeech.MeropeSpeechSource>,
  Provides<FigureSpeech.SpeechProsodyPlan, InterfaceSpeech.SpeechProsodyPlan>,
  Accepts<InterfaceSpeech.SpeechProsodyPlan, FigureSpeech.SpeechProsodyPlan>,
  Provides<FigureSpeech.SpeechProsodyTimeline, InterfaceSpeech.SpeechProsodyTimeline>,
  Accepts<InterfaceSpeech.SpeechProsodyTimeline, FigureSpeech.SpeechProsodyTimeline>,
  Provides<FigureSpeech.SpeechUtteranceInput, InterfaceSpeech.SpeechUtteranceInput>,
  Accepts<InterfaceSpeech.SpeechUtteranceInput, FigureSpeech.SpeechUtteranceInput>,
  Provides<FigureSpeech.TextVisemeCue, InterfaceSpeech.TextVisemeCue>,
  Accepts<InterfaceSpeech.TextVisemeCue, FigureSpeech.TextVisemeCue>,
  Provides<FigureSpeech.VisemeSpan, InterfaceSpeech.VisemeSpan>,
  Accepts<InterfaceSpeech.VisemeSpan, FigureSpeech.VisemeSpan>,
  Provides<typeof FigureWorkbench, Values<typeof InterfaceWorkbench>>,
  Provides<FigureWorkbench.Anime25DDebugSnapshot, InterfaceWorkbench.Anime25DDebugSnapshot>,
  Accepts<InterfaceWorkbench.Anime25DDebugSnapshot, FigureWorkbench.Anime25DDebugSnapshot>,
  Provides<FigureWorkbench.Anime25DDriver, InterfaceWorkbench.Anime25DDriver>,
  Accepts<InterfaceWorkbench.Anime25DDriver, FigureWorkbench.Anime25DDriver>,
  Provides<FigureWorkbench.Anime25DMotionEnvelopeProbeId, InterfaceWorkbench.Anime25DMotionEnvelopeProbeId>,
  Accepts<InterfaceWorkbench.Anime25DMotionEnvelopeProbeId, FigureWorkbench.Anime25DMotionEnvelopeProbeId>,
  Provides<FigureWorkbench.Anime25DPart, InterfaceWorkbench.Anime25DPart>,
  Accepts<InterfaceWorkbench.Anime25DPart, FigureWorkbench.Anime25DPart>,
  Provides<FigureWorkbench.Anime25DPartProfile, InterfaceWorkbench.Anime25DPartProfile>,
  Accepts<InterfaceWorkbench.Anime25DPartProfile, FigureWorkbench.Anime25DPartProfile>,
  Provides<FigureWorkbench.Anime25DPsdDefectKind, InterfaceWorkbench.Anime25DPsdDefectKind>,
  Accepts<InterfaceWorkbench.Anime25DPsdDefectKind, FigureWorkbench.Anime25DPsdDefectKind>,
  Provides<FigureWorkbench.Anime25DPsdReconciliation, InterfaceWorkbench.Anime25DPsdReconciliation>,
  Accepts<InterfaceWorkbench.Anime25DPsdReconciliation, FigureWorkbench.Anime25DPsdReconciliation>,
  Provides<FigureWorkbench.PoseCorrectionRegion, InterfaceWorkbench.PoseCorrectionRegion>,
  Accepts<InterfaceWorkbench.PoseCorrectionRegion, FigureWorkbench.PoseCorrectionRegion>,
  Provides<FigureWorkbench.PoseCorrectionStagePick, InterfaceWorkbench.PoseCorrectionStagePick>,
  Accepts<InterfaceWorkbench.PoseCorrectionStagePick, FigureWorkbench.PoseCorrectionStagePick>,
  Provides<FigureWorkbench.PoseCorrectionV7, InterfaceWorkbench.PoseCorrectionV7>,
  Accepts<InterfaceWorkbench.PoseCorrectionV7, FigureWorkbench.PoseCorrectionV7>,
]
