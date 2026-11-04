<script lang="ts">
  let { capacityMah, energyWh, uptimeMs, onReset }: { capacityMah: number; energyWh: number; uptimeMs: number; onReset: () => void } = $props()

  function duration(ms: number): string {
    const totalSeconds = Math.floor(ms / 1000)
    const hours = Math.floor(totalSeconds / 3600)
    const minutes = Math.floor((totalSeconds % 3600) / 60)
    const seconds = totalSeconds % 60
    return [hours, minutes, seconds].map((value) => String(value).padStart(2, '0')).join(':')
  }
</script>

<section class="accumulation">
  <h2>累计量</h2>
  <div class="metrics"><div><span>容量</span><strong>{capacityMah.toFixed(1)} <small>mAh</small></strong></div><div><span>能量</span><strong>{energyWh.toFixed(3)} <small>Wh</small></strong></div><div><span>运行时间</span><strong>{duration(uptimeMs)}</strong></div></div>
  <button onclick={onReset}>重置累计量</button>
</section>

<style>
  .accumulation { overflow: hidden; border: 1px solid #e4e7eb; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); }.accumulation h2 { margin: 0; padding: 12px 15px; border-bottom: 1px solid #e5e7eb; color: #1f2937; font-size: 14px; }.metrics { display: grid; gap: 8px; padding: 14px 15px 0; }.metrics div { display: flex; align-items: center; justify-content: space-between; gap: 10px; padding: 9px 10px; border: 1px solid #e4e7eb; border-radius: 4px; background: #fafafa; }.metrics span { color: #64748b; font-size: 12px; }.metrics strong { color: #18181b; font: 600 14px ui-monospace, Consolas, monospace; font-variant-numeric: tabular-nums; }.metrics small { color: #94a3b8; font-size: 10px; font-weight: 500; }.accumulation button { width: calc(100% - 30px); margin: 14px 15px 15px; padding: 8px 11px; border: 1px solid #d4d7dc; border-radius: 4px; color: #334155; background: #fff; cursor: pointer; font-size: 11px; }.accumulation button:hover { background: #f8fafc; }
</style>
