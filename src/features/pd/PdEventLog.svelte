<script lang="ts">
  import type { PdEvent } from '../../types'

  type Filter = '重点' | '协商' | '连接' | '异常' | '全部'
  let { events, showRaw, onToggleRaw }: { events: PdEvent[]; showRaw: boolean; onToggleRaw: (checked: boolean) => void } = $props()
  let filter = $state<Filter>('重点')

  let timelineEl = $state<HTMLDivElement | null>(null)
  // The currently-open <details> element (native DOM keeps its open state).
  let expandedEl: HTMLDetailsElement | null = null
  // Distance of the expanded item from the timeline's top, captured before a
  // re-render so it can be restored afterwards (new items must not shove the
  // open message out of view).
  let anchor: { top: number; valid: boolean } = { top: 0, valid: false }

  function onToggle(event: Event) {
    const target = event.target
    expandedEl = target instanceof HTMLDetailsElement && target.open ? target : null
  }

  $effect.pre(() => {
    void events
    if (expandedEl && timelineEl && expandedEl.isConnected) {
      anchor = {
        top: expandedEl.getBoundingClientRect().top - timelineEl.getBoundingClientRect().top,
        valid: true,
      }
    } else {
      anchor = { top: 0, valid: false }
    }
  })

  $effect(() => {
    void events
    if (anchor.valid && expandedEl && timelineEl && expandedEl.isConnected) {
      const now = expandedEl.getBoundingClientRect().top - timelineEl.getBoundingClientRect().top
      timelineEl.scrollTop = Math.max(timelineEl.scrollTop + (now - anchor.top), 0)
    }
  })

  const importantNames = ['Source_Capabilities', 'Request', 'Accept', 'Reject', 'Wait', 'Soft_Reset', 'PS_RDY', 'PPS_Status', 'DR_Swap', 'PR_Swap', 'VCONN_Swap', 'FR_Swap', 'Vendor_Defined', 'Vendor_Defined_Extended']
  const filteredEvents = $derived(events.filter((event) => matchesFilter(event, filter)))

  function matchesFilter(event: PdEvent, selected: Filter): boolean {
    if (selected === '全部') return true
    if (selected === '连接') return event.kind === 'attach' || event.kind === 'detach' || event.kind === 'conn-unknown' || event.kind === 'device-event'
    if (selected === '异常') return event.kind === 'unparsed' || event.kind === 'conn-unknown'
    if (selected === '协商') return event.kind !== 'message' || importantNames.includes(event.message?.standard_name ?? '')
    if (event.kind !== 'message') return true
    return importantNames.includes(event.message?.standard_name ?? '')
  }

  function typeLabel(event: PdEvent): string {
    if (event.kind === 'attach') return '已连接'
    if (event.kind === 'detach') return '已断开'
    if (event.kind === 'conn-unknown') return '未知连接'
    if (event.kind === 'device-event') return '设备事件'
    if (event.kind === 'unparsed') return '异常'
    if (event.message?.standard_name === 'Source_Capabilities') return '供电能力'
    if (event.message?.standard_name === 'Request') return '电源请求'
    if (event.message?.standard_name === 'Accept') return '已接受'
    if (event.message?.standard_name === 'Reject') return '已拒绝'
    if (event.message?.standard_name === 'Wait') return '等待处理'
    if (event.message?.standard_name === 'Soft_Reset') return '软复位'
    if (event.message?.standard_name === 'PS_RDY') return '供电就绪'
    if (event.message?.standard_name === 'PPS_Status') return '实时状态'
    if (event.message?.standard_name === 'Vendor_Defined') return '厂商消息'
    if (event.message?.standard_name === 'Vendor_Defined_Extended') return '厂商扩展'
    return 'PD 报文'
  }

  function headline(event: PdEvent): string {
    if (event.kind === 'attach') return '设备已连接'
    if (event.kind === 'detach') return '设备已断开'
    if (event.kind === 'conn-unknown' || event.kind === 'device-event' || event.kind === 'unparsed') return event.summary
    return event.message?.title ?? event.summary
  }

  function conclusion(event: PdEvent): string {
    const name = event.message?.standard_name
    if (name === 'Source_Capabilities') return `充电器提供 ${event.message?.object_count ?? 0} 个供电档位`
    if (name === 'Request') return event.detail.find((line) => line.includes('请求')) ?? '设备正在请求供电档位'
    if (name === 'Accept') return '充电器接受了本次请求'
    if (name === 'Reject') return '充电器拒绝了本次供电请求'
    if (name === 'Wait') return '充电器要求稍后再处理请求'
    if (name === 'Soft_Reset') return '协商状态已复位，等待重新发送供电能力'
    if (name === 'PS_RDY') return '供电已就绪，协商完成'
    if (name === 'PPS_Status') return event.detail[0] ?? '充电器上报 PPS 实时状态'
    if (name === 'Vendor_Defined') return event.detail.find((line) => line.includes('VDM')) ?? '已收到厂商自定义消息；载荷含义待确认'
    if (name === 'Vendor_Defined_Extended') return '已收到厂商扩展消息；载荷含义待确认'
    if (event.kind === 'attach') return '等待充电器发送供电能力'
    if (event.kind === 'detach') return '历史曲线已保留，等待重新连接'
    return event.detail[0] ?? event.summary
  }

  function dotClass(event: PdEvent): string {
    if (event.kind === 'attach') return 'green'
    if (event.kind === 'detach' || event.kind === 'conn-unknown' || event.kind === 'unparsed') return 'red'
    if (event.message?.standard_name === 'PS_RDY') return 'green'
    if (event.message?.standard_name === 'Request') return 'amber'
    return 'blue'
  }
</script>

<section class="pd-panel">
  <header class="section-title">
    <div><h2>PD 分析</h2><span class="subtitle">按时间顺序显示关键节点</span></div>
    <label><input type="checkbox" checked={showRaw} onchange={(event) => onToggleRaw((event.currentTarget as HTMLInputElement).checked)} /> 原始字节</label>
  </header>
  <nav class="filters" aria-label="PD 事件筛选">
    {#each ['重点', '协商', '连接', '异常', '全部'] as option}
      <button class:active={filter === option} onclick={() => filter = option as Filter}>{option}</button>
    {/each}
    <span class="count">显示 {filteredEvents.length} / {events.length}</span>
  </nav>
  <div class="timeline" bind:this={timelineEl} ontoggle={onToggle}>
    {#each filteredEvents as event (event)}
      <article class="event">
        <div class="rail"><span class={`dot ${dotClass(event)}`}></span><time>{event.device_ms} ms</time></div>
        <div class="event-body">
          <div class="event-top"><span class={`tag ${dotClass(event)}`}>{typeLabel(event)}</span>{#if event.sop !== null}<span class="sop">SOP{event.sop}{event.sop === 1 ? ' · 线缆' : ''}</span>{/if}</div>
          <h3>{headline(event)}</h3>
          <p class="conclusion">{conclusion(event)}</p>
          {#if event.message || event.detail.length > 0 || showRaw}
            <details><summary>查看技术详情</summary>
              {#if event.message}<div class="message-grid"><span>标准名称<strong>{event.message.standard_name}</strong></span><span>消息 ID<strong>{event.message.msg_id}</strong></span><span>PD 版本<strong>{event.message.spec_rev}</strong></span><span>角色<strong>{event.message.power_role} / {event.message.data_role}</strong></span><span>数据对象<strong>{event.message.object_count}</strong></span></div>{/if}
              {#if event.detail.length > 0}<div class="details"><span>解码详情</span>{#each event.detail as detail}<p>{detail}</p>{/each}</div>{/if}
              {#if showRaw}<div class="raw"><span>原始字节</span>{#if event.wire_hex}<code>PD 线上：{event.wire_hex}</code>{/if}<code>完整记录：{event.raw_hex}</code></div>{/if}
            </details>
          {/if}
        </div>
      </article>
    {:else}<div class="empty"><strong>暂无重点事件</strong><span>连接设备并开始 PD 协商后，关键节点会出现在这里。</span></div>{/each}
  </div>
</section>

<style>
  .pd-panel { display: flex; flex-direction: column; min-height: 250px; border: 1px solid #e4e7eb; border-radius: 6px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); overflow: hidden; }.section-title { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 14px; border-bottom: 1px solid #e5e7eb; }.section-title > div { display: flex; align-items: baseline; gap: 9px; }.section-title h2 { margin: 0; color: #111827; font-size: 15px; }.subtitle { color: #94a3b8; font-size: 10px; }.section-title label { color: #64748b; font-size: 10px; white-space: nowrap; }.section-title input { accent-color: #2563eb; vertical-align: -2px; }
  .filters { display: flex; align-items: center; gap: 5px; padding: 10px 12px 4px; }.filters button { padding: 5px 10px; border: 1px solid #dbe1e8; border-radius: 4px; color: #64748b; background: #fff; cursor: pointer; font-size: 10px; }.filters button.active { border-color: #2563eb; color: #fff; background: #2563eb; }.count { margin-left: auto; color: #94a3b8; font: 10px ui-monospace, monospace; }
  .timeline { flex: 1; min-height: 0; overflow: auto; padding: 4px 12px 8px; background: #f8fafc; }.event { display: grid; grid-template-columns: 76px 1fr; gap: 12px; padding: 9px 0; border-bottom: 1px solid #e5e7eb; }.event:last-child { border-bottom: 0; }.rail { position: relative; display: flex; flex-direction: column; align-items: flex-end; gap: 5px; padding-top: 3px; }.rail::after { content: ''; position: absolute; top: 16px; right: 4px; bottom: -16px; width: 1px; background: #dbe1e8; }.event:last-child .rail::after { display: none; }.dot { z-index: 1; width: 9px; height: 9px; border-radius: 50%; background: #3b82f6; box-shadow: 0 0 0 3px #dbeafe; }.dot.green { background: #10b981; box-shadow: 0 0 0 3px #d1fae5; }.dot.amber { background: #f59e0b; box-shadow: 0 0 0 3px #fef3c7; }.dot.red { background: #ef4444; box-shadow: 0 0 0 3px #fee2e2; }.rail time { color: #94a3b8; font: 9px ui-monospace, monospace; white-space: nowrap; }.event-body { min-width: 0; padding: 0 2px; }.event-top { display: flex; align-items: center; gap: 6px; }.tag, .sop { padding: 3px 7px; border-radius: 10px; color: #1d4ed8; background: #dbeafe; font-size: 10px; }.tag.green { color: #047857; background: #d1fae5; }.tag.amber { color: #b45309; background: #fef3c7; }.tag.red { color: #b91c1c; background: #fee2e2; }.sop { color: #64748b; background: #e2e8f0; }.event h3 { margin: 5px 0 2px; color: #111827; font-size: 14px; line-height: 1.3; }.conclusion { margin: 0; color: #475569; font-size: 12px; line-height: 1.35; }.event details { margin-top: 5px; }.event summary { color: #2563eb; cursor: pointer; font-size: 10px; }.message-grid { display: grid; grid-template-columns: repeat(5, minmax(72px, 1fr)); gap: 5px; margin-top: 7px; }.message-grid span { padding: 5px 6px; border: 1px solid #e2e8f0; border-radius: 4px; color: #64748b; background: #fff; font-size: 9px; }.message-grid strong { display: block; margin-top: 2px; color: #1e293b; font: 10px ui-monospace, monospace; overflow-wrap: anywhere; }.details { margin-top: 7px; padding: 7px 9px; border-left: 2px solid #cbd5e1; background: #fff; }.details > span, .raw > span { color: #64748b; font-size: 9px; }.details p { margin: 3px 0 0; color: #475569; font: 10px/1.45 ui-monospace, monospace; overflow-wrap: anywhere; }.raw { display: grid; gap: 3px; margin-top: 7px; }.raw code { color: #64748b; font: 9px/1.4 ui-monospace, monospace; overflow-wrap: anywhere; }.empty { display: flex; flex-direction: column; gap: 5px; align-items: center; padding: 36px 12px; color: #64748b; font-size: 11px; text-align: center; }.empty strong { color: #334155; font-size: 13px; }
  @media (max-width: 600px) { .subtitle { display: none; }.message-grid { grid-template-columns: repeat(2, minmax(80px, 1fr)); }.event { grid-template-columns: 62px 1fr; gap: 9px; }.rail time { font-size: 8px; }.filters { overflow-x: auto; }.count { display: none; } }
</style>
