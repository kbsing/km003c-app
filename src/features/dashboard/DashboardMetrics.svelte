<script lang="ts">
  import type { Measurement } from '../../types'
  import { magnitudePower } from '../../lib/measurement'
  import { formatCurrent, formatPower } from '../../lib/format'
  let { latest, showWitrnExtras, hidePower }: { latest: Measurement | null; showWitrnExtras: boolean; hidePower: boolean } = $props()
</script>

<section class="metrics" class:four={showWitrnExtras} class:no-power={hidePower}>
  <div><span>电压</span><strong class="voltage">{latest ? latest.vbus_v.toFixed(3) : '—'}<small>V</small></strong></div>
  <div><span>电流</span><strong class="current">{latest ? formatCurrent(Math.abs(latest.ibus_a)).split(' ')[0] : '—'}<small>{latest ? formatCurrent(Math.abs(latest.ibus_a)).split(' ')[1] : 'A'}</small></strong></div>
  {#if !hidePower}
  <div><span>功率</span><strong>{latest ? formatPower(magnitudePower(latest)).split(' ')[0] : '—'}<small>{latest ? formatPower(magnitudePower(latest)).split(' ')[1] : 'W'}</small></strong></div>
  {/if}
  {#if showWitrnExtras}
    <div class="extras">
      <div class="mini-grid">
        <div class="mini"><span>D+</span><b>{latest && latest.d_plus_v !== null ? latest.d_plus_v.toFixed(2) : '—'}<small>V</small></b></div>
        <div class="mini"><span>D-</span><b>{latest && latest.d_minus_v !== null ? latest.d_minus_v.toFixed(2) : '—'}<small>V</small></b></div>
        <div class="mini"><span>Ah</span><b>{latest && latest.amp_hours !== null ? latest.amp_hours.toFixed(3) : '—'}</b></div>
        <div class="mini"><span>Wh</span><b>{latest && latest.watt_hours !== null ? latest.watt_hours.toFixed(3) : '—'}</b></div>
      </div>
    </div>
  {/if}
</section>

<style>
  .metrics { display: grid; grid-template-columns: repeat(3, 1fr); border: 1px solid #e4e7eb; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); }.metrics div { min-height: 112px; min-width: 0; padding: 18px 26px; overflow: hidden; }.metrics div + div { border-left: 1px solid #e5e7eb; }.metrics span { display: block; margin-bottom: 7px; color: #64748b; font-size: 12px; letter-spacing: .04em; }.metrics strong { color: #111827; font: 600 clamp(31px, 3vw, 42px)/1 ui-monospace, Consolas, monospace; font-variant-numeric: tabular-nums; }.metrics strong.voltage { color: #2563eb; }.metrics strong.current { color: #d97706; }.metrics small { margin-left: 6px; color: #94a3b8; font-size: 13px; font-weight: 500; }
  .metrics.four { grid-template-columns: repeat(4, 1fr); }.metrics.four div { padding: 16px 18px; }.metrics.four strong { font-size: clamp(24px, 2.3vw, 32px); }
  .metrics.no-power { grid-template-columns: repeat(2, 1fr); }
  .metrics .extras { display: flex; align-items: center; }.mini-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 7px 14px; width: 100%; }.mini span { margin-bottom: 2px; font-size: 9px; letter-spacing: .06em; }.mini b { color: #1e293b; font: 600 15px/1.15 ui-monospace, Consolas, monospace; font-variant-numeric: tabular-nums; white-space: nowrap; }.mini b small { margin-left: 3px; font-size: 9px; }
  @media (max-width: 560px) { .metrics div { min-height: 100px; padding: 16px; }.metrics strong { font-size: 26px; }.metrics span { font-size: 10px; }.metrics.four { grid-template-columns: repeat(2, 1fr); }.metrics.four strong { font-size: 24px; } }
</style>
