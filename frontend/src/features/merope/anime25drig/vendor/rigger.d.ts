declare const Rigger: {
  buildRig: (
    psd: { width: number; height: number; children?: unknown[] },
    opts?: { generic?: unknown },
  ) => {
    canvas: { w: number; h: number }
    layers: Array<{
      name: string
      x: number
      y: number
      w: number
      h: number
      depth: number
      group: 'head' | 'body'
      phys: 'hair' | null
      fade: string | null
      side: 'L' | 'R' | null
      strands: Array<{ x: number; rootY: number; tipY: number }> | null
      img: { width: number; height: number; data: Uint8ClampedArray }
    }>
    anchors: {
      face: {
        cx: number
        cy: number
        x0: number
        x1: number
        y0: number
        y1: number
      }
    }
    warnings: string[]
  }
  cleanPsdLayers: (psd: unknown) => { noisy: number; layers: number }
  normName: (value: string) => string
  baseName: (value: string) => string
}

export default Rigger
