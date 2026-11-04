<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { renderChartImage, canvasToPngDataUrl, type ExportTheme } from './chartExport'
  import type { ChartRange } from '../types'

  let { open, tSec, volts, amps, viewRange, onClose }: {
    open: boolean
    tSec: number[]
    volts: Array<number | null>
    amps: Array<number | null>
    viewRange: Pick<ChartRange, 'start' | 'end'>
    onClose: () => void
  } = $props()

  const RESOLUTIONS = [
    { label: '标准 1280×720', width: 1280, height: 720 },
    { label: '高清 1920×1080', width: 1920, height: 1080 },
    { label: '超清 2560×1440', width: 2560, height: 1440 },
  ] as const

  let theme = $state<ExportTheme>('light')
  let resolution = $state(1)
  let title = $state('Star monitor')
  let exporting = $state(false)
  let notice = $state('')
  let range = $state<'full' | 'view'>('full')

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') onClose()
  }

  async function doExport() {
    const preset = RESOLUTIONS[resolution]
    exporting = true
    notice = ''
    try {
      const selected = range === 'view'
        ? tSec.reduce<{ tSec: number[]; volts: Array<number | null>; amps: Array<number | null> }>((out, t, index) => {
            if (t >= viewRange.start && t <= viewRange.end) {
              out.tSec.push(t)
              out.volts.push(volts[index] ?? null)
              out.amps.push(amps[index] ?? null)
            }
            return out
          }, { tSec: [], volts: [], amps: [] })
        : { tSec, volts, amps }
      if (selected.tSec.length === 0) {
        notice = '当前视窗没有可导出的采样点'
        return
      }
      // Pick the destination first: the dialog appears instantly after the
      // click, and cancelling it skips the render entirely.
      const stamp = new Date().toISOString().replace(/[:.]/g, '-').replace('T', '_').replace('Z', '')
      const dest = await invoke<string | null>('pick_chart_png_path', {
        defaultName: `km003c-chart-${stamp}.png`,
      })
      if (!dest) return
      // Let the "exporting" state paint before the synchronous canvas render.
      await new Promise((resolve) => requestAnimationFrame(resolve))
      const canvas = await renderChartImage({
        ...selected,
        theme,
        width: preset.width,
        height: preset.height,
        title,
      })
      if (!canvas) {
        notice = '渲染图表失败'
        return
      }
      await invoke('write_chart_png', { path: dest, dataUrl: await canvasToPngDataUrl(canvas) })
      onClose()
    } catch (reason) {
      notice = `导出失败：${String(reason)}`
    } finally {
      exporting = false
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="backdrop">
    <button class="backdrop-dismiss" onclick={onClose} aria-label="关闭导出设置"></button>
    <dialog open class="export-dialog" aria-labelledby="export-title">
      <header><h2 id="export-title">导出图表</h2><button class="close" onclick={onClose} aria-label="关闭导出设置" title="关闭">&#10005;</button></header>
      <div class="export-body">
        <div class="option-row title-row"><span>报告标题</span><input type="text" value={title} placeholder="如：充电器 PD 测试报告" oninput={(event) => title = (event.currentTarget as HTMLInputElement).value} /></div>
        <div class="option-row"><span>样式</span><div class="seg"><button class:active={theme === 'light'} onclick={() => theme = 'light'}>浅色</button><button class:active={theme === 'dark'} onclick={() => theme = 'dark'}>深色</button></div></div>
        <div class="option-row"><span>数据范围</span><div class="seg"><button class:active={range === 'full'} onclick={() => range = 'full'}>完整会话（保留数据）</button><button class:active={range === 'view'} onclick={() => range = 'view'}>当前视窗</button></div></div>
        <p class="scope-note">图片使用前端为流畅显示保留的曲线数据；CSV/JSON 的完整会话会从记录文件读取。</p>
        <div class="option-row"><span>分辨率</span><div class="seg">{#each RESOLUTIONS as preset, index}<button class:active={resolution === index} onclick={() => resolution = index}>{preset.label}</button>{/each}</div></div>
        {#if notice}<p class="export-notice" role="status">{notice}</p>{/if}
      </div>
      <footer><button class="primary" onclick={() => void doExport()} disabled={exporting}>{exporting ? '正在导出…' : '导出'}</button></footer>
    </dialog>
  </div>
{/if}

<style>
  .backdrop { position: fixed; z-index: 20; inset: 0; display: grid; place-items: center; padding: 20px; background: rgb(15 23 42 / 28%); }.backdrop-dismiss { position: absolute; inset: 0; padding: 0; border: 0; background: transparent; cursor: default; }.export-dialog { position: relative; z-index: 1; width: min(480px, 100%); margin: 0; padding: 0; border: 1px solid #d9dee4; border-radius: 6px; background: #fff; box-shadow: 0 18px 45px rgb(15 23 42 / 22%); }.export-dialog header { display: flex; align-items: center; justify-content: space-between; padding: 13px 15px; border-bottom: 1px solid #e5e7eb; }.export-dialog h2 { margin: 0; color: #1f2937; font-size: 15px; }.close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; border: 1px solid #d4d7dc; border-radius: 4px; color: #64748b; background: #fff; cursor: pointer; font-size: 13px; }.close:hover { color: #1f2937; background: #f8fafc; }  .export-body { display: grid; gap: 12px; padding: 15px; }.option-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }.option-row > span { color: #475569; font-size: 12px; white-space: nowrap; }.option-row.title-row input { flex: 1; min-width: 0; padding: 6px 9px; border: 1px solid #cbd5e1; border-radius: 4px; color: #1e293b; background: #fff; font-size: 12px; }.seg { display: flex; gap: 6px; flex-wrap: wrap; justify-content: flex-end; }.seg button { padding: 6px 10px; border: 1px solid #cbd5e1; border-radius: 4px; color: #475569; background: #fff; cursor: pointer; font-size: 11px; }.seg button:hover { background: #f8fafc; }.seg button.active { border-color: #2563eb; color: #fff; background: #2563eb; }.export-notice { margin: 0; color: #b91c1c; font-size: 10px; line-height: 1.4; }.scope-note { margin: 0; color: #64748b; font-size: 10px; line-height: 1.4; }.export-dialog footer { padding: 0 15px 15px; text-align: right; }.export-dialog footer button { padding: 8px 18px; border: 1px solid #2563eb; border-radius: 4px; color: #fff; background: #2563eb; cursor: pointer; font-size: 12px; }.export-dialog footer button:disabled { cursor: default; opacity: .55; }
</style>
