// Incremental chart downsampling.
//
// The chart only ever renders a bounded snapshot (MAX_RENDER_POINTS) while
// App.svelte keeps the full raw history for exports. Growing the snapshot by
// re-scanning every raw point on every refresh would make a long session
// increasingly expensive (O(n) per 120 ms tick).
//
// Instead we keep persistent per-bucket min/max summaries. Each refresh only
// folds the newly appended points into the existing buckets, so the cost is
// O(delta + bucket count) instead of O(total). When the bucket size grows
// (ceil(n / MAX_BUCKETS) increments), the bucket boundaries move and the
// summaries are rebuilt with a single full scan -- this happens only a few
// times per session.
//
// The output shape is identical to the original one-shot version:
// per bucket the first point, the min/max of both series, the last null
// ("gap") index, and the last point, sorted and de-duplicated.

// Keep the raw session untouched, but cap the interactive canvas to a size
// that keeps pointer/tooltip work responsive on ordinary WebView2 hardware.
export const MAX_RENDER_POINTS = 6_000

const MAX_BUCKETS = Math.floor(MAX_RENDER_POINTS / 6)

export interface DownsampleResult {
  tSec: number[]
  volts: Array<number | null>
  amps: Array<number | null>
  temps: Array<number | null>
}

interface Extrema {
  value: number
  index: number
}

interface BucketAgg {
  minV: Extrema | null
  maxV: Extrema | null
  minA: Extrema | null
  maxA: Extrema | null
  minT: Extrema | null
  maxT: Extrema | null
  /** Index of the last null sample in the bucket, -1 when none. */
  gap: number
}

function emptyBucket(): BucketAgg {
  return { minV: null, maxV: null, minA: null, maxA: null, minT: null, maxT: null, gap: -1 }
}

function updateBucket(
  agg: BucketAgg,
  index: number,
  voltage: number | null,
  current: number | null,
  temperature: number | null,
) {
  if (voltage === null || current === null) {
    agg.gap = index
    return
  }
  if (agg.minV === null || voltage < agg.minV.value) agg.minV = { value: voltage, index }
  if (agg.maxV === null || voltage > agg.maxV.value) agg.maxV = { value: voltage, index }
  if (agg.minA === null || current < agg.minA.value) agg.minA = { value: current, index }
  if (agg.maxA === null || current > agg.maxA.value) agg.maxA = { value: current, index }
  if (temperature !== null && Number.isFinite(temperature)) {
    if (agg.minT === null || temperature < agg.minT.value) agg.minT = { value: temperature, index }
    if (agg.maxT === null || temperature > agg.maxT.value) agg.maxT = { value: temperature, index }
  }
}

function buildBuckets(
  volts: Array<number | null>,
  amps: Array<number | null>,
  temps: Array<number | null>,
  count: number,
  bucketSize: number,
): BucketAgg[] {
  const buckets: BucketAgg[] = []
  for (let index = 0; index < count; index += 1) {
    const slot = Math.floor(index / bucketSize)
    if (buckets[slot] === undefined) buckets[slot] = emptyBucket()
    updateBucket(buckets[slot], index, volts[index], amps[index], temps[index])
  }
  return buckets
}

function buildOutput(
  tSec: number[],
  volts: Array<number | null>,
  amps: Array<number | null>,
  temps: Array<number | null>,
  buckets: BucketAgg[],
  count: number,
  bucketSize: number,
): DownsampleResult {
  const outTime: number[] = []
  const outV: Array<number | null> = []
  const outA: Array<number | null> = []
  const outTemp: Array<number | null> = []
  for (let bucketIndex = 0; bucketIndex < buckets.length; bucketIndex += 1) {
    const agg = buckets[bucketIndex]
    const start = bucketIndex * bucketSize
    const end = Math.min(start + bucketSize, count)
    const indices = [start]
    if (agg.minV !== null) indices.push(agg.minV.index)
    if (agg.maxV !== null) indices.push(agg.maxV.index)
    if (agg.minA !== null) indices.push(agg.minA.index)
    if (agg.maxA !== null) indices.push(agg.maxA.index)
    if (agg.minT !== null) indices.push(agg.minT.index)
    if (agg.maxT !== null) indices.push(agg.maxT.index)
    if (agg.gap >= 0) indices.push(agg.gap)
    indices.push(end - 1)
    indices.sort((left, right) => left - right)
    let previous = -1
    for (const index of indices) {
      if (index === previous) continue
      outTime.push(tSec[index])
      outV.push(volts[index])
      outA.push(amps[index])
      outTemp.push(temps[index])
      previous = index
    }
  }
  return { tSec: outTime, volts: outV, amps: outA, temps: outTemp }
}

/**
 * Creates a stateful downsampler. Call it with the full raw arrays on every
 * refresh; it only re-processes the points appended since the previous call.
 * State is auto-reset when the input shrinks or falls back under the limit.
 */
export function createDownsampler() {
  let bucketSize = -1
  let foldedUpTo = 0
  let buckets: BucketAgg[] = []

  return function downsample(
    tSec: number[],
    volts: Array<number | null>,
    amps: Array<number | null>,
    temps: Array<number | null>,
  ): DownsampleResult {
    const count = tSec.length
    if (count <= MAX_RENDER_POINTS) {
      bucketSize = -1
      buckets = []
      foldedUpTo = count
      return { tSec: tSec.slice(), volts: volts.slice(), amps: amps.slice(), temps: temps.slice() }
    }

    const nextBucketSize = Math.ceil(count / MAX_BUCKETS)
    if (nextBucketSize !== bucketSize) {
      buckets = buildBuckets(volts, amps, temps, count, nextBucketSize)
      bucketSize = nextBucketSize
      foldedUpTo = count
    } else if (foldedUpTo < count) {
      for (let index = foldedUpTo; index < count; index += 1) {
        const slot = Math.floor(index / bucketSize)
        const agg = buckets[slot] ?? (buckets[slot] = emptyBucket())
        updateBucket(agg, index, volts[index], amps[index], temps[index])
      }
      foldedUpTo = count
    }

    return buildOutput(tSec, volts, amps, temps, buckets, count, bucketSize)
  }
}
