import type {
  MusicMotionAudio,
  MusicMotionClock,
  MusicMotionVisibility,
} from '@myriad/figure/motion'
import { getRigMotionCoordinator, MusicMotionSource } from '@myriad/figure/motion'
import { isPageVisible, onVisibility } from '../../../hooks/animation'
import { audioManager } from '../../../utils/musicPlayer'

const productionClock: MusicMotionClock = {
  now: () => performance.now(),
  raf: (callback) => requestAnimationFrame(callback),
  caf: (id) => cancelAnimationFrame(id),
}

const productionAudio: MusicMotionAudio = {
  getCurrentAudio: () => audioManager.getCurrentAudio(),
  getMotionAudioFeatures: (audio) =>
    audioManager.getMotionAudioFeatures(audio as HTMLAudioElement),
  connectAudioToAnalyser: (audio) =>
    audioManager.connectAudioToAnalyser(audio as HTMLAudioElement),
}

const productionVisibility: MusicMotionVisibility = {
  isPageVisible,
  onVisibility,
}

const runtime: { current: MusicMotionSource | null } = { current: null }

export function getMusicMotionSource(): MusicMotionSource {
  if (!runtime.current) {
    runtime.current = new MusicMotionSource(
      getRigMotionCoordinator(),
      productionClock,
      productionAudio,
      productionVisibility,
    )
  }
  return runtime.current
}
