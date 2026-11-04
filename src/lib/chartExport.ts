// High-resolution chart export.
//
// The exported PNG is NOT a screenshot of the live dashboard: it uses its own
// report-style ECharts option (buildExportChartOption) — deep-contrast colors,
// a title, an explicit legend and labeled axes, and a stats strip
// (Duration / Vavg / Vmax / Imax / Pmax) — so it reads like a USB PD
// test-instrument waveform report.
//
// Rendering still happens off-screen at the requested size (so the PNG
// resolution is independent of the display), ECharts is loaded on demand, and
// the caller (Rust) still saves the PNG.
import type { ComposeOption } from 'echarts/core'
import type { LineSeriesOption } from 'echarts/charts'
import type {
  GridComponentOption,
  LegendComponentOption,
  TitleComponentOption,
  TooltipComponentOption,
} from 'echarts/components'
import { loadChartDeps } from './chartLoader'

export type ExportTheme = 'light' | 'dark'

export interface ChartExportParams {
  tSec: number[]
  volts: Array<number | null>
  amps: Array<number | null>
  theme: ExportTheme
  width: number
  height: number
  /** Report title shown at the top; empty falls back to "Star monitor". */
  title: string
}

export type ExportChartOption = ComposeOption<
  | LineSeriesOption
  | GridComponentOption
  | TitleComponentOption
  | LegendComponentOption
  | TooltipComponentOption
>

// Export-only palette: high-contrast colors that stay readable on a white
// background, deliberately different from the live dashboard colors.
const REPORT_PALETTES: Record<ExportTheme, {
  background: string
  text: string
  muted: string
  axis: string
  grid: string
  voltage: string
  current: string
  power: string
}> = {
  light: {
    background: '#ffffff',
    text: '#111827',
    muted: '#475569',
    axis: '#6b7280',
    grid: '#e5e7eb',
    voltage: '#0b3d91', // deep blue
    current: '#e8590c', // orange
    power: '#c92a2a',   // red
  },
  dark: {
    background: '#0f172a',
    text: '#f1f5f9',
    muted: '#94a3b8',
    axis: '#94a3b8',
    grid: '#334155',
    voltage: '#60a5fa',
    current: '#fb923c',
    power: '#f87171',
  },
}

const MONO = 'ui-monospace, "Cascadia Mono", Consolas, monospace'

function pairs(values: Array<number | null>, tSec: number[]): Array<[number, number | null]> {
  const out: Array<[number, number | null]> = new Array(values.length)
  for (let i = 0; i < values.length; i += 1) {
    const v = values[i]
    out[i] = v === null || v === undefined ? [tSec[i], null] : [tSec[i], v]
  }
  return out
}

/** Compact x-axis labels: "61s" for short runs, "m:ss" beyond. */
function formatAxisSeconds(value: number, duration: number): string {
  const v = Math.round(value)
  if (duration < 120) return `${v}s`
  const m = Math.floor(v / 60)
  const s = v % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

/** Report footer duration: "61s", "1m 5s", "1h 2m". */
function formatDuration(seconds: number): string {
  const total = Math.round(seconds)
  if (total < 60) return `${total}s`
  if (total < 3600) {
    const m = Math.floor(total / 60)
    const s = total % 60
    return s === 0 ? `${m}m` : `${m}m ${s}s`
  }
  const h = Math.floor(total / 3600)
  const m = Math.round((total % 3600) / 60)
  return h > 0 && m > 0 ? `${h}h ${m}m` : `${h}h`
}

export interface ExportChartData {
  tSec: number[]
  volts: Array<number | null>
  amps: Array<number | null>
  theme: ExportTheme
  title: string
}

/** Export-only ECharts option: professional waveform-report layout. */
export function buildExportChartOption(data: ExportChartData): ExportChartOption {
  const { tSec, volts, amps, theme, title } = data
  const p = REPORT_PALETTES[theme]

  // Power is always part of the report, independent of the live power toggle.
  const power = volts.map((v, i) => (v === null || amps[i] === null ? null : v * amps[i]!))

  const duration = tSec.length >= 2 ? Math.max(tSec[tSec.length - 1] - tSec[0], 0) : 0
  // Stats are computed over exactly the arrays being plotted (possibly
  // view-filtered, downsampled display data), so the numbers always match the
  // curve in the image rather than the live session stats.
  let sumVolts = 0
  let countVolts = 0
  let maxVolts = 0
  let maxAmps = 0
  let maxPower = 0
  for (let i = 0; i < volts.length; i += 1) {
    const v = volts[i]
    if (v !== null && v !== undefined) {
      sumVolts += v
      countVolts += 1
      if (Math.abs(v) > maxVolts) maxVolts = Math.abs(v)
      const a = amps[i]
      if (a !== null && a !== undefined) {
        const p = Math.abs(v * a)
        if (p > maxPower) maxPower = p
      }
    }
    const a = amps[i]
    if (a !== null && a !== undefined && Math.abs(a) > maxAmps) maxAmps = Math.abs(a)
  }
  const avgVolts = countVolts > 0 ? sumVolts / countVolts : 0

  const axisLabel = (color: string, formatter: (v: number) => string) => ({
    color,
    fontSize: 12,
    fontFamily: MONO,
    formatter,
  })

  return {
    animation: false,
    backgroundColor: p.background,
    title: [
      {
        text: title || 'Star monitor',
        left: 90,
        top: 22,
        textStyle: { color: p.text, fontSize: 22, fontWeight: 'bold' },
      },
      {
        // Stats strip: label/value rich text so the numbers stand out from
        // the muted labels at a glance.
        text: `{label|Duration} {value|${formatDuration(duration)}}`
          + `   {label|Vavg} {value|${avgVolts.toFixed(2)}V}`
          + `   {label|Vmax} {value|${maxVolts.toFixed(2)}V}`
          + `   {label|Imax} {value|${maxAmps.toFixed(2)}A}`
          + `   {label|Pmax} {value|${maxPower.toFixed(1)}W}`,
        left: 90,
        bottom: 20,
        textStyle: {
          color: p.text,
          fontSize: 13,
          fontFamily: MONO,
          rich: {
            label: { color: p.muted, fontSize: 12 },
            value: { color: p.text, fontSize: 15, fontWeight: 'bold' },
          },
        },
      },
    ],
    legend: {
      data: ['Voltage', 'Current', 'Power'],
      right: 132,
      top: 28,
      icon: 'roundRect',
      itemWidth: 18,
      itemHeight: 10,
      itemGap: 22,
      textStyle: { color: p.text, fontSize: 13 },
    },
    grid: { left: 90, right: 120, top: 120, bottom: 80 },
    xAxis: {
      type: 'value',
      axisLabel: {
        color: p.axis,
        fontSize: 12,
        fontFamily: MONO,
        formatter: (v: number) => formatAxisSeconds(v, duration),
      },
      axisLine: { lineStyle: { color: p.axis } },
      axisTick: { show: false },
      splitLine: { show: false },
    },
    yAxis: [
      {
        type: 'value',
        name: 'Voltage (V)',
        // Rotated names run along each axis, so the two right-axis names never
        // collide the way horizontal 'end' names did.
        nameLocation: 'middle',
        nameGap: 36,
        nameTextStyle: { color: p.voltage, fontSize: 14, fontWeight: 'bold' },
        position: 'left',
        axisLabel: axisLabel(p.voltage, (v) => v.toFixed(2)),
        axisLine: { lineStyle: { color: p.voltage } },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: p.grid } },
      },
      {
        type: 'value',
        name: 'Current (A)',
        nameLocation: 'middle',
        nameGap: 24,
        nameTextStyle: { color: p.current, fontSize: 14, fontWeight: 'bold' },
        position: 'right',
        axisLabel: axisLabel(p.current, (v) => v.toFixed(2)),
        axisLine: { lineStyle: { color: p.current } },
        axisTick: { show: false },
        splitLine: { show: false },
      },
      {
        type: 'value',
        name: 'Power (W)',
        nameLocation: 'middle',
        nameGap: 24,
        nameTextStyle: { color: p.power, fontSize: 14, fontWeight: 'bold' },
        position: 'right',
        offset: 48,
        axisLabel: axisLabel(p.power, (v) => v.toFixed(1)),
        axisLine: { lineStyle: { color: p.power } },
        axisTick: { show: false },
        splitLine: { show: false },
      },
    ],
    series: [
      {
        name: 'Voltage',
        type: 'line',
        yAxisIndex: 0,
        data: pairs(volts, tSec),
        symbol: 'none',
        smooth: false,
        lineStyle: { width: 2, color: p.voltage },
        z: 3,
      },
      {
        name: 'Current',
        type: 'line',
        yAxisIndex: 1,
        data: pairs(amps, tSec),
        symbol: 'none',
        smooth: false,
        lineStyle: { width: 2, color: p.current },
        z: 2,
      },
      {
        name: 'Power',
        type: 'line',
        yAxisIndex: 2,
        data: pairs(power, tSec),
        symbol: 'none',
        smooth: false,
        lineStyle: { width: 2, color: p.power },
        z: 1,
      },
    ],
  }
}

/** Render the chart from data at the requested size and return a PNG canvas.
 *  Returns null on any failure so callers can show feedback instead of failing
 *  silently. */
export async function renderChartImage(params: ChartExportParams): Promise<HTMLCanvasElement | null> {
  const { tSec, volts, amps, theme, width, height, title } = params
  const mod = await loadChartDeps()
  const palette = mod.CHART_PALETTES[theme]

  const root = document.createElement('div')
  root.style.position = 'absolute'
  root.style.left = '-100000px'
  root.style.top = '0'
  root.style.width = `${width}px`
  root.style.height = `${height}px`
  document.body.appendChild(root)

  let chart: ReturnType<typeof mod.echarts.init> | null = null
  try {
    // devicePixelRatio 1 makes getRenderedCanvas() match the requested size
    // exactly, independent of the display's devicePixelRatio.
    chart = mod.echarts.init(root, null, { width, height, renderer: 'canvas', devicePixelRatio: 1 })
    chart.setOption(buildExportChartOption({ tSec, volts, amps, theme, title }))

    const canvas = chart.getRenderedCanvas()

    // The ECharts canvas already carries the report background; composite it
    // once more so the returned canvas has a solid background instead of alpha.
    const out = document.createElement('canvas')
    out.width = canvas.width
    out.height = canvas.height
    const ctx = out.getContext('2d')
    if (!ctx) return null
    ctx.fillStyle = palette.background
    ctx.fillRect(0, 0, out.width, out.height)
    ctx.drawImage(canvas, 0, 0)
    return out
  } catch {
    return null
  } finally {
    chart?.dispose()
    root.remove()
  }
}

/** Encode a canvas as a PNG data URL.
 *
 *  Uses async `toBlob` (encoded off the main thread in Chromium) instead of
 *  the synchronous `toDataURL`, which freezes the UI for hundreds of ms on
 *  multi-megapixel canvases. Output format matches what `write_chart_png`
 *  expects ("data:image/png;base64,..."). */
export function canvasToPngDataUrl(canvas: HTMLCanvasElement): Promise<string> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (!blob) {
        reject(new Error('PNG 编码失败'))
        return
      }
      const reader = new FileReader()
      reader.onload = () => resolve(reader.result as string)
      reader.onerror = () => reject(reader.error ?? new Error('PNG 读取失败'))
      reader.readAsDataURL(blob)
    }, 'image/png')
  })
}
