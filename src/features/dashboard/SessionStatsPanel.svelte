<script lang="ts">
  import type { SessionStats } from '../../types'

  let { stats }: { stats: SessionStats } = $props()

  function value(count: number, number: number, digits: number): string {
    return count > 0 ? number.toFixed(digits) : '—'
  }
</script>

<section class="panel stats-panel" aria-label="会话统计">
  <h2>会话统计</h2>
  <div class="stat-grid">
    <div class="stat-card">
      <h3>电压 <small>V</small></h3>
      <dl><dt>最小</dt><dd>{value(stats.count, stats.vbus.min, 3)}</dd><dt>最大</dt><dd>{value(stats.count, stats.vbus.max, 3)}</dd><dt>平均</dt><dd>{value(stats.count, stats.vbus.avg, 3)}</dd></dl>
    </div>
    <div class="stat-card">
      <h3>电流 <small>A</small></h3>
      <dl><dt>最小</dt><dd>{value(stats.count, stats.ibus.min, 3)}</dd><dt>最大</dt><dd>{value(stats.count, stats.ibus.max, 3)}</dd><dt>平均</dt><dd>{value(stats.count, stats.ibus.avg, 3)}</dd></dl>
    </div>
    <div class="stat-card">
      <h3>功率 <small>W</small></h3>
      <dl><dt>最小</dt><dd>{value(stats.count, stats.power.min, 3)}</dd><dt>最大</dt><dd>{value(stats.count, stats.power.max, 3)}</dd><dt>平均</dt><dd>{value(stats.count, stats.power.avg, 3)}</dd></dl>
    </div>
  </div>
  <p class="stat-note">本次连接的完整采样，共 {stats.count.toLocaleString()} 个点</p>
</section>

<style>
  .stats-panel { overflow: hidden; border: 1px solid #e4e7eb; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); }.stats-panel h2 { margin: 0; padding: 12px 15px; border-bottom: 1px solid #e5e7eb; color: #1f2937; font-size: 14px; }.stat-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; padding: 12px 15px 4px; }.stat-card { padding: 9px 10px; border: 1px solid #e4e7eb; border-radius: 5px; background: #f8fafc; }.stat-card h3 { margin: 0 0 6px; color: #475569; font-size: 11px; }.stat-card h3 small { color: #94a3b8; font-weight: 500; }.stat-card dl { display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; margin: 0; }.stat-card dt { color: #94a3b8; font-size: 10px; }.stat-card dd { margin: 0; color: #1e293b; font: 12px ui-monospace, Consolas, monospace; text-align: right; font-variant-numeric: tabular-nums; }.stat-note { margin: 8px 15px 12px; color: #94a3b8; font-size: 10px; }
  @media (max-width: 760px) { .stat-grid { grid-template-columns: 1fr; } }
</style>
