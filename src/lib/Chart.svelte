<script lang="ts">
  import type { EChartsType } from 'echarts/core'
  import { onMount } from 'svelte'
  import ChartNavigator from './ChartNavigator.svelte'
  import ChartExportDialog from './ChartExportDialog.svelte'
  import { loadChartDeps, type ChartDeps } from './chartLoader'
  import { autoCurrentUnit, autoPowerUnit, formatCurrent, formatPower, formatSeconds } from './format'
  import type { ChartEventMarker, ChartHover, ChartRange, CurveColors } from '../types'

  // tSec: seconds since first sample (device clock, not host clock).
  // volts/amps: same length as tSec.
  let {
    tSec = [] as number[],
    volts = [] as Array<number | null>,
    amps = [] as Array<number | null>,
    temps = [] as Array<number | null>,
    colors = { voltage: '#2f7795', current: '#a9652c', power: '#8c7040', temperature: '#b45309' } as CurveColors,
    showPower = false,
    showTemperature = false,
    events = [] as ChartEventMarker[],
    onHover = undefined as ((value: ChartHover | null) => void) | undefined,
    onRangeChange = undefined as ((value: ChartRange) => void) | undefined,
  }: {
    tSec: number[]
    volts: Array<number | null>
    amps: Array<number | null>
    temps?: Array<number | null>
    colors?: CurveColors
    showPower?: boolean
    showTemperature?: boolean
    events?: ChartEventMarker[]
    onHover?: (value: ChartHover | null) => void
    onRangeChange?: (value: ChartRange) => void
  } = $props()

  let container = $state<HTMLDivElement | null>(null)
  let chart = $state<EChartsType | null>(null)
  let hoverCard: HTMLDivElement | null = null
  let hoverCardValue = $state<ChartHover | null>(null)
  // Unit selection scans the complete retained sample set. Keep that work
  // tied to incoming data, never to pointer movement or hover-card updates.
  const currentUnit = $derived(autoCurrentUnit(amps))
  const powerValues = $derived(volts.map((v, i) => v === null || amps[i] === null ? null : v * amps[i]!))
  const powerUnit = $derived(autoPowerUnit(powerValues))
  // ECharts module, loaded lazily after first paint. The chart initializes as
  // soon as it arrives; the UI is already interactive before then.
  let deps = $state<ChartDeps | null>(null)
  let userZoomed = false
  let viewStart = $state(0)
  let viewEnd = $state(1)
  let exportOpen = $state(false)
  let compactAxes = $state(false)
  let hoverFrame: number | null = null
  let pendingHover: ChartHover | null | undefined
  let pendingPointer: { x: number; y: number } | null = null
  let lastHoverKey = ''

  // Smallest x window the wheel can zoom into. Large enough to keep a
  // meaningful number of points on screen (0.2 s ≈ 10 samples at 50 Hz) but
  // small enough that short test sessions still have room to zoom in. A bigger
  // floor makes zooming feel dead on fresh sessions.
  const MIN_ZOOM_WINDOW = 0.2

  // Current visible x window in chart-time seconds. Owned here; the x axis
  // always gets an explicit min/max (data extent while following, the zoomed
  // window after a wheel/navigator zoom), so ECharts never has to auto-clear
  // an old range.
  let viewMinT = $state(0)
  let viewMaxT = $state(1)
  let reportedRange: ChartRange | null = null

  // Wheel gesture coalescing: precision wheels / touchpads fire many small
  // wheel events per notch; stepping per event would compound the zoom factor
  // and jump straight to an extreme. Accumulate deltas and apply ONE step per
  // notch's worth of delta (a physical notch is ~±100).
  let wheelDelta = 0
  let wheelCursorX = 0
  let wheelFlushTimer: ReturnType<typeof setTimeout> | null = null

  function reportRange(range: ChartRange) {
    if (reportedRange && reportedRange.start === range.start && reportedRange.end === range.end && reportedRange.following === range.following) return
    reportedRange = range
    onRangeChange?.(range)
  }

  function currentSpan() {
    return Math.max(viewMaxT - viewMinT, 0.001)
  }

  function syncNavigator() {
    if (tSec.length < 2) return
    const first = tSec[0]
    const span = Math.max(tSec[tSec.length - 1] - first, 0.001)
    viewStart = Math.min(Math.max((viewMinT - first) / span, 0), 1)
    viewEnd = Math.min(Math.max((viewMaxT - first) / span, 0), 1)
  }

  function setVisibleRange(start: number, end: number) {
    const inst = chart
    if (!inst || tSec.length < 2) return
    const first = tSec[0]
    const span = tSec[tSec.length - 1] - first
    viewStart = start
    viewEnd = end
    userZoomed = start > 0 || end < 1
    viewMinT = first + span * start
    viewMaxT = first + span * end
    inst.setOption({ xAxis: { min: viewMinT, max: viewMaxT } })
    reportRange({ start: viewMinT, end: viewMaxT, following: !userZoomed })
  }

  function onWheel(event: WheelEvent) {
    if (!chart || tSec.length < 2) return
    event.preventDefault()
    wheelDelta += event.deltaY
    wheelCursorX = event.clientX
    if (Math.abs(wheelDelta) >= 100) {
      const delta = wheelDelta
      wheelDelta = 0
      zoomStep(delta)
    }
    if (wheelFlushTimer === null) {
      wheelFlushTimer = window.setTimeout(() => {
        wheelFlushTimer = null
        if (wheelDelta !== 0) {
          const delta = wheelDelta
          wheelDelta = 0
          zoomStep(delta)
        }
      }, 120)
    }
  }

  function zoomStep(delta: number) {
    const inst = chart
    if (!inst || !container || tSec.length < 2) return
    const first = tSec[0]
    const last = tSec[tSec.length - 1]
    const fullSpan = Math.max(last - first, MIN_ZOOM_WINDOW)
    const min = userZoomed ? viewMinT : first
    const max = userZoomed ? viewMaxT : last
    const range = max - min
    if (!Number.isFinite(range) || range <= 0) return
    const factor = delta < 0 ? 0.8 : 1.25
    const rect = container.getBoundingClientRect()
    const ratio = rect.width > 0 ? Math.min(Math.max((wheelCursorX - rect.left) / rect.width, 0), 1) : 0.5
    const center = min + range * ratio
    const nextRange = Math.min(Math.max(range * factor, MIN_ZOOM_WINDOW), fullSpan)
    const nextMin = Math.max(first, center - nextRange * ratio)
    const nextMax = Math.min(last, nextMin + nextRange)
    if (nextMax - nextMin < MIN_ZOOM_WINDOW) return
    userZoomed = true
    viewMinT = nextMin
    viewMaxT = nextMax
    inst.setOption({ xAxis: { min: nextMin, max: nextMax } })
    syncNavigator()
    reportRange({ start: nextMin, end: nextMax, following: false })
  }

  export function showAll() {
    if (!chart || tSec.length < 2) return
    setVisibleRange(0, 1)
  }

  export function followLatest() {
    userZoomed = false
    if (tSec.length > 0) {
      viewMinT = tSec[0]
      viewMaxT = tSec[tSec.length - 1]
    } else {
      viewMinT = 0
      viewMaxT = 1
    }
    viewStart = 0
    viewEnd = 1
    chart?.setOption({ xAxis: { min: viewMinT, max: viewMaxT } })
    reportRange({ start: viewMinT, end: viewMaxT, following: true })
  }

  export function openExport() {
    exportOpen = true
  }

  function requestHoverFrame() {
    if (hoverFrame === null) hoverFrame = window.requestAnimationFrame(flushHover)
  }

  function positionHoverCard() {
    const root = container
    const card = hoverCard
    const pointer = pendingPointer
    if (!root || !card || card.hidden || !pointer) return

    const width = card.offsetWidth
    const height = card.offsetHeight
    const maxX = Math.max(root.clientWidth - 8, 8)
    const maxY = Math.max(root.clientHeight - 8, 8)
    let x = pointer.x + 14
    let y = pointer.y - 18
    const leftSide = x + width > maxX
    if (leftSide) x = pointer.x - width - 14
    if (y < 8) y = pointer.y + 16
    else if (y + height > maxY) y = pointer.y - height - 16

    card.classList.toggle('from-left', leftSide)
    card.style.transform = `translate3d(${Math.round(x)}px, ${Math.round(y)}px, 0)`
  }

  function flushHover() {
    hoverFrame = null
    positionHoverCard()
    if (pendingHover === undefined) return
    const next = pendingHover
    pendingHover = undefined
    const key = next === null ? 'leave' : `${next.t}:${next.v}:${next.a}:${next.temp}`
    if (key === lastHoverKey) return
    lastHoverKey = key
    hoverCardValue = next
    if (hoverCard) hoverCard.hidden = next === null
    onHover?.(next)
  }

  function scheduleHover(next: ChartHover | null) {
    pendingHover = next
    requestHoverFrame()
  }

  function handlePointerMove(event: PointerEvent) {
    const root = container
    if (!root) return
    const rect = root.getBoundingClientRect()
    pendingPointer = {
      x: Math.min(Math.max(event.clientX - rect.left, 0), rect.width),
      y: Math.min(Math.max(event.clientY - rect.top, 0), rect.height),
    }
    requestHoverFrame()
  }

  function handleHover(raw: unknown) {
    const params = raw as { axesInfo?: Array<{ value?: number }> } | undefined
    const t = params?.axesInfo?.[0]?.value
    if (t == null || tSec.length === 0) {
      scheduleHover(null)
      return
    }
    // The updateAxisPointer payload only carries the hovered x value; find the
    // nearest sample in our own arrays for the readout.
    let lo = 0
    let hi = tSec.length - 1
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (tSec[mid] < t) lo = mid + 1
      else hi = mid
    }
    let idx = lo
    if (idx > 0 && Math.abs(tSec[idx - 1] - t) <= Math.abs(tSec[idx] - t)) idx -= 1
    const pointer = pendingPointer
    scheduleHover({
      t,
      v: volts[idx] ?? null,
      a: amps[idx] ?? null,
      temp: temps[idx] ?? null,
      span: currentSpan(),
      x: pointer?.x ?? 0,
      y: pointer?.y ?? 0,
    })
  }

  function handleLeave() {
    scheduleHover(null)
  }

  onMount(() => {
    let alive = true
    loadChartDeps().then((mod) => {
      if (alive) deps = mod
    })
    return () => {
      alive = false
    }
  })

  // Create once the container exists (and ECharts has loaded); tear down on
  // unmount.
  //
  // Deliberately seeds with EMPTY arrays. Reading tSec/volts/amps here would
  // make this effect track them, so every incoming batch (~12/s) would dispose
  // and rebuild the chart instead of updating it. The setOption effect below
  // owns all data flow. Reading showPower/colors here is deliberate: toggling
  // the power series or changing a color needs a fresh chart (series/axes are
  // fixed at creation time). These change rarely, unlike the data arrays.
  $effect(() => {
    const mod = deps
    const root = container
    if (!mod || !root) return

    const rect = root.getBoundingClientRect()
    const inst = mod.echarts.init(root, null, {
      width: Math.max(rect.width, 200),
      height: Math.max(rect.height, 150),
    })
    chart = inst

    // Keep the creation effect independent of responsive axis state. Crossing
    // the compact-layout threshold should update options, not recreate ECharts.
    const empty = mod.buildChartOption({ tSec: [], volts: [], amps: [], temps: [], showPower, showTemperature, compactAxes: false, colors, theme: 'light', events: [] })
    inst.setOption({ ...empty, tooltip: mod.buildTooltip(mod.CHART_PALETTES.light) })

    let resizeFrame: number | null = null
    const resizeNow = () => {
      resizeFrame = null
      const rect = root.getBoundingClientRect()
      const width = Math.max(Math.floor(rect.width || root.clientWidth), 200)
      const height = Math.max(Math.floor(rect.height || root.clientHeight), 150)
      compactAxes = width < 720
      // The chart is initialized with explicit dimensions for the first paint.
      // Passing the observed dimensions here is important on WebView2: a bare
      // resize() can keep the init-time width after a window maximize/restore.
      // Resize animation is disabled so the canvas reaches the container size
      // in the same layout pass instead of visibly easing toward it.
      inst.resize({ width, height, animation: { duration: 0 }, silent: true })
    }
    const scheduleResize = () => {
      if (resizeFrame === null) resizeFrame = window.requestAnimationFrame(resizeNow)
    }
    const ro = new ResizeObserver(scheduleResize)
    ro.observe(root)
    window.addEventListener('resize', scheduleResize)
    resizeNow()

    // Wheel zoom. Attached imperatively with { passive: false } so the handler
    // can always preventDefault (stop the page from scrolling while zooming).
    root.addEventListener('wheel', onWheel, { passive: false })
    root.addEventListener('pointermove', handlePointerMove, { passive: true, capture: true })

    inst.on('updateAxisPointer', handleHover)
    const zr = inst.getZr()
    zr.on('globalout', handleLeave)

    return () => {
      ro.disconnect()
      window.removeEventListener('resize', scheduleResize)
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame)
      if (hoverFrame !== null) window.cancelAnimationFrame(hoverFrame)
      resizeFrame = null
      hoverFrame = null
      pendingHover = undefined
      pendingPointer = null
      root.removeEventListener('wheel', onWheel)
      root.removeEventListener('pointermove', handlePointerMove, true)
      if (wheelFlushTimer !== null) clearTimeout(wheelFlushTimer)
      wheelFlushTimer = null
      zr.off('globalout', handleLeave)
      inst.off('updateAxisPointer', handleHover)
      inst.dispose()
      chart = null
      onHover?.(null)
    }
  })

  // Push rendering snapshots whenever they change. Until the user zooms, the
  // X axis follows the complete retained session; after zooming it stays put.
  $effect(() => {
    const inst = chart
    const mod = deps
    if (!inst || !mod) return
    const first = tSec.length > 0 ? tSec[0] : undefined
    const last = tSec.length > 0 ? tSec[tSec.length - 1] : undefined
    const option = mod.buildChartOption({
      tSec,
      volts,
      amps,
      temps,
      showPower,
      showTemperature,
      compactAxes,
      colors,
      theme: 'light',
      events,
      xMin: userZoomed ? viewMinT : first,
      xMax: userZoomed ? viewMaxT : last,
    })
    // Replace series/axes as a unit when the optional power curve is toggled.
    // Plain merge can retain an old series shape in some ECharts builds.
    inst.setOption(option, { replaceMerge: ['series', 'yAxis'], lazyUpdate: true, silent: true })
    if (!userZoomed) {
      viewStart = 0
      viewEnd = 1
      viewMinT = first ?? 0
      viewMaxT = last ?? 1
      reportRange({ start: viewMinT, end: viewMaxT, following: true })
    } else {
      syncNavigator()
    }
  })
</script>

<div class="chart-shell">
  {#if !deps}<div class="chart-loading">正在加载图表…</div>{/if}
  <div class="chart" bind:this={container}></div>
  <div class="hover-card" bind:this={hoverCard} hidden>
    {#if hoverCardValue}
      <div class="hover-time">{formatSeconds(hoverCardValue.t, hoverCardValue.span)}</div>
      <div class="hover-row"><i style={`background:${colors.voltage}`}></i><span>电压</span><b>{hoverCardValue.v === null ? '—' : `${hoverCardValue.v.toFixed(3)} V`}</b></div>
      <div class="hover-row"><i style={`background:${colors.current}`}></i><span>电流</span><b>{formatCurrent(hoverCardValue.a, currentUnit)}</b></div>
      {#if showPower}<div class="hover-row"><i style={`background:${colors.power}`}></i><span>功率</span><b>{hoverCardValue.v === null || hoverCardValue.a === null ? '—' : formatPower(hoverCardValue.v * hoverCardValue.a, powerUnit)}</b></div>{/if}
      {#if showTemperature}<div class="hover-row"><i style={`background:${colors.temperature}`}></i><span>温度</span><b>{hoverCardValue.temp === null ? '—' : `${hoverCardValue.temp.toFixed(1)} °C`}</b></div>{/if}
    {/if}
  </div>
  <ChartNavigator {tSec} {volts} start={viewStart} end={viewEnd} onChange={setVisibleRange} />
  <ChartExportDialog open={exportOpen} {tSec} {volts} {amps} viewRange={{ start: viewMinT, end: viewMaxT }} onClose={() => exportOpen = false} />
</div>

<style>
  .chart {
    width: 100%;
    height: 100%;
    /* ECharts creates a canvas with an intrinsic width. Without an explicit
       zero minimum, CSS grid can keep this item at its old maximized width
       when the window is restored to a narrower size. */
    min-width: 0;
    min-height: 220px;
  }
  .chart-shell { position: relative; display: grid; grid-template-rows: minmax(220px, 1fr) 52px; gap: 7px; width: 100%; height: 100%; min-width: 0; min-height: 279px; }
  .chart-loading { position: absolute; inset: 0 0 52px 0; display: grid; place-items: center; z-index: 2; color: #94a3b8; background: #fafafa; font-size: 11px; }
  .hover-card { position: absolute; top: 0; left: 0; z-index: 3; min-width: 116px; padding: 7px 9px; border: 1px solid #cbd5e1; border-radius: 4px; color: #334155; background: rgb(255 255 255 / 96%); box-shadow: 0 4px 13px rgb(15 23 42 / 13%); pointer-events: none; transform: translate3d(-9999px, -9999px, 0); will-change: transform; transition: transform 48ms cubic-bezier(.18, .88, .22, 1), opacity 80ms ease; }
  .hover-card::before { content: ''; position: absolute; top: 13px; left: -5px; width: 8px; height: 8px; border-bottom: 1px solid #cbd5e1; border-left: 1px solid #cbd5e1; background: rgb(255 255 255 / 96%); transform: rotate(45deg); }
  .hover-card:global(.from-left)::before { right: -5px; left: auto; border: 0; border-top: 1px solid #cbd5e1; border-right: 1px solid #cbd5e1; transform: rotate(45deg); }
  .hover-time { margin-bottom: 4px; color: #0f172a; font: 600 11px ui-monospace, "Cascadia Mono", Consolas, monospace; }
  .hover-row { display: grid; grid-template-columns: 8px auto auto; align-items: center; gap: 5px; min-height: 16px; font: 10px ui-monospace, "Cascadia Mono", Consolas, monospace; white-space: nowrap; }
  .hover-row i { width: 7px; height: 7px; border-radius: 50%; }
  .hover-row b { color: #0f172a; font-weight: 600; text-align: right; }
</style>
