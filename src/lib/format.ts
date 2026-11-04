// Display formatting helpers shared by the live chart and the image export.

/** Formats a seconds offset for a time axis that can span a whole session.
 *  Chooses the unit based on the visible span: sub-minute windows keep
 *  decimals, longer windows collapse to m:ss / h:mm:ss. */
export function formatSeconds(seconds: number, spanSeconds: number): string {
  if (!Number.isFinite(seconds)) return ''
  if (seconds < 0) seconds = 0
  if (spanSeconds < 120) return `${seconds.toFixed(1)}s`
  const total = Math.round(seconds)
  if (spanSeconds < 3600) {
    const minutes = Math.floor(total / 60)
    const secs = total % 60
    return `${minutes}:${String(secs).padStart(2, '0')}`
  }
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const secs = total % 60
  return `${hours}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`
}

/** Converts a `#rrggbb` (or `#rgb`) color to an rgba() string at `alpha`. */
export function hexToRgba(hex: string, alpha: number): string {
  let normalized = hex.replace('#', '')
  if (normalized.length === 3) normalized = normalized.split('').map((c) => c + c).join('')
  const value = parseInt(normalized, 16)
  if (Number.isNaN(value) || normalized.length !== 6) return `rgba(0,0,0,${alpha})`
  const r = (value >> 16) & 0xff
  const g = (value >> 8) & 0xff
  const b = value & 0xff
  return `rgba(${r},${g},${b},${alpha})`
}

export function autoCurrentUnit(values: Array<number | null>): 'A' | 'mA' {
  const max = values.reduce<number>((largest, value) => Math.max(largest, Math.abs(value ?? 0)), 0)
  return max < 1 ? 'mA' : 'A'
}

export function autoPowerUnit(values: Array<number | null>): 'W' | 'mW' {
  const max = values.reduce<number>((largest, value) => Math.max(largest, Math.abs(value ?? 0)), 0)
  return max < 1 ? 'mW' : 'W'
}

export function formatCurrent(value: number | null | undefined, unit?: 'A' | 'mA'): string {
  if (value == null || !Number.isFinite(value)) return '—'
  const resolved = unit ?? (Math.abs(value) < 1 ? 'mA' : 'A')
  return `${(resolved === 'mA' ? value * 1000 : value).toFixed(resolved === 'mA' ? 1 : 3)} ${resolved}`
}

export function formatPower(value: number | null | undefined, unit?: 'W' | 'mW'): string {
  if (value == null || !Number.isFinite(value)) return '—'
  const resolved = unit ?? (Math.abs(value) < 1 ? 'mW' : 'W')
  return `${(resolved === 'mW' ? value * 1000 : value).toFixed(resolved === 'mW' ? 1 : 3)} ${resolved}`
}
