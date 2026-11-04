<script lang="ts">
  // 传输层诊断面板：当前已禁用（2026-08）。采集线程仍在统计
  // polls / empty / timeout / stall / dropped 等计数器，但面板暂未接入
  // 主界面。代码保留不删除，重新启用时恢复下方模板即可。
  import type { Stats } from '../../types'
  let { stats }: { stats: Stats | null } = $props()
  // 禁用期间保持 props 被引用，避免 svelte-check 报未使用。
  $effect(() => {
    void stats
  })
</script>

<!--
<aside class="transport">
  <div class="heading"><span>SEC.05</span><h2>传输状态 <em>TRANSPORT</em></h2></div>
  {#if stats}
    <dl>
      <div><dt>POLLS</dt><dd>{stats.polls}</dd></div><div><dt>SAMPLES</dt><dd>{stats.ok}</dd></div><div><dt>PD EVENTS</dt><dd>{stats.events_seen}</dd></div><div><dt>LARGEST</dt><dd>{stats.largest_response} B</dd></div>
      <div><dt>EMPTY ACK</dt><dd>{stats.empty}</dd></div><div class:bad={stats.real_errors > 0}><dt>FAULTS</dt><dd>{stats.real_errors}</dd></div><div class:bad={stats.dropped > 0}><dt>DROPPED</dt><dd>{stats.dropped}</dd></div>
    </dl>
    <p><code>EMPTY ACK</code> 是正常零长度确认。<code>DROPPED</code> 增长表示 UI 未及时消费采样。</p>
  {:else}
    <div class="waiting">等待采集会话</div>
  {/if}
</aside>
-->

<!--
<style>
  .transport { min-width: 0; padding: 15px; border: 1px solid #bdc7c3; background: #f0f4f2; }.heading { display: flex; align-items: baseline; gap: 8px; padding-bottom: 10px; border-bottom: 1px solid #bdc7c3; }.heading span { color: #9a6029; font: 9px ui-monospace, monospace; letter-spacing: .12em; }.heading h2 { margin: 0; color: #26312e; font: 600 14px Georgia, "Songti SC", serif; letter-spacing: .08em; }.heading em { color: #71807b; font: 9px ui-monospace, monospace; letter-spacing: .12em; font-style: normal; }dl { margin: 11px 0 0; }dl div { display: flex; justify-content: space-between; gap: 10px; padding: 6px 0; border-bottom: 1px dashed #cbd4d0; font: 10px ui-monospace, monospace; letter-spacing: .06em; }dt { color: #6c7974; }dd { margin: 0; color: #26312e; font-variant-numeric: tabular-nums; font-weight: 600; }.bad dt, .bad dd { color: #a74539; }p, .waiting { margin: 13px 0 0; color: #75827d; font-size: 10px; line-height: 1.55; }code { color: #536661; font-family: ui-monospace, monospace; }.waiting { padding: 16px 0; }
</style>
-->
