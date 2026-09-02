declare module 'agora-rtc-sdk-ng' {
  export interface IAgoraRTCClient {
    join: (
      appId: string,
      channel: string,
      token: string,
      uid: number | string,
    ) => Promise<number | string>
    publish: (tracks: unknown[]) => Promise<void>
    subscribe: (user: IAgoraRTCRemoteUser, mediaType: 'audio' | 'video') => Promise<void>
    leave: () => Promise<void>
    on: (
      event: 'user-published' | 'user-unpublished',
      listener: (user: IAgoraRTCRemoteUser, mediaType?: 'audio' | 'video') => void,
    ) => void
  }
  export interface IAgoraRTCRemoteUser {
    uid: number | string
    audioTrack?: ILocalAudioTrack
  }
  export interface ILocalAudioTrack {
    play: () => void
    stop: () => void
    close: () => void
    getMediaStreamTrack: () => MediaStreamTrack
  }
  const AgoraRTC: {
    createClient: (config: { mode: string; codec: string }) => IAgoraRTCClient
    createMicrophoneAudioTrack: () => Promise<ILocalAudioTrack>
  }
  export default AgoraRTC
}
