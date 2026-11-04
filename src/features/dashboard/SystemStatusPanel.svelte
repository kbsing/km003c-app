<script lang="ts">
  import type { Measurement, PdEvent } from '../../types'
  let { connected, reconnecting, latest, events, onDisconnect }: { connected: boolean; reconnecting: boolean; latest: Measurement | null; events: PdEvent[]; onDisconnect: () => void } = $props()
  const status = $derived(reconnecting ? '正在重连' : connected ? '已连接' : '未连接')
  const vbus = $derived(latest && latest.vbus_v > 1 ? '有电压' : '无电压')
  const negotiation = $derived(events[0]?.summary ?? '等待 PD 报文')
  const temperature = $derived(latest?.temp_c ?? null)
</script>

<section class="panel" aria-label="系统状态">
  <h2>系统状态</h2>
  <div class="rows"><div><span>连接状态</span><b class:active={connected}>{status}</b></div><div><span>VBUS 状态</span><b>{vbus}</b></div><div><span>PD 活动</span><b title={negotiation}>{events.length > 0 ? '已检测' : '等待中'}</b></div></div>
  <div class="temperature"><div><span>设备温度</span><b>{temperature === null ? '—' : `${temperature.toFixed(1)}°C`}</b></div><div class="bar"><i style={`width:${Math.min(Math.max((temperature ?? 0) / 80 * 100, 0), 100)}%`}></i></div></div>
  {#if connected || reconnecting}<button class="disconnect" onclick={onDisconnect}>断开连接</button>{/if}
</section>

<style>
  .panel { border: 1px solid #e4e7eb; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); overflow: hidden; }.panel h2 { margin: 0; padding: 12px 15px; border-bottom: 1px solid #e5e7eb; color: #1f2937; font-size: 14px; }.rows { padding: 14px 15px 7px; }.rows div, .temperature > div:first-child { display: flex; justify-content: space-between; align-items: center; gap: 10px; padding: 7px 0; border-bottom: 1px solid #f1f5f9; color: #64748b; font-size: 12px; }.rows b, .temperature b { color: #1e293b; font: 12px ui-monospace, Consolas, monospace; }.rows b.active { color: #059669; }.temperature { padding: 3px 15px 15px; }.temperature > div:first-child { border-bottom: 0; }.bar { height: 7px; overflow: hidden; border-radius: 4px; background: #e2e8f0; }.bar i { display: block; height: 100%; background: #f59e0b; }.disconnect { margin: 0 15px 15px; padding: 8px 11px; border: 1px solid #fecaca; border-radius: 4px; color: #dc2626; background: #fff; cursor: pointer; font-size: 11px; }.disconnect:hover { background: #fef2f2; }
</style>
