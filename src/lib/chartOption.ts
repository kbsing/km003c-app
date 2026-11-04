// Shared ECharts option builder for the live desktop chart and the PNG export.
// Keeping the axes/series/colors in one place means the export looks exactly
// like the live chart and the two cannot drift apart.
//
// ECharts is imported modularly (echarts/core) so the shipped bundle only
// contains the line chart, grid, tooltip/axis-pointer and dataZoom slider —
// the full `echarts` package would add ~800 KB of unused code to the startup
// bundle.
import * as echarts from 'echarts/core'
import { LineChart, type LineSeriesOption } from 'echarts/charts'
import {
  AxisPointerComponent,
  DataZoomComponent,
  DataZoomSliderComponent,
  GridComponent,
  LegendComponent,
  TitleComponent,
  TooltipComponent,
  type AxisPointerComponentOption,
  type DataZoomComponentOption,
  type GridComponentOption,
  type TooltipComponentOption,
} from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { ComposeOption } from 'echarts/core'
import { autoCurrentUnit, autoPowerUnit, formatCurrent, formatPower, formatSeconds, hexToRgba } from './format'
import type { CurveColors } from '../types'
import type { ChartEventMarker } from '../types'

echarts.use([
  LineChart,
  GridComponent,
  TooltipComponent,
  AxisPointerComponent,
  DataZoomComponent,
  DataZoomSliderComponent,
  TitleComponent,
  LegendComponent,
  CanvasRenderer,
])

// Re-export the registered ECharts instance so dynamic importers can init charts
// without importing ECharts directly (which would pull it into their chunk).
export { echarts }

export type ChartOption = ComposeOption<
  | LineSeriesOption
  | GridComponentOption
  | TooltipComponentOption
  | DataZoomComponentOption
  | AxisPointerComponentOption
>

export type ChartTheme = 'light' | 'dark'

export interface ChartPalette {
  background: string
  grid: string
  axisLabel: string
  axisLine: string
  tooltipBg: string
  tooltipBorder: string
  tooltipText: string
}

export const CHART_PALETTES: Record<ChartTheme, ChartPalette> = {
  light: {
    background: '#ffffff',
    grid: '#d7dfdc',
    axisLabel: '#687772',
    axisLine: '#c1cbc7',
    tooltipBg: 'rgb(255 255 255 / 96%)',
    tooltipBorder: '#cbd5e1',
    tooltipText: '#475569',
  },
  dark: {
    background: '#0f172a',
    grid: '#334155',
    axisLabel: '#94a3b8',
    axisLine: '#475569',
    tooltipBg: 'rgb(15 23 42 / 96%)',
    tooltipBorder: '#334155',
    tooltipText: '#e2e8f0',
  },
}

export interface ChartData {
  tSec: number[]
  volts: Array<number | null>
  amps: Array<number | null>
  temps: Array<number | null>
  showPower: boolean
  showTemperature: boolean
  compactAxes?: boolean
  colors: CurveColors
  theme: ChartTheme
  /** Visible x window in seconds. Omit (or pass undefined) to fit the data. */
  xMin?: number
  xMax?: number
  events?: ChartEventMarker[]
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

/** Builds the base ECharts option (no tooltip; callers add it). */
export function buildChartOption(data: ChartData): ChartOption {
  const { tSec, volts, amps, temps, showPower, showTemperature, compactAxes = false, colors, theme, xMin, xMax, events = [] } = data
  const palette = CHART_PALETTES[theme]
  const first = tSec.length > 0 ? tSec[0] : 0
  const last = tSec.length > 0 ? tSec[tSec.length - 1] : 1
  const span = Math.max((xMax ?? last) - (xMin ?? first), 0.001)

  const power = showPower
    ? volts.map((v, i) => (v === null || amps[i] === null ? null : v * amps[i]!))
    : null
  const currentUnit = autoCurrentUnit(amps)
  const powerUnit = autoPowerUnit(power ?? [])
  const temperatureAxisIndex = showPower ? 3 : 2
  const temperatureOffset = showPower ? 96 : 48
  const currentLabel = (value: number) => compactAxes ? formatCurrent(value, currentUnit) : (currentUnit === 'mA' ? `${(value * 1000).toFixed(1)}` : value.toFixed(3))
  const powerLabel = (value: number) => compactAxes ? formatPower(value, powerUnit) : (powerUnit === 'mW' ? `${(value * 1000).toFixed(1)}` : value.toFixed(3))
  const voltageLabel = (value: number) => compactAxes ? `${value.toFixed(2)}V` : value.toFixed(2)

  return {
    animation: false,
    legend: { show: false },
    grid: {
      left: 44,
      // Auxiliary series keep their independent scales for tooltip values, but
      // their tick labels are rendered in the legend/readout instead of
      // reserving a wide strip inside the plot.
      right: 12,
      top: 10,
      bottom: 24,
    },
    xAxis: {
      type: 'value',
      min: xMin,
      max: xMax,
      axisLabel: {
        color: palette.axisLabel,
        fontSize: 10,
        fontFamily: MONO,
        formatter: (value: number) => formatSeconds(value, span),
      },
      axisLine: { lineStyle: { color: palette.axisLine } },
      axisTick: { show: false },
      splitLine: { show: false },
    },
    yAxis: [
      {
        type: 'value',
        position: 'left',
        name: compactAxes ? undefined : 'V',
        nameLocation: 'middle',
        nameGap: 28,
        nameTextStyle: { color: colors.voltage, fontSize: 10, fontFamily: MONO },
        axisLabel: {
          color: colors.voltage,
          fontSize: 10,
          fontFamily: MONO,
          formatter: voltageLabel,
        },
        axisLine: { show: false },
        axisTick: { show: false },
        splitLine: { lineStyle: { color: palette.grid, width: 1 } },
      },
      {
        type: 'value',
        position: 'right',
        name: undefined,
        axisLabel: {
          show: false,
          color: colors.current,
          fontSize: 10,
          fontFamily: MONO,
          formatter: currentLabel,
        },
        axisLine: { show: false },
        axisTick: { show: false },
        splitLine: { show: false },
      },
      ...(showPower
        ? [
            {
              type: 'value' as const,
              position: 'right' as const,
              offset: 48,
              name: undefined,
              axisLabel: { show: false, color: colors.power, fontSize: 10, fontFamily: MONO, formatter: powerLabel },
              axisLine: { show: false },
              axisTick: { show: false },
              splitLine: { show: false },
            },
          ]
        : []),
      ...(showTemperature
        ? [
            {
              type: 'value' as const,
              position: 'right' as const,
              offset: temperatureOffset,
              name: undefined,
              axisLabel: { show: false, color: colors.temperature, fontSize: 10, fontFamily: MONO, formatter: (value: number) => value.toFixed(1) },
              axisLine: { show: false },
              axisTick: { show: false },
              splitLine: { show: false },
            },
          ]
        : []),
    ],
    series: [
      {
        name: 'VBUS',
        type: 'line',
        yAxisIndex: 0,
        data: pairs(volts, tSec),
        symbol: 'none',
        lineStyle: { width: 1.5, color: colors.voltage },
        areaStyle: { color: hexToRgba(colors.voltage, 0.07) },
        markLine: events.length > 0 ? {
          symbol: ['none', 'none'],
          silent: true,
          lineStyle: { color: '#64748b', type: 'dashed', width: 1 },
          label: { show: true, color: '#475569', fontSize: 9, formatter: (params: unknown) => String((params as { name?: string })?.name ?? '') },
          data: events.map((event) => ({ xAxis: event.t, name: event.label })),
        } : undefined,
        z: 2,
      },
      {
        name: 'IBUS',
        type: 'line',
        yAxisIndex: 1,
        data: pairs(amps, tSec),
        symbol: 'none',
        lineStyle: { width: 1.5, color: colors.current },
        areaStyle: { color: hexToRgba(colors.current, 0.07) },
        z: 1,
      },
      ...(showPower && power
        ? [
            {
              name: 'P',
              type: 'line' as const,
              yAxisIndex: 2,
              data: pairs(power, tSec),
              symbol: 'none' as const,
              lineStyle: { width: 1.5, color: colors.power },
              z: 3,
            },
          ]
        : []),
      ...(showTemperature
        ? [
            {
              name: 'TEMP',
              type: 'line' as const,
              yAxisIndex: temperatureAxisIndex,
              data: pairs(temps, tSec),
              symbol: 'none' as const,
              lineStyle: { width: 1.5, color: colors.temperature },
              z: 0,
            },
          ]
        : []),
    ],
  }
}

/**
 * Keep ECharts responsible for the axis pointer only. Its HTML tooltip is
 * deliberately suppressed: rendering and laying out HTML on every pointer
 * event is expensive on long sessions, while Chart.svelte owns a lightweight
 * composited hover card.
 */
export function buildTooltip(palette: ChartPalette): TooltipComponentOption {
  return {
    trigger: 'axis',
    triggerOn: 'mousemove',
    showContent: false,
    axisPointer: {
      type: 'cross',
      lineStyle: { color: palette.axisLine, type: 'dashed' },
      label: { backgroundColor: palette.axisLine },
    },
  }
}
