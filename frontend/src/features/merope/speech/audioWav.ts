export function pcmToWav(pcmData: Float32Array[], sampleRate: number): Blob {
  const totalLength = pcmData.reduce((acc, arr) => acc + arr.length, 0)
  const merged = new Float32Array(totalLength)
  let offset = 0
  for (const arr of pcmData) {
    merged.set(arr, offset)
    offset += arr.length
  }

  const buffer = new ArrayBuffer(44 + merged.length * 2)
  const view = new DataView(buffer)
  const writeString = (off: number, string: string) => {
    for (let i = 0; i < string.length; i++) {
      view.setUint8(off + i, string.charCodeAt(i))
    }
  }

  writeString(0, 'RIFF')
  view.setUint32(4, 36 + merged.length * 2, true)
  writeString(8, 'WAVE')
  writeString(12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true)
  view.setUint16(22, 1, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * 2, true)
  view.setUint16(32, 2, true)
  view.setUint16(34, 16, true)
  writeString(36, 'data')
  view.setUint32(40, merged.length * 2, true)

  const int16MinMagnitude = 32768
  const int16Max = 32767
  let dataOffset = 44
  for (let i = 0; i < merged.length; i++) {
    const sample = Math.max(-1, Math.min(1, merged[i]))
    view.setInt16(
      dataOffset,
      sample < 0 ? sample * int16MinMagnitude : sample * int16Max,
      true,
    )
    dataOffset += 2
  }

  return new Blob([buffer], { type: 'audio/wav' })
}

export function frameRms(frame: Float32Array): number {
  let sum = 0
  for (let i = 0; i < frame.length; i++) sum += frame[i]! * frame[i]!
  return Math.sqrt(sum / Math.max(1, frame.length))
}

export function isSubmittableTranscript(text: string): boolean {
  const letters = text.trim().replace(/[\s\p{P}\p{S}]/gu, '')
  return letters.length >= 2
}
