<script lang="ts">
  import type { EChartsType } from 'echarts/core'
  import { onMount } from 'svelte'
  import { formatSeconds } from './format'
  import { loadChartDeps, type ChartDeps } from './chartLoader'
  import type { ChartOption } from './chartOption'

  let {
    tSec,
    volts,
    start,
    end,
    onChange,
  }: {
    tSec: number[]
    volts: Array<number | null>
    start: number
    end: number
    onChange: (start: number, end: number) => void
  } = $props()

  let root = $state<HTMLDivElement | null>(null)
  let chart = $state<EChartsType | null>(null)
  let deps = $state<ChartDeps | null>(null)

  const MONO = 'ui-monospace, "Cascadia Mono", Consolas, monospace'
  const VBUS = '#2f7795'

  function pairs(values: Array<number | null>): Array<[number, number | null]> {
    const out: Array<[number, number | null]> = new Array(values.length)
    for (let i = 0; i < values.length; i += 1) {
      const v = values[i]
      out[i] = v === null || v === undefined ? [tSec[i], null] : [tSec[i], v]
    }
    return out
  }

  function buildOption(): ChartOption {
    const first = tSec.length > 0 ? tSec[0] : 0
    const last = tSec.length > 0 ? tSec[tSec.length - 1] : 1
    const span = Math.max(last - first, 0.001)
    return {
      animation: false,
      // The grid is fully covered by the opaque slider below; the VBUS series
      // only feeds the slider's full-session data shadow (mini chart).
      grid: { left: 0, right: 0, top: 0, bottom: 0 },
      xAxis: { type: 'value', show: false, min: first, max: last },
      yAxis: { type: 'value', show: false, min: 'dataMin', max: 'dataMax' },
      series: [
        {
          type: 'line',
          data: pairs(volts),
          symbol: 'none',
          lineStyle: { width: 1, color: VBUS },
          silent: true,
        },
      ],
      dataZoom: [
        {
          type: 'slider',
          xAxisIndex: 0,
          height: '100%',
          bottom: 0,
          start: start * 100,
          end: end * 100,
          showDataShadow: true,
          brushSelect: false,
          borderColor: '#cbd5e1',
          backgroundColor: '#f8fafc',
          fillerColor: 'rgba(37,99,235,0.12)',
          handleSize: '90%',
          handleStyle: { color: '#eff6ff', borderColor: '#1d4ed8' },
          moveHandleSize: 8,
          moveHandleStyle: { color: '#1d4ed8' },
          textStyle: { color: '#475569', fontSize: 9, fontFamily: MONO },
          labelFormatter: (value: number) => formatSeconds(value, span),
          dataBackground: {
            lineStyle: { color: VBUS, width: 1 },
            areaStyle: { color: 'rgba(47,119,149,0.12)' },
          },
        },
      ],
    }
  }

  function handleDataZoom(params: unknown) {
    const p = params as { batch?: Array<{ start?: number; end?: number }>; start?: number; end?: number }
    const dz = p && Array.isArray(p.batch) && p.batch.length > 0 ? p.batch[0] : p
    if (typeof dz?.start === 'number' && typeof dz.end === 'number') {
      onChange(dz.start / 100, dz.end / 100)
    }
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

  // Create once the container exists and ECharts has loaded; tear down on
  // unmount. Deliberately reads no data props so it only runs when the element
  // appears (or the chart deps arrive).
  $effect(() => {
    const mod = deps
    const el = root
    if (!mod || !el) return
    const rect = el.getBoundingClientRect()
    const inst = mod.echarts.init(el, null, {
      width: Math.max(rect.width, 100),
      height: Math.max(rect.height, 40),
    })
    chart = inst
    inst.on('datazoom', handleDataZoom)
    let resizeFrame: number | null = null
    const resizeNow = () => {
      resizeFrame = null
      const rect = el.getBoundingClientRect()
      const width = Math.max(Math.floor(rect.width || el.clientWidth), 100)
      const height = Math.max(Math.floor(rect.height || el.clientHeight), 40)
      inst.resize({ width, height, animation: { duration: 0 }, silent: true })
    }
    const scheduleResize = () => {
      if (resizeFrame === null) resizeFrame = window.requestAnimationFrame(resizeNow)
    }
    const ro = new ResizeObserver(scheduleResize)
    ro.observe(el)
    window.addEventListener('resize', scheduleResize)
    resizeNow()
    return () => {
      ro.disconnect()
      window.removeEventListener('resize', scheduleResize)
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame)
      resizeFrame = null
      inst.off('datazoom', handleDataZoom)
      inst.dispose()
      chart = null
    }
  })

  // Push data and the selection window whenever they change.
  $effect(() => {
    const inst = chart
    if (!inst) return
    inst.setOption(buildOption())
  })
</script>

<div class="navigator" bind:this={root} role="group" aria-label="曲线显示范围"></div>

<style>
  .navigator { width: 100%; min-width: 0; height: 52px; border: 1px solid #cbd5e1; background: #f8fafc; touch-action: none; user-select: none; overflow: hidden; }
</style>
