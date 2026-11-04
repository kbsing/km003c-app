<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'

  let { open, kind, sourcePath, viewHostRange, onClose }: {
    open: boolean
    kind: 'csv' | 'json'
    sourcePath: string
    viewHostRange: { start: number; end: number } | null
    onClose: () => void
  } = $props()

  let range = $state<'full' | 'view'>('full')
  let exporting = $state(false)
  let notice = $state('')

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') onClose()
  }

  async function doExport() {
    if (!sourcePath || (range === 'view' && !viewHostRange)) return
    exporting = true
    notice = ''
    const args = {
      sourcePath,
      rangeStartHostMs: range === 'view' ? viewHostRange?.start ?? null : null,
      rangeEndHostMs: range === 'view' ? viewHostRange?.end ?? null : null,
    }
    try {
      const command = kind === 'csv' ? 'convert_recording_to_csv' : 'convert_recording_to_json'
      const saved = await invoke<string | null>(command, args)
      if (saved) onClose()
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
    <button class="backdrop-dismiss" onclick={onClose} aria-label="关闭数据导出设置"></button>
    <dialog open class="export-dialog" aria-labelledby="data-export-title">
      <header><h2 id="data-export-title">导出{kind === 'csv' ? ' CSV 表格' : ' JSON 数据'}</h2><button class="close" onclick={onClose} aria-label="关闭" title="关闭">&#10005;</button></header>
      <div class="export-body">
        <div class="option-row"><span>数据范围</span><div class="seg"><button class:active={range === 'full'} onclick={() => range = 'full'}>完整会话</button><button class:active={range === 'view'} disabled={!viewHostRange} onclick={() => range = 'view'}>当前视窗</button></div></div>
        {#if range === 'view' && viewHostRange}<p class="range-note">按记录文件的 host_ms 过滤：{viewHostRange.start}–{viewHostRange.end} ms</p>{/if}
        {#if !viewHostRange}<p class="range-note">当前视窗没有可映射到记录文件的采样点。</p>{/if}
        {#if kind === 'csv'}<p class="schema-note">CSV 同时保留带符号电流/功率、绝对值和设备累计 Ah/Wh；空值留空。</p>{:else}<p class="schema-note">JSON 使用 schema_version 2，并在 range、columns 中记录筛选范围与字段语义。</p>{/if}
        {#if notice}<p class="export-notice" role="status">{notice}</p>{/if}
      </div>
      <footer><button class="primary" onclick={() => void doExport()} disabled={exporting || !sourcePath || (range === 'view' && !viewHostRange)}>{exporting ? '正在导出…' : '导出'}</button></footer>
    </dialog>
  </div>
{/if}

<style>
  .backdrop { position: fixed; z-index: 21; inset: 0; display: grid; place-items: center; padding: 20px; background: rgb(15 23 42 / 28%); }
  .backdrop-dismiss { position: absolute; inset: 0; padding: 0; border: 0; background: transparent; cursor: default; }
  .export-dialog { position: relative; z-index: 1; width: min(470px, 100%); margin: 0; padding: 0; border: 1px solid #d9dee4; border-radius: 6px; background: #fff; box-shadow: 0 18px 45px rgb(15 23 42 / 22%); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 13px 15px; border-bottom: 1px solid #e5e7eb; }
  h2 { margin: 0; color: #1f2937; font-size: 15px; }
  .close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; border: 1px solid #d4d7dc; border-radius: 4px; color: #64748b; background: #fff; cursor: pointer; }
  .export-body { display: grid; gap: 11px; padding: 15px; }
  .option-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .option-row > span { color: #475569; font-size: 12px; white-space: nowrap; }
  .seg { display: flex; gap: 6px; flex-wrap: wrap; justify-content: flex-end; }
  .seg button { padding: 6px 10px; border: 1px solid #cbd5e1; border-radius: 4px; color: #475569; background: #fff; cursor: pointer; font-size: 11px; }
  .seg button.active { border-color: #2563eb; color: #fff; background: #2563eb; }
  .seg button:disabled { cursor: default; opacity: .45; }
  .range-note, .schema-note { margin: 0; color: #64748b; font-size: 10px; line-height: 1.45; }
  .export-notice { margin: 0; color: #b91c1c; font-size: 10px; }
  footer { padding: 0 15px 15px; text-align: right; }
  footer button { padding: 8px 18px; border: 1px solid #2563eb; border-radius: 4px; color: #fff; background: #2563eb; cursor: pointer; font-size: 12px; }
  footer button:disabled { cursor: default; opacity: .55; }
</style>
