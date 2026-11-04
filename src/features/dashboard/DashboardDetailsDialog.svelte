<script lang="ts">
  import type { Measurement, PdEvent, SessionStats } from '../../types'
  import CcLinesPanel from './CcLinesPanel.svelte'
  import SystemStatusPanel from './SystemStatusPanel.svelte'
  import AccumulationPanel from '../telemetry/AccumulationPanel.svelte'
  import SessionStatsPanel from './SessionStatsPanel.svelte'

  let { open, connected, reconnecting, latest, events, capacityMah, energyWh, uptimeMs, sessionStats, onDisconnect, onReset, onClose }: { open: boolean; connected: boolean; reconnecting: boolean; latest: Measurement | null; events: PdEvent[]; capacityMah: number; energyWh: number; uptimeMs: number; sessionStats: SessionStats; onDisconnect: () => void; onReset: () => void; onClose: () => void } = $props()

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') onClose()
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="backdrop">
    <button class="backdrop-dismiss" onclick={onClose} aria-label="关闭状态窗口"></button>
    <dialog open class="details-dialog" aria-labelledby="details-title">
      <header><h2 id="details-title">运行状态</h2><button class="close" onclick={onClose} aria-label="关闭状态窗口" title="关闭">&#10005;</button></header>
      <div class="details-grid"><CcLinesPanel {latest} {connected} /><SystemStatusPanel {connected} {reconnecting} {latest} {events} {onDisconnect} /><AccumulationPanel {capacityMah} {energyWh} {uptimeMs} {onReset} /><SessionStatsPanel stats={sessionStats} /></div>
    </dialog>
  </div>
{/if}

<style>
  .backdrop { position: fixed; z-index: 20; inset: 0; display: grid; place-items: center; padding: 20px; background: rgb(15 23 42 / 28%); }.backdrop-dismiss { position: absolute; inset: 0; padding: 0; border: 0; background: transparent; cursor: default; }  .details-dialog { position: relative; z-index: 1; width: min(940px, 100%); margin: 0; padding: 0; border: 1px solid #d9dee4; border-radius: 6px; background: #fff; box-shadow: 0 18px 45px rgb(15 23 42 / 22%); }.details-dialog header { display: flex; align-items: center; justify-content: space-between; padding: 12px 14px; border-bottom: 1px solid #e5e7eb; }.details-dialog h2 { margin: 0; color: #1f2937; font-size: 15px; }.close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; border: 1px solid #d4d7dc; border-radius: 4px; color: #64748b; background: #fff; cursor: pointer; font-size: 13px; }.close:hover { color: #1f2937; background: #f8fafc; }  .details-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; padding: 12px; }.details-grid :global(.panel), .details-grid :global(.accumulation) { min-width: 0; }.details-grid :global(.stats-panel) { grid-column: 1 / -1; }
  @media (max-width: 760px) { .details-grid { grid-template-columns: 1fr; max-height: calc(100vh - 110px); overflow: auto; } }
</style>
