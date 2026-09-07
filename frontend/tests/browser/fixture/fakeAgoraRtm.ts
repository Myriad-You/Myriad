let currentListener: ((event: unknown) => void) | null = null

class FakeRtm {
  private listener: ((event: unknown) => void) | null = null
  async login(): Promise<void> {}
  async logout(): Promise<void> {
    if (currentListener === this.listener) currentListener = null
    this.listener = null
  }

  async subscribe(): Promise<void> {}
  async unsubscribe(): Promise<void> {}
  async publish(): Promise<void> {}
  addEventListener(event: string, listener: (event: unknown) => void): void {
    if (event === 'message') {
      this.listener = listener
      currentListener = listener
    }
  }

  removeEventListener(): void {}
  emit(event: unknown): void {
    this.listener?.(event)
  }
}

window.__fakeAgoraTranscript = (event: unknown) => currentListener?.(event)

export default { RTM: FakeRtm }
