import { useMemo } from 'react'
import { encode } from 'uqr'

export interface DeviceBindQrCodeProps {
  uri: string
  label: string
  size?: number
}

/**
 * Renders a crisp inline SVG QR code with a fixed white quiet zone so dark
 * themes never invert module contrast.
 */
export function DeviceBindQrCode({
  uri,
  label,
  size = 180,
}: DeviceBindQrCodeProps) {
  const { modules, path } = useMemo(() => {
    const qr = encode(uri, { ecc: 'M', border: 4 })
    const segments: string[] = []
    for (let y = 0; y < qr.size; y++) {
      const row = qr.data[y]
      for (let x = 0; x < qr.size; x++) {
        if (row[x]) {
          segments.push(`M${x} ${y}h1v1h-1z`)
        }
      }
    }
    return { modules: qr.size, path: segments.join('') }
  }, [uri])

  return (
    <div className="agent-device-qr-frame">
      <svg
        className="agent-device-qr-svg"
        viewBox={`0 0 ${modules} ${modules}`}
        width={size}
        height={size}
        role="img"
        aria-label={label}
        shapeRendering="crispEdges"
      >
        <rect width={modules} height={modules} fill="#ffffff" />
        <path d={path} fill="#000000" />
      </svg>
    </div>
  )
}

export default DeviceBindQrCode
