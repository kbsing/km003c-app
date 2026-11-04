<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { listen, type UnlistenFn } from '@tauri-apps/api/event'
  import { onDestroy, onMount } from 'svelte'
  import ToolHeader from './features/dashboard/ToolHeader.svelte'
  import DashboardMetrics from './features/dashboard/DashboardMetrics.svelte'
  import DashboardChart from './features/dashboard/DashboardChart.svelte'
  import DashboardDetailsDialog from './features/dashboard/DashboardDetailsDialog.svelte'
  import DeviceDialog from './features/dashboard/DeviceDialog.svelte'
  import PdEventLog from './features/pd/PdEventLog.svelte'
  import PowerProfiles from './features/pd/PowerProfiles.svelte'
  import SettingsDialog from './features/settings/SettingsDialog.svelte'
  import DataExportDialog from './lib/DataExportDialog.svelte'
  import { defaultRecordingOptions, type RecordingOptions, type RecordingStatus } from './features/recording/types'
  import { directionFor, type CurrentDirection } from './lib/measurement'
  import { createDownsampler } from './lib/downsample'
  import type { Batch, ChartEventMarker, ChartRange, ChartSelectionStats, CurveColors, LastSessionResult, Measurement, PdContract, PdEvent, PowerProfile, SessionStats, StartResult, Stats } from './types'

  const MAX_EVENTS = 300
  const CHART_REFRESH_MS = 120
  const REPLAY_FRAME_BUDGET_MS = 8
  const RECONNECT_INTERVAL_MS = 250
  const RECONNECT_WINDOW_MS = 3000
  const U32_RANGE_MS = 0x1_0000_0000
  const U32_HALF_RANGE_MS = 0x8000_0000
  const SETTINGS_KEY = 'km003c.settings.v1'
  const DEFAULT_CURVE_COLORS: CurveColors = { voltage: '#2f7795', current: '#a9652c', power: '#8c7040', temperature: '#b45309' }
  const MAX_PAUSED_BATCHES = 30_000
  // In-memory history is bounded: the full record lives in the NDJSON session
  // file on disk, so we only keep a rolling window for the live chart.
  const MAX_SESSION_POINTS = 120_000

  // Reused by every chart refresh. It keeps per-bucket min/max summaries and
  // only folds the newly appended points, so a long session stays O(delta +
  // bucket count) per refresh instead of re-scanning the whole history. Reset
  // when the bounded window trims old points (the incremental state is index-
  // based and cannot survive a splice).
  let downsample = createDownsampler()

  function clampNumber(value: unknown, min: number, max: number, fallback: number): number {
    if (typeof value !== 'number' || !Number.isFinite(value)) return fallback
    return Math.min(max, Math.max(min, Math.round(value)))
  }

  function emptySessionStats(): SessionStats {
    return {
      vbus: { min: Infinity, max: -Infinity, avg: 0 },
      ibus: { min: Infinity, max: -Infinity, avg: 0 },
      power: { min: Infinity, max: -Infinity, avg: 0 },
      count: 0,
    }
  }

  let connected = $state(false)
  let starting = $state(false)
  let reconnecting = $state(false)
  let paused = $state(false)
  let replaying = $state(false)
  /** Device model shown next to the connection light (product string). */
  let deviceModel = $state('')
  /** Brand label ("KM003C" / "WITRN" / "Power-Pico"); drives brand-specific UI. */
  let deviceKind = $state('')
  let error = $state('')
  let latest = $state<Measurement | null>(null)
  let stats = $state<Stats | null>(null)
  let showRaw = $state(false)
  let settingsOpen = $state(false)
  let dashboardDetailsOpen = $state(false)
  let dataExportOpen = $state(false)
  let dataExportKind = $state<'csv' | 'json'>('csv')
  let deviceDialogOpen = $state(false)
  let recordingOptions = $state<RecordingOptions>({ ...defaultRecordingOptions })
  let recordingStatus = $state<RecordingStatus | null>(null)
  let curveColors = $state<CurveColors>({ ...DEFAULT_CURVE_COLORS })
  // Power is useful in the default report view; the checkbox remains the
  // explicit way to hide it. A saved false preference still wins on reload.
  let showPower = $state(true)
  let showTemperature = $state(true)
  /** Hide the curve and power readout on the desktop; they remain on the
   *  LAN browser page. Useful when the desktop is only a capture head. */
  let hideCurvePower = $state(false)

  // Session lifecycle: before "开始" nothing but the top metrics is shown; the
  // chart/PD panels appear only while recording, and after "结束" the data can
  // be exported (export reads the NDJSON file on disk, not memory). On startup
  // the last recording file is restored so an unexpected close keeps the data.
  let sessionPhase = $state<'idle' | 'recording' | 'stopped' | 'restored'>('idle')
  /** Path of the current or last session recording (used for export). */
  let sessionPath = $state('')
  let showSession = $derived(sessionPhase === 'recording' || sessionPhase === 'stopped' || sessionPhase === 'restored')
  let canExport = $derived((sessionPhase === 'stopped' || sessionPhase === 'restored') && sessionPath !== '')

  function loadSettings() {
    try {
      const raw = localStorage.getItem(SETTINGS_KEY)
      if (!raw) return
      const saved = JSON.parse(raw) as {
        showRaw?: boolean
        recordingOptions?: Partial<RecordingOptions>
        curveColors?: Partial<CurveColors>
        showPower?: boolean
        showTemperature?: boolean
        hideCurvePower?: boolean
      }
      if (saved && typeof saved === 'object') {
        if (saved.showRaw !== undefined) showRaw = saved.showRaw
        if (saved.recordingOptions && typeof saved.recordingOptions === 'object') {
          const merged = { ...defaultRecordingOptions, ...saved.recordingOptions }
          recordingOptions = {
            ...merged,
            // Clamp saved values into the backend's validation ranges so a
            // corrupt entry cannot fail start_recording with a confusing error.
            current_threshold_ma: typeof merged.current_threshold_ma === 'number' && Number.isFinite(merged.current_threshold_ma) && merged.current_threshold_ma >= 0
              ? merged.current_threshold_ma
              : defaultRecordingOptions.current_threshold_ma,
            hold_seconds: clampNumber(merged.hold_seconds, 1, 86_400, defaultRecordingOptions.hold_seconds),
            max_buffer_mb: clampNumber(merged.max_buffer_mb, 1, 1024, defaultRecordingOptions.max_buffer_mb),
          }
        }
        curveColors = { ...DEFAULT_CURVE_COLORS, ...(saved.curveColors ?? {}) }
        if (saved.showPower !== undefined) showPower = saved.showPower === true
        if (saved.showTemperature !== undefined) showTemperature = saved.showTemperature === true
        hideCurvePower = saved.hideCurvePower === true
      }
    } catch {
      // ignore corrupt settings
    }
  }
  loadSettings()

  $effect(() => {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({ showRaw, recordingOptions, curveColors, showPower, showTemperature, hideCurvePower }))
  })

  // Full session history used for exports and downsampled chart rendering.
  // These arrays intentionally remain outside Svelte reactivity; the chart
  // receives a bounded rendering snapshot at a controlled refresh rate.
  let tSec: number[] = []
  let deviceMs: Array<number | null> = []
  let signedAmps: Array<number | null> = []
  let hostMs: Array<number | null> = []
  let volts: Array<number | null> = []
  let amps: Array<number | null> = []
  let temps: Array<number | null> = []
  let capacityMah = $state(0)
  let energyWh = $state(0)
  let uptimeMs = $state(0)
  let directions: Array<CurrentDirection | null> = []
  let chartTSec = $state<number[]>([])
  let chartVolts = $state<Array<number | null>>([])
  let chartAmps = $state<Array<number | null>>([])
  let chartTemps = $state<Array<number | null>>([])
  let chartEvents = $state<ChartEventMarker[]>([])
  let selectionStats = $state<ChartSelectionStats | null>(null)
  let chartRange = $state<ChartRange | null>(null)
  let viewHostRange = $state<{ start: number; end: number } | null>(null)
  let events = $state<PdEvent[]>([])
  let powerProfiles = $state<PowerProfile[]>([])
  let pdContract = $state<PdContract>({ phase: 'idle', visible_request: null, active_contract: null })
  let sessionStats = $state<SessionStats>(emptySessionStats())
  let workbenchEl: HTMLElement
  let workbenchHeight = $state(0)
  let workbenchObserver: ResizeObserver | null = null
  let temperatureAvailable = $derived(chartTemps.some((value) => value !== null && Number.isFinite(value) && value > 0))

  let t0: number | null = null
  let xOffsetSec = 0
  let lastDeviceMs: number | null = null
  let deviceWrapOffsetMs = 0
  let breakBeforeNextBatch = false
  let disconnectedAtMs: number | null = null
  let awaitingStartResult = false
  let unlisten: UnlistenFn[] = []
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  /** Device chosen in the picker; re-used when a dropped device reconnects. */
  let lastDeviceId = ''
  /** Wall-clock deadline for the auto-reconnect window; null when not reconnecting. */
  let reconnectDeadline: number | null = null
  let chartRefreshTimer: ReturnType<typeof setTimeout> | null = null
  let lastAccumHostMs: number | null = null
  let pausedBatches: Batch[] = []
  let replayQueue: Batch[] = []
  let replayIndex = 0
  let replayFrame: number | null = null

  onMount(async () => {
    workbenchObserver = new ResizeObserver(([entry]) => {
      const nextHeight = Math.ceil(entry?.contentRect.height ?? 0)
      if (nextHeight > 0) workbenchHeight = nextHeight
    })
    if (workbenchEl) workbenchObserver.observe(workbenchEl)
    unlisten.push(await listen<Batch>('batch', ({ payload }) => receiveBatch(payload)))
    unlisten.push(await listen<Stats>('acq-stopped', ({ payload }) => {
      stats = payload
      connected = false
      paused = false
      // Record the end time so a later manual reconnect inserts a real gap in
      // the chart instead of a 1 ms splice (device unplug sets this too).
      disconnectedAtMs = performance.now()
      discardPausedBatches()
    }))
    unlisten.push(await listen<string>('acq-error', ({ payload }) => {
      error = payload
      connected = false
      paused = false
      discardPausedBatches()
      if (payload === 'device disconnected') {
        disconnectedAtMs = performance.now()
        beginReconnect()
      }
    }))
    unlisten.push(await listen<RecordingStatus>('recording-stopped', ({ payload }) => {
      recordingStatus = payload
      // Recording ended on its own (device dropped / auto-stop condition): the
      // session is finished and its file is ready for export.
      if (sessionPhase === 'recording') sessionPhase = 'stopped'
    }))
    unlisten.push(await listen<string>('recording-error', ({ payload }) => {
      recordingStatus = null
      error = `记录保存失败：${payload}`
      if (sessionPhase === 'recording') sessionPhase = 'stopped'
    }))
    await loadLastSession()
  })

  onDestroy(() => {
    workbenchObserver?.disconnect()
    cancelReconnect()
    discardPausedBatches()
    if (chartRefreshTimer !== null) clearTimeout(chartRefreshTimer)
    unlisten.forEach((stopListening) => stopListening())
  })

  function receiveBatch(batch: Batch) {
    // A new acquisition thread can emit before start_acquisition resolves.
    if (awaitingStartResult) return

    // The top metrics always reflect the latest sample, but the curve / PD /
    // recording only start after the user clicks 开始.
    if (batch.measurements.length > 0) latest = batch.measurements[batch.measurements.length - 1]
    if (sessionPhase !== 'recording') return

    if (paused || replaying) {
      pausedBatches.push(batch)
      if (pausedBatches.length > MAX_PAUSED_BATCHES) {
        // Bound memory on long pauses: drop the oldest, keep the most recent.
        pausedBatches.splice(0, pausedBatches.length - MAX_PAUSED_BATCHES)
      }
      return
    }

    applyBatch(batch)
  }

  function applyBatch(batch: Batch) {
    if (batch.measurements.length > 0) appendMeasurements(batch.measurements)
    if (batch.events.length > 0) {
      events = batch.events.slice().reverse().concat(events).slice(0, MAX_EVENTS)
      const markers = batch.events.map(eventMarker).filter((marker): marker is ChartEventMarker => marker !== null)
      if (markers.length > 0) chartEvents = chartEvents.concat(markers).slice(-200)
    }
    powerProfiles = batch.power_profiles
    pdContract = batch.contract
    stats = batch.stats
  }

  function appendMeasurements(measurements: Measurement[]) {
    latest = measurements[measurements.length - 1]
    accumulate(measurements)
    const nextTimes = measurements.map((measurement) => chartTime(measurement.device_ms))
    const addBreak = breakBeforeNextBatch && tSec.length > 0 && nextTimes.length > 0
    const breakTime = addBreak ? Math.max(tSec[tSec.length - 1] + 0.001, nextTimes[0] - 0.001) : null
    if (addBreak) {
      // Every per-sample array must receive a placeholder: these arrays are
      // index-aligned, and a single missed push shifts hostMs off by one for
      // the rest of the session (event markers, range stats, export bounds).
      tSec.push(breakTime!)
      deviceMs.push(null)
      hostMs.push(null)
      signedAmps.push(null)
      volts.push(null)
      amps.push(null)
      temps.push(null)
      directions.push(null)
    }
    for (let index = 0; index < measurements.length; index += 1) {
      const measurement = measurements[index]
      recordStat(measurement)
      tSec.push(nextTimes[index])
      deviceMs.push(measurement.device_ms)
      hostMs.push(measurement.host_ms)
      signedAmps.push(measurement.ibus_a)
      volts.push(measurement.vbus_v)
      amps.push(Math.abs(measurement.ibus_a))
      temps.push(chartTemperature(measurement.temp_c))
      directions.push(directionFor(measurement.ibus_a))
    }
    breakBeforeNextBatch = false
    // Bound in-memory history to a rolling window (the full record is on disk
    // in the NDJSON session file). Dropping the head invalidates the
    // incremental downsampler's index-based state, so recreate it.
    const overflow = tSec.length - MAX_SESSION_POINTS
    if (overflow > 0) {
      tSec.splice(0, overflow)
      deviceMs.splice(0, overflow)
      hostMs.splice(0, overflow)
      signedAmps.splice(0, overflow)
      volts.splice(0, overflow)
      amps.splice(0, overflow)
      temps.splice(0, overflow)
      directions.splice(0, overflow)
      downsample = createDownsampler()
    }
    queueChartRefresh()
    if (chartRange) {
      updateSelectionStats(chartRange.start, chartRange.end)
      updateViewHostRange()
    }
  }

  function chartTemperature(value: number | null): number | null {
    if (value === null || !Number.isFinite(value)) return null
    return deviceKind === 'WITRN' && value <= 0 ? null : value
  }

  function eventMarker(event: PdEvent): ChartEventMarker | null {
    const important = event.kind !== 'message' || ['Source_Capabilities', 'Request', 'Accept', 'Reject', 'Wait', 'Soft_Reset', 'PS_RDY', 'PPS_Status', 'DR_Swap', 'PR_Swap', 'VCONN_Swap', 'FR_Swap'].includes(event.message?.standard_name ?? '')
    if (!important) return null
    const start = Math.max(0, deviceMs.lastIndexOf(null) + 1)
    let best = -1
    let distance = Number.POSITIVE_INFINITY
    for (let index = start; index < deviceMs.length; index += 1) {
      const value = deviceMs[index]
      if (value === null) continue
      const host = hostMs[index]
      const delta = host === null
        ? Math.abs(value - event.device_ms)
        : Math.abs(host - event.host_ms)
      if (delta < distance) { distance = delta; best = index }
    }
    if (best < 0 || !Number.isFinite(tSec[best])) return null
    return { t: tSec[best], label: event.message?.title ?? event.summary, kind: event.kind }
  }

  function updateSelectionStats(start: number, end: number) {
    const values = [] as Array<{ index: number; v: number; a: number; host: number }>
    for (let index = 0; index < tSec.length; index += 1) {
      const v = volts[index]
      const a = amps[index]
      const host = hostMs[index]
      if (tSec[index] >= start && tSec[index] <= end && v !== null && a !== null && host !== null) values.push({ index, v, a, host })
    }
    if (values.length === 0) { selectionStats = null; return }
    const min = (key: 'v' | 'a') => Math.min(...values.map((value) => value[key]))
    const max = (key: 'v' | 'a') => Math.max(...values.map((value) => value[key]))
    const avg = (key: 'v' | 'a') => values.reduce((sum, value) => sum + value[key], 0) / values.length
    const powers = values.map((value) => Math.abs(value.v * value.a))
    let capacity = 0
    let energy = 0
    for (let index = 1; index < values.length; index += 1) {
      // A chart break (disconnect gap) sits between the two samples; its dead
      // time must not be integrated, matching the main accumulators.
      if (values[index].index !== values[index - 1].index + 1) continue
      const dt = Math.max(0, values[index].host - values[index - 1].host) / 1000
      capacity += values[index - 1].a * dt * 1000 / 3600
      energy += powers[index - 1] * dt / 3600
    }
    selectionStats = {
      duration_s: Math.max(0, end - start),
      count: values.length,
      vbus: { min: min('v'), max: max('v'), avg: avg('v') },
      ibus: { min: min('a'), max: max('a'), avg: avg('a') },
      power: { min: Math.min(...powers), max: Math.max(...powers), avg: powers.reduce((sum, value) => sum + value, 0) / powers.length },
      capacity_mah: capacity,
      energy_wh: energy,
    }
  }

  function handleChartRange(range: ChartRange) {
    chartRange = range
    updateSelectionStats(range.start, range.end)
    updateViewHostRange()
  }

  function updateViewHostRange() {
    if (!chartRange || hostMs.length === 0) {
      viewHostRange = null
      return
    }
    let start: number | null = null
    let end: number | null = null
    for (let index = 0; index < hostMs.length; index += 1) {
      const host = hostMs[index]
      if (host === null || tSec[index] < chartRange.start || tSec[index] > chartRange.end) continue
      if (start === null) start = host
      end = host
    }
    viewHostRange = start === null || end === null ? null : { start, end }
  }

  /** Pure incremental min/max/avg fold, so bulk rebuilds (session restore)
   *  can accumulate locally and assign the $state once. */
  function foldStat(acc: SessionStats, measurement: Measurement): SessionStats {
    const v = measurement.vbus_v
    const a = Math.abs(measurement.ibus_a)
    const p = Math.abs(measurement.vbus_v * measurement.ibus_a)
    const n = acc.count
    return {
      vbus: {
        min: Math.min(acc.vbus.min, v),
        max: Math.max(acc.vbus.max, v),
        avg: n === 0 ? v : acc.vbus.avg + (v - acc.vbus.avg) / (n + 1),
      },
      ibus: {
        min: Math.min(acc.ibus.min, a),
        max: Math.max(acc.ibus.max, a),
        avg: n === 0 ? a : acc.ibus.avg + (a - acc.ibus.avg) / (n + 1),
      },
      power: {
        min: Math.min(acc.power.min, p),
        max: Math.max(acc.power.max, p),
        avg: n === 0 ? p : acc.power.avg + (p - acc.power.avg) / (n + 1),
      },
      count: n + 1,
    }
  }

  function recordStat(measurement: Measurement) {
    sessionStats = foldStat(sessionStats, measurement)
  }

  function queueChartRefresh() {
    if (chartRefreshTimer !== null) return
    chartRefreshTimer = setTimeout(() => {
      chartRefreshTimer = null
      const render = downsample(tSec, volts, amps, temps)
      chartTSec = render.tSec
      chartVolts = render.volts
      chartAmps = render.amps
      chartTemps = render.temps
    }, CHART_REFRESH_MS)
  }

  function accumulate(measurements: Measurement[]) {
    for (const measurement of measurements) {
      if (lastAccumHostMs !== null && measurement.host_ms >= lastAccumHostMs) {
        const dtSeconds = (measurement.host_ms - lastAccumHostMs) / 1000
        const absCurrent = Math.abs(measurement.ibus_a)
        capacityMah += absCurrent * dtSeconds * 1000 / 3600
        energyWh += Math.abs(measurement.vbus_v * measurement.ibus_a) * dtSeconds / 3600
        uptimeMs += measurement.host_ms - lastAccumHostMs
      }
      lastAccumHostMs = measurement.host_ms
    }
  }

  function cancelReconnect() {
    if (reconnectTimer !== null) clearTimeout(reconnectTimer)
    reconnectTimer = null
    reconnectDeadline = null
    reconnecting = false
  }

  function beginReconnect() {
    if (connected || starting || reconnecting) return
    reconnecting = true
    reconnectDeadline = performance.now() + RECONNECT_WINDOW_MS
    error = '设备已断开，正在尝试重新连接…'
    scheduleReconnect()
  }

  function scheduleReconnect() {
    if (!reconnecting || reconnectTimer !== null) return
    // Retry every 250 ms (4×/s) but give up once the 3 s window has elapsed
    // without the device coming back, so the connect button is restored and
    // the user can pick a device manually (including a different one).
    if (reconnectDeadline !== null && performance.now() > reconnectDeadline) {
      reconnectTimer = null
      reconnectDeadline = null
      reconnecting = false
      error = '设备已断开，自动重连未成功。请手动连接设备。'
      return
    }
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null
      void start(lastDeviceId, true)
    }, RECONNECT_INTERVAL_MS)
  }

  async function start(deviceId: string, isReconnect = false) {
    if (starting) return
    error = ''
    starting = true
    awaitingStartResult = true
    try {
      const id = deviceId || 'km003c'
      const result = await invoke<StartResult>('start_acquisition', { deviceId: id })
      if (tSec.length > 0) beginChartSegment()
      deviceModel = result.product
      deviceKind = result.kind
      connected = true
      cancelReconnect()
    } catch (reason) {
      if (isReconnect && reconnecting) {
        error = '设备已断开，正在尝试重新连接…'
        scheduleReconnect()
      } else {
        error = String(reason)
      }
    } finally {
      awaitingStartResult = false
      starting = false
    }
  }

  async function stop() {
    cancelReconnect()
    paused = false
    discardPausedBatches()
    if (!connected) {
      error = ''
      return
    }
    try {
      await invoke('stop_acquisition')
    } catch (reason) {
      error = String(reason)
    }
  }

  /** 开始 a new recording session: clear the display, then record to disk. */
  async function startSession() {
    if (!connected || sessionPhase === 'recording' || starting) return
    error = ''
    clearHistory()
    paused = false
    try {
      const status = await invoke<RecordingStatus>('start_recording', { options: recordingOptions })
      recordingStatus = status
      sessionPath = status.file_path
      sessionPhase = 'recording'
    } catch (reason) {
      error = String(reason)
      sessionPhase = 'idle'
    }
  }

  /** 结束 the session: finalize the recording file; export becomes available. */
  async function stopSession() {
    if (sessionPhase !== 'recording') return
    paused = false
    discardPausedBatches()
    try {
      const status = await invoke<RecordingStatus>('stop_recording')
      recordingStatus = status
      sessionPath = status.file_path
    } catch (reason) {
      error = String(reason)
    } finally {
      sessionPhase = 'stopped'
    }
  }

  /** Restore the previous run's data after an unexpected close. */
  async function loadLastSession() {
    try {
      const result = await invoke<LastSessionResult>('load_last_session')
      if (result.found && result.measurements.length > 0) {
        restoreSession(result)
        sessionPath = result.file_path
        sessionPhase = 'restored'
      }
    } catch {
      // no usable previous session; stay idle
    }
  }

  function restoreSession(result: LastSessionResult) {
    clearHistory()
    const ms = result.measurements
    if (ms.length === 0) return
    const base = ms[0].device_ms
    let wrapOffset = 0
    let last: number | null = null
    let acc = emptySessionStats()
    for (const m of ms) {
      if (last !== null && m.device_ms < last && last >= U32_HALF_RANGE_MS && m.device_ms < U32_HALF_RANGE_MS) {
        wrapOffset += U32_RANGE_MS
      }
      last = m.device_ms
      const t = (wrapOffset + m.device_ms - base) / 1000
      tSec.push(t)
      deviceMs.push(m.device_ms)
      hostMs.push(m.host_ms)
      signedAmps.push(m.ibus_a)
      volts.push(m.vbus_v)
      amps.push(Math.abs(m.ibus_a))
      temps.push(chartTemperature(m.temp_c))
      directions.push(directionFor(m.ibus_a))
      acc = foldStat(acc, m)
    }
    // Rebuild what a live session would have shown: min/max/avg plus the
    // capacity/energy/uptime accumulators (lastAccumHostMs is null after
    // clearHistory, so the first sample does not integrate).
    sessionStats = acc
    accumulate(ms)
    latest = ms[ms.length - 1]
    events = result.events.slice().reverse()
    chartEvents = result.events.map(eventMarker).filter((marker): marker is ChartEventMarker => marker !== null).slice(-200)
    stats = result.stats
    queueChartRefresh()
  }

  function togglePause() {
    if (!connected || reconnecting) return

    if (paused) {
      paused = false
      replayPausedBatches()
    } else {
      paused = true
    }
  }

  function replayPausedBatches() {
    if (paused || replaying || pausedBatches.length === 0) return
    replayQueue = pausedBatches
    pausedBatches = []
    replayIndex = 0
    replaying = true
    replayFrame = requestAnimationFrame(replayPendingBatches)
  }

  function replayPendingBatches() {
    replayFrame = null
    if (paused) {
      pausedBatches = replayQueue.slice(replayIndex).concat(pausedBatches)
      replayQueue = []
      replayIndex = 0
      replaying = false
      return
    }

    const deadline = performance.now() + REPLAY_FRAME_BUDGET_MS
    while (replayIndex < replayQueue.length && performance.now() < deadline) {
      applyBatch(replayQueue[replayIndex])
      replayIndex += 1
    }

    if (replayIndex >= replayQueue.length) {
      if (pausedBatches.length === 0) {
        replayQueue = []
        replayIndex = 0
        replaying = false
        return
      }
      replayQueue = pausedBatches
      pausedBatches = []
      replayIndex = 0
    }

    replayFrame = requestAnimationFrame(replayPendingBatches)
  }

  function discardPausedBatches() {
    if (replayFrame !== null) cancelAnimationFrame(replayFrame)
    replayFrame = null
    replaying = false
    pausedBatches = []
    replayQueue = []
    replayIndex = 0
  }

  function clearHistory() {
    t0 = null
    xOffsetSec = 0
    lastDeviceMs = null
    deviceWrapOffsetMs = 0
    breakBeforeNextBatch = false
    disconnectedAtMs = null
    tSec = []
    deviceMs = []
    hostMs = []
    signedAmps = []
    volts = []
    amps = []
    temps = []
    directions = []
    chartTSec = []
    chartVolts = []
    chartAmps = []
    chartTemps = []
    chartEvents = []
    selectionStats = null
    chartRange = null
    viewHostRange = null
    if (chartRefreshTimer !== null) {
      clearTimeout(chartRefreshTimer)
      chartRefreshTimer = null
    }
    events = []
    powerProfiles = []
    pdContract = { phase: 'idle', visible_request: null, active_contract: null }
    sessionStats = emptySessionStats()
    // Reset transport stats too, otherwise the chart title keeps showing the
    // pre-clear sample count until the next batch arrives.
    stats = null
    capacityMah = 0
    energyWh = 0
    uptimeMs = 0
    lastAccumHostMs = null
    sessionPhase = 'idle'
    sessionPath = ''
    downsample = createDownsampler()
    discardPausedBatches()
  }

  function resetAccumulators() {
    capacityMah = 0
    energyWh = 0
    uptimeMs = 0
    lastAccumHostMs = null
  }

  function exportCsv() {
    if (!canExport) return
    dataExportKind = 'csv'
    dataExportOpen = true
  }

  function exportDebug() {
    if (!canExport) return
    dataExportKind = 'json'
    dataExportOpen = true
  }

  function chartTime(nextDeviceMs: number): number {
    if (t0 === null) {
      t0 = nextDeviceMs
      lastDeviceMs = nextDeviceMs
      deviceWrapOffsetMs = 0
    } else if (lastDeviceMs !== null && nextDeviceMs < lastDeviceMs && lastDeviceMs >= U32_HALF_RANGE_MS && nextDeviceMs < U32_HALF_RANGE_MS) {
      deviceWrapOffsetMs += U32_RANGE_MS
    }
    lastDeviceMs = nextDeviceMs
    return xOffsetSec + (deviceWrapOffsetMs + nextDeviceMs - t0) / 1000
  }

  function beginChartSegment() {
    const lastTime = tSec[tSec.length - 1]
    const reconnectGapSec = disconnectedAtMs === null ? 0.001 : Math.max((performance.now() - disconnectedAtMs) / 1000, 0.001)
    xOffsetSec = lastTime + reconnectGapSec
    t0 = null
    lastDeviceMs = null
    deviceWrapOffsetMs = 0
    breakBeforeNextBatch = true
    disconnectedAtMs = null
    lastAccumHostMs = null
  }
</script>

<svelte:head><title>Star monitor</title></svelte:head>

<div class="app-shell">
  <ToolHeader
    {connected}
    {reconnecting}
    {starting}
    {paused}
    {replaying}
    {deviceModel}
    {sessionPhase}
    {canExport}
    onConnect={() => deviceDialogOpen = true}
    onTogglePause={togglePause}
    onStartSession={() => void startSession()}
    onStopSession={() => void stopSession()}
    onOpenSettings={() => settingsOpen = true}
    onOpenDetails={() => dashboardDetailsOpen = true}
    onClear={clearHistory}
    onExportCsv={exportCsv}
    onExportDebug={exportDebug}
    {stats}
  />

  <SettingsDialog open={settingsOpen} {curveColors} {recordingOptions} recordingStatus={recordingStatus} {hideCurvePower} onClose={() => settingsOpen = false} onCurveColorsChange={(colors) => curveColors = colors} onRecordingOptionsChange={(options) => recordingOptions = options} onToggleHideCurvePower={(checked) => hideCurvePower = checked} />
  <DataExportDialog open={dataExportOpen} kind={dataExportKind} sourcePath={sessionPath} {viewHostRange} onClose={() => dataExportOpen = false} />
  <DashboardDetailsDialog open={dashboardDetailsOpen} {connected} {reconnecting} {latest} {events} {capacityMah} {energyWh} {uptimeMs} {sessionStats} onDisconnect={() => void stop()} onReset={resetAccumulators} onClose={() => dashboardDetailsOpen = false} />
  <DeviceDialog open={deviceDialogOpen} onSelect={(deviceId) => { lastDeviceId = deviceId; deviceDialogOpen = false; void start(deviceId) }} onClose={() => deviceDialogOpen = false} />

  {#if error}<div class="notice" role="status">{error}</div>{/if}

  <main class="layout">
    <div class="top-grid">
      <section class="workbench" bind:this={workbenchEl}>
        <DashboardMetrics {latest} showWitrnExtras={deviceKind === 'WITRN'} hidePower={hideCurvePower} />
        <DashboardChart tSec={showSession ? chartTSec : []} volts={showSession ? chartVolts : []} amps={showSession ? chartAmps : []} temps={showSession ? chartTemps : []} samples={showSession ? sessionStats.count : 0} colors={curveColors} {showPower} {showTemperature} {temperatureAvailable} hideChart={hideCurvePower} hint={connected && !showSession ? '点击「开始」开始记录并显示曲线' : null} events={showSession ? chartEvents : []} selectionStats={showSession ? selectionStats : null} onRangeChange={handleChartRange} onTogglePower={(checked) => showPower = checked} onToggleTemperature={(checked) => showTemperature = checked} onOpenDetails={() => dashboardDetailsOpen = true} />
      </section>
      <aside class="inspector" style:height={workbenchHeight > 0 ? `${workbenchHeight}px` : undefined}><PdEventLog {events} {showRaw} onToggleRaw={(checked) => showRaw = checked} /></aside>
      </div>
    <PowerProfiles profiles={powerProfiles} contract={pdContract} />
  </main>

</div>

<style>
  .app-shell { min-height: 100vh; padding: 8px; display: flex; flex-direction: column; gap: 8px; background: #f4f4f5; }.notice { width: 100%; max-width: 1480px; margin: 0 auto; padding: 9px 12px; color: #b91c1c; border: 1px solid #fecaca; border-radius: 6px; background: #fff; font-size: 12px; }.layout { width: 100%; max-width: 1480px; margin: 0 auto; display: flex; flex-direction: column; gap: 8px; }.top-grid { display: grid; grid-template-columns: minmax(0, 2fr) minmax(390px, 1fr); gap: 8px; align-items: stretch; }.workbench { min-width: 0; display: flex; flex-direction: column; gap: 8px; }.inspector { min-width: 0; min-height: 0; }.inspector :global(.pd-panel) { height: 100%; min-height: 0; max-height: none; }
  @media (max-width: 1100px) { .top-grid { grid-template-columns: minmax(0, 1.7fr) minmax(330px, 1fr); } }
  @media (max-width: 820px) { .top-grid { display: flex; flex-direction: column; }.workbench, .inspector { width: 100%; }.inspector { height: 560px !important; } }
  @media (max-width: 600px) { .app-shell { padding: 6px; } }
</style>
