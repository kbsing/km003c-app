import type { Measurement } from '../types'

export type CurrentDirection = 'positive' | 'negative' | 'zero'

export function directionFor(amps: number): CurrentDirection {
  if (amps > 0) return 'positive'
  if (amps < 0) return 'negative'
  return 'zero'
}

export function directionLabel(direction: CurrentDirection): string {
  if (direction === 'positive') return '+'
  if (direction === 'negative') return '-'
  return '0'
}

export function magnitudePower(measurement: Measurement | null): number {
  return measurement ? Math.abs(measurement.vbus_v * measurement.ibus_a) : 0
}
