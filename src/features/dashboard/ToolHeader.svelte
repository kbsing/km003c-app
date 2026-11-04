<script lang="ts">
  import type { Stats } from '../../types'
  let { connected, reconnecting, starting, paused, replaying, sessionPhase, canExport, deviceModel, stats, onConnect, onTogglePause, onStartSession, onStopSession, onOpenSettings, onOpenDetails, onClear, onExportCsv, onExportDebug }: { connected: boolean; reconnecting: boolean; starting: boolean; paused: boolean; replaying: boolean; sessionPhase: 'idle' | 'recording' | 'stopped' | 'restored'; canExport: boolean; deviceModel: string; stats: Stats | null; onConnect: () => void; onTogglePause: () => void; onStartSession: () => void; onStopSession: () => void; onOpenSettings: () => void; onOpenDetails: () => void; onClear: () => void; onExportCsv: () => void; onExportDebug: () => void } = $props()
  const recording = $derived(sessionPhase === 'recording')
  const state = $derived(reconnecting ? '正在重连' : paused ? '已暂停，仍在采集' : replaying ? '正在补齐显示' : connected ? '已连接' : starting ? '正在连接' : '未连接')
</script>

<header class="toolbar-header">
  <span class:online={connected} class="connection-light" role="status" aria-label={state} title={state}></span>
  {#if connected && deviceModel}<span class="device-model" title={deviceModel}>{deviceModel}</span>{/if}
  <div class="header-actions">
    {#if connected}<button class:recording class="session-button" onclick={recording ? onStopSession : onStartSession} disabled={starting || reconnecting} title={recording ? '结束本次记录' : '开始记录并显示曲线'}>{recording ? '结束' : '开始'}</button>{/if}
    <button class="icon-button" onclick={onOpenSettings} aria-label="设置" title="设置">&#9881;</button>
    <span class:connected class:pending={reconnecting || starting || paused || replaying} class="state">{state}</span>
    {#if connected}<button onclick={onOpenDetails} disabled={false}>状态</button>{/if}
    <button onclick={onClear} disabled={!stats || stats.ok === 0}>清空记录</button>
    <button onclick={onExportCsv} disabled={!canExport}>导出 CSV</button>
    <button onclick={onExportDebug} disabled={!canExport}>导出 JSON</button>
    {#if connected && sessionPhase === 'recording'}<button class:primary={paused} class="pause" onclick={onTogglePause}>{paused ? '继续显示' : '暂停显示'}</button>{:else if !reconnecting && !connected}<button class="primary" onclick={onConnect} disabled={starting}>{starting ? '打开中' : '连接设备'}</button>{/if}
  </div>
</header>

<style>
  .toolbar-header { display: flex; align-items: center; gap: 16px; min-height: 58px; padding: 9px 18px; border: 1px solid #d9dee4; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 8%); }.connection-light { width: 9px; height: 9px; flex: none; border-radius: 50%; background: #a1a1aa; }.connection-light.online { background: #10b981; }.device-model { max-width: 220px; overflow: hidden; color: #334155; font-size: 12px; font-weight: 600; letter-spacing: .03em; text-overflow: ellipsis; white-space: nowrap; }.state, button { font-family: ui-monospace, Consolas, monospace; }.header-actions { display: flex; align-items: center; gap: 8px; margin-left: auto; }.state { color: #64748b; font-size: 10px; letter-spacing: .08em; }.state.connected { color: #059669; }.state.pending { color: #d97706; }.header-actions button { padding: 7px 11px; border: 1px solid #d4d7dc; border-radius: 4px; color: #334155; background: #fff; cursor: pointer; font-size: 10px; letter-spacing: .04em; }.header-actions button.icon-button { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; font-family: sans-serif; font-size: 16px; line-height: 1; }.header-actions button.session-button { border-color: #2563eb; color: #fff; background: #2563eb; }.header-actions button.session-button.recording { border-color: #dc2626; color: #fff; background: #dc2626; }.header-actions button:hover:not(:disabled) { background: #f8fafc; }.header-actions button.session-button:hover:not(:disabled) { background: #1d4ed8; }.header-actions button.session-button.recording:hover:not(:disabled) { background: #b91c1c; }.header-actions button.primary { border-color: #2563eb; color: #fff; background: #2563eb; }.header-actions button.pause { border-color: #fbbf24; color: #92400e; background: #fffbeb; }.header-actions button.pause.primary { border-color: #2563eb; color: #fff; background: #2563eb; }.header-actions button:disabled { cursor: default; opacity: .45; }
  @media (max-width: 800px) { .toolbar-header { flex-wrap: wrap; }.header-actions { flex-basis: 100%; justify-content: flex-end; } }
</style>
