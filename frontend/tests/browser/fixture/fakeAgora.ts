type Published = (user: FakeUser, mediaType: 'audio') => void
type AudioPts = (pts: number) => void

interface FakeUser {
  uid: number
  audioTrack: FakeRemoteTrack
}

class FakeRemoteTrack {
  constructor(private readonly track: MediaStreamTrack) {}

  play(): void {}
  stop(): void {}

  getMediaStreamTrack(): MediaStreamTrack {
    return this.track
  }
}

let remoteContext: AudioContext | null = null
let remoteGain: GainNode | null = null
let audioPts: AudioPts | null = null
let ptsTimer: number | null = null
let currentPts = 1_000

function remoteUser(): FakeUser {
  remoteContext = new AudioContext({ sampleRate: 16_000 })
  const destination = remoteContext.createMediaStreamDestination()
  const oscillator = remoteContext.createOscillator()
  oscillator.frequency.value = 210
  remoteGain = remoteContext.createGain()
  remoteGain.gain.value = 0
  oscillator.connect(remoteGain).connect(destination)
  oscillator.start()
  void remoteContext.resume()
  return {
    uid: 8888,
    audioTrack: new FakeRemoteTrack(destination.stream.getAudioTracks()[0]!),
  }
}

window.__fakeAgoraVoice = (active: boolean) => {
  if (remoteGain) remoteGain.gain.value = active ? 0.2 : 0
  if (ptsTimer != null) window.clearInterval(ptsTimer)
  ptsTimer = active
    ? window.setInterval(() => {
        currentPts += 20
        audioPts?.(currentPts)
      }, 20)
    : null
}

export function setParameter(): void {}

const AgoraRTC = {
  setParameter,
  createClient() {
    let published: Published | null = null
    return {
      on(event: string, listener: Published | AudioPts) {
        if (event === 'user-published') published = listener as Published
        if (event === 'audio-pts') audioPts = listener as AudioPts
      },
      off(event: string, listener: Published | AudioPts) {
        if (event === 'user-published' && published === listener)
          published = null
        if (event === 'audio-pts' && audioPts === listener) audioPts = null
      },
      async join() {
        const user = remoteUser()
        window.setTimeout(() => published?.(user, 'audio'), 0)
      },
      async publish() {},
      async subscribe() {},
      removeAllListeners() {
        published = null
        audioPts = null
      },
      async leave() {
        if (ptsTimer != null) window.clearInterval(ptsTimer)
        ptsTimer = null
        currentPts = 1_000
        await remoteContext?.close().catch(() => {})
        remoteContext = null
        remoteGain = null
      },
    }
  },
  async createMicrophoneAudioTrack() {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    const track = stream.getAudioTracks()[0]!
    return {
      stop() {},
      close() {
        track.stop()
      },
      getMediaStreamTrack() {
        return track
      },
    }
  },
}

export default AgoraRTC
