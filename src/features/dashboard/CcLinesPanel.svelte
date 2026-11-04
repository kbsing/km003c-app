<script lang="ts">
  import type { Measurement } from '../../types'
  let { latest, connected }: { latest: Measurement | null; connected: boolean } = $props()
</script>

<section class="panel">
  <h2>CC 线路</h2>
  <div class="cells"><div><span>CC1</span><strong>{latest ? latest.cc1_v.toFixed(3) : '—'} <small>V</small></strong></div><div><span>CC2</span><strong>{latest ? latest.cc2_v.toFixed(3) : '—'} <small>V</small></strong></div></div>
  <div class="foot"><span>接口状态</span><b>{connected ? '已连接' : '未连接'}</b></div>
</section>

<style>
  .panel { border: 1px solid #e4e7eb; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); overflow: hidden; }.panel h2 { margin: 0; padding: 12px 15px; border-bottom: 1px solid #e5e7eb; color: #1f2937; font-size: 14px; }.cells { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; padding: 14px 15px 11px; }.cells div { padding: 11px; border: 1px solid #e2e8f0; border-radius: 4px; background: #f8fafc; }.cells span, .foot span { display: block; color: #64748b; font-size: 11px; }.cells strong { display: block; margin-top: 4px; color: #1e293b; font: 16px ui-monospace, Consolas, monospace; }.cells small { color: #94a3b8; font-size: 10px; }.foot { display: flex; justify-content: space-between; align-items: center; margin: 0 15px; padding: 10px 0 14px; border-top: 1px solid #f1f5f9; }.foot b { padding: 3px 7px; border-radius: 4px; color: #2563eb; background: #eff6ff; font: 10px ui-monospace, Consolas, monospace; }
</style>
