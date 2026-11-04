<script lang="ts">
  import type { PdContract, PdRequest, PowerProfile } from '../../types'
  let { profiles, contract }: { profiles: PowerProfile[]; contract: PdContract } = $props()
  const activePdo = $derived(contract.active_contract?.pdo_position ?? null)
  const requestedPdo = $derived(contract.visible_request?.pdo_position ?? null)

  function label(kind: PowerProfile['kind']): string {
    if (kind === 'pps') return 'PPS'
    if (kind === 'fixed') return '固定档'
    if (kind === 'battery') return '电池档'
    if (kind === 'variable') return '可变档'
    return 'APDO'
  }

  function requestValue(request: PdRequest | null, empty: string): string {
    if (!request) return empty
    if (request.voltage_v !== null && request.current_a !== null) return `${request.voltage_v.toFixed(2)} V / ${request.current_a.toFixed(2)} A`
    return `PDO #${request.pdo_position}（${request.kind === 'other' ? '未展开' : '上下文不足'}）`
  }

  function phaseLabel(phase: PdContract['phase']): string {
    if (phase === 'request-pending') return '等待接受'
    if (phase === 'accepted-waiting-ps-rdy') return '等待供电就绪'
    if (phase === 'offered') return '已收到供电能力'
    if (phase === 'soft-reset') return '协商已复位'
    if (phase === 'control-pending' || phase === 'control-accepted') return '控制消息交互中'
    return '未观察到活动协商'
  }
</script>

<section class="power-profiles">
  <div class="profiles-title"><h2>充电器供电能力</h2><span>来自已解析的 Source_Capabilities</span></div>
  <div class="contract-overview">
    <article><span>可见 PD 请求</span><b>{requestValue(contract.visible_request, '未观察到')}</b><small>{phaseLabel(contract.phase)}</small></article>
    <article class="active"><span>已确认 PD 合约</span><b>{requestValue(contract.active_contract, '尚未确认')}</b><small>仅在 Accept 后收到 PS_RDY 才标记</small></article>
  </div>
  <p class="private-protocol-note">若设备使用私有快充协议，本区域仅显示最近检测到的 PD 供电能力，仅供参考，不代表设备当前实际申请或使用的供电档位。</p>
  {#if profiles.length > 0}
    <div class="profile-list">
      {#each profiles as profile}
        <article class:pps={profile.kind === 'pps'} class:requested={profile.index === requestedPdo} class:selected={profile.index === activePdo} class="profile">
          <div class="profile-top"><span class="icon">{profile.kind === 'pps' ? '↕' : 'ϟ'}</span><span class="kind">{label(profile.kind)}</span><small>#{String(profile.index).padStart(2, '0')}</small></div>
          <strong class="voltage">{profile.min_v === null ? profile.max_v.toFixed(2) : `${profile.min_v.toFixed(2)} - ${profile.max_v.toFixed(2)}`} <small>V</small></strong>
          <div class="limits">{#if profile.max_a !== null}<span>最大 <b>{profile.max_a.toFixed(2)} A</b></span>{/if}{#if profile.max_w !== null}<span><b>{profile.max_w.toFixed(1)} W</b></span>{/if}{#if profile.kind === 'pps'}<span>可调输出</span>{/if}{#if profile.index === requestedPdo}<span class="pending-request">请求中</span>{/if}{#if profile.index === activePdo}<span class="active-contract">已确认</span>{/if}</div>
        </article>
      {/each}
    </div>
  {:else}
    <div class="empty">等待充电器上报供电能力</div>
  {/if}
</section>

<style>
  .power-profiles { padding: 12px 14px; border: 1px solid #dbe3f0; border-radius: 8px; background: #fff; box-shadow: 0 1px 3px rgb(15 23 42 / 5%); }.profiles-title { display: flex; align-items: baseline; gap: 10px; margin-bottom: 7px; }.profiles-title h2 { margin: 0; color: #334155; font-size: 14px; }.profiles-title span { color: #94a3b8; font-size: 10px; }.contract-overview { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-bottom: 8px; }.contract-overview article { display: grid; grid-template-columns: minmax(78px, .8fr) minmax(126px, 1.1fr) minmax(132px, 1.25fr); align-items: center; gap: 8px; min-width: 0; min-height: 34px; padding: 6px 9px; border: 1px solid #dbe3f0; border-radius: 4px; background: #f8fafc; }.contract-overview article.active { border-color: #bbf7d0; background: #f0fdf4; }.contract-overview span, .contract-overview b, .contract-overview small { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.contract-overview span, .contract-overview small { color: #64748b; font-size: 9px; }.contract-overview b { color: #1e293b; font: 600 11px ui-monospace, monospace; }.private-protocol-note { margin: 0 0 10px; padding: 6px 9px; border-left: 3px solid #f59e0b; color: #92400e; background: #fffbeb; font-size: 10px; line-height: 1.35; }.profile-list { display: grid; grid-template-columns: repeat(auto-fit, minmax(145px, 1fr)); gap: 10px; }.profile { min-width: 0; padding: 11px; border: 1px solid #dbe3f0; border-radius: 8px; background: #f8fafc; }.profile.pps { border-color: #c4b5fd; background: #faf5ff; }.profile.requested { border: 2px solid #f59e0b; background: #fffbeb; box-shadow: 0 0 0 2px #fef3c7; }.profile.selected { border: 2px solid #16a34a; background: #f0fdf4; box-shadow: 0 0 0 2px #dcfce7; }.profile-top { display: flex; align-items: center; gap: 7px; }.icon { display: grid; place-items: center; width: 27px; height: 27px; border-radius: 7px; color: #2563eb; background: #dbeafe; font-size: 17px; }.pps .icon { color: #7c3aed; background: #ede9fe; }.kind { padding: 3px 7px; border-radius: 10px; color: #2563eb; background: #dbeafe; font: 10px ui-monospace, monospace; }.pps .kind { color: #7c3aed; background: #ede9fe; }.profile-top small { margin-left: auto; color: #94a3b8; font: 10px ui-monospace, monospace; }.voltage { display: block; margin-top: 12px; color: #0f172a; font: 600 23px ui-monospace, Consolas, monospace; }.voltage small { color: #94a3b8; font-size: 11px; font-weight: 500; }.limits { display: flex; justify-content: space-between; gap: 6px; margin-top: 10px; color: #94a3b8; font-size: 10px; }.limits b { color: #2563eb; font: 600 12px ui-monospace, Consolas, monospace; }.active-contract { color: #15803d; font-weight: 600; }.pending-request { color: #b45309; font-weight: 600; }.empty { padding: 18px 0 5px; color: #94a3b8; font-size: 11px; }
  @media (max-width: 720px) { .contract-overview { grid-template-columns: 1fr; }.contract-overview article { grid-template-columns: minmax(78px, .8fr) minmax(126px, 1.1fr) minmax(100px, 1fr); } }
</style>
