<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import type { RecordingOptions, RecordingStatus } from './types'

  let { options, status, disabled, onChange }: { options: RecordingOptions; status: RecordingStatus | null; disabled: boolean; onChange: (options: RecordingOptions) => void } = $props()
  let feedback = $state('')

  function numberOption(key: 'current_threshold_ma' | 'hold_seconds' | 'max_buffer_mb', event: Event) {
    const value = Number((event.currentTarget as HTMLInputElement).value)
    if (Number.isFinite(value)) onChange({ ...options, [key]: value })
  }

  function toggleOption(key: 'stop_when_condition_met' | 'use_system_time' | 'edge_save', event: Event) {
    onChange({ ...options, [key]: (event.currentTarget as HTMLInputElement).checked })
  }

  function formatBytes(bytes: number) {
    return bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`
  }

  async function openDir() {
    feedback = ''
    try {
      await invoke('open_recordings_dir')
      feedback = '已打开记录目录'
    } catch (reason) {
      feedback = String(reason)
    }
  }

  async function saveAs() {
    if (!status) return
    feedback = ''
    try {
      const result = await invoke<string | null>('save_recording_as', { sourcePath: status.file_path })
      feedback = result ? `已另存为：${result}` : '已取消'
    } catch (reason) {
      feedback = String(reason)
    }
  }

  async function toCsv() {
    if (!status) return
    feedback = ''
    try {
      const result = await invoke<string | null>('convert_recording_to_csv', { sourcePath: status.file_path, rangeStartHostMs: null, rangeEndHostMs: null })
      feedback = result ? `已导出：${result}` : '已取消'
    } catch (reason) {
      feedback = String(reason)
    }
  }
</script>

<section class="recording-settings" aria-labelledby="recording-title">
  <h3 id="recording-title">会话记录</h3>
  <div class="condition-grid">
    <label>电流阈值<input type="number" min="0" step="0.1" value={options.current_threshold_ma} disabled={disabled} onchange={(event) => numberOption('current_threshold_ma', event)} /><small>mA</small></label>
    <label>持续时间<input type="number" min="1" max="86400" step="1" value={options.hold_seconds} disabled={disabled} onchange={(event) => numberOption('hold_seconds', event)} /><small>秒</small></label>
  </div>
  <label class="check-row"><input type="checkbox" checked={options.stop_when_condition_met} disabled={disabled} onchange={(event) => toggleOption('stop_when_condition_met', event)} /><span><strong>满足条件自动停止</strong><small>电流绝对值不高于阈值并持续设定时间后停止记录，不会断开设备。</small></span></label>
  <label class="check-row"><input type="checkbox" checked={options.use_system_time} disabled={disabled} onchange={(event) => toggleOption('use_system_time', event)} /><span><strong>写入系统开始时间</strong><small>在会话文件头保存本机 Unix 时间戳。</small></span></label>
  <label class="check-row"><input type="checkbox" checked={options.edge_save} disabled={disabled} onchange={(event) => toggleOption('edge_save', event)} /><span><strong>边采集边保存</strong><small>每个数据批次都同步写入文件；关闭后在缓存达到上限或停止记录时写入。</small></span></label>
  <label class="buffer-row">缓存深度<input type="number" min="1" max="1024" step="1" value={options.max_buffer_mb} disabled={disabled} onchange={(event) => numberOption('max_buffer_mb', event)} /><small>MB</small></label>
  <p class="buffer-note">缓存达到上限会自动写入会话文件，不会为保持缓存深度而丢弃样本。</p>
  {#if status}
    <div class:active={status.active} class="recording-status"><strong>{status.active ? '正在记录' : '最近一次记录已结束'}</strong><span>{status.samples.toLocaleString()} 个采样点 / {status.events.toLocaleString()} 条 PD 事件 / 已写入 {formatBytes(status.bytes_written)}</span><code title={status.file_path}>{status.file_path}</code>{#if status.stop_reason}<small>结束原因：{status.stop_reason}</small>{/if}</div>
    <div class="recording-actions"><button onclick={() => void openDir()}>打开目录</button><button onclick={() => void saveAs()}>另存为</button><button onclick={() => void toCsv()}>转 CSV</button></div>
    {#if feedback}<p class="recording-feedback" role="status">{feedback}</p>{/if}
  {/if}
</section>

<style>
  .recording-settings { padding: 15px; border-top: 1px solid #e5e7eb; }.recording-settings h3 { margin: 0 0 10px; color: #64748b; font-size: 11px; font-weight: 600; }.condition-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 9px; }.condition-grid label, .buffer-row { display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 6px; color: #64748b; font-size: 11px; }.condition-grid input, .buffer-row input { width: 88px; padding: 6px 7px; border: 1px solid #d4d7dc; border-radius: 4px; color: #1e293b; background: #fff; font: 12px ui-monospace, monospace; }.condition-grid small, .buffer-row small { color: #94a3b8; }.check-row { display: flex; align-items: flex-start; gap: 9px; margin-top: 11px; cursor: pointer; }.check-row input { width: 16px; height: 16px; flex: none; margin: 1px 0 0; accent-color: #2563eb; }.check-row strong, .check-row small { display: block; }.check-row strong { color: #334155; font-size: 12px; }.check-row small { margin-top: 3px; color: #94a3b8; font-size: 10px; line-height: 1.35; }.buffer-row { margin-top: 12px; grid-template-columns: 1fr 92px auto; }.buffer-note { margin: 5px 0 0; color: #94a3b8; font-size: 10px; line-height: 1.35; }  .recording-status { display: grid; gap: 4px; margin-top: 13px; padding: 9px; border: 1px solid #e2e8f0; border-left: 3px solid #64748b; background: #f8fafc; }.recording-status.active { border-left-color: #dc2626; }.recording-status strong { color: #334155; font-size: 11px; }.recording-status span, .recording-status small { color: #64748b; font-size: 10px; }.recording-status code { color: #475569; font: 9px/1.35 ui-monospace, monospace; overflow-wrap: anywhere; }.recording-actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 9px; }.recording-actions button { padding: 5px 9px; border: 1px solid #cbd5e1; border-radius: 4px; color: #334155; background: #fff; cursor: pointer; font-size: 10px; }.recording-actions button:hover { background: #f8fafc; }.recording-feedback { margin: 7px 0 0; color: #059669; font-size: 10px; line-height: 1.4; overflow-wrap: anywhere; }
  @media (max-width: 440px) { .condition-grid { grid-template-columns: 1fr; } }
</style>
