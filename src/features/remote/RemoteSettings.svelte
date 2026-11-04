<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { onMount } from 'svelte'
  import type { RemoteStatus } from '../../types'

  let { hideCurvePower, onToggleHideCurvePower }: { hideCurvePower: boolean; onToggleHideCurvePower: (checked: boolean) => void } = $props()

  let status = $state<RemoteStatus>({ running: false, url: '' })
  let port = $state(8765)
  let busy = $state(false)
  let error = $state('')
  let copied = $state(false)

  async function refresh() {
    try {
      status = await invoke<RemoteStatus>('remote_viewer_status')
    } catch {
      // backend command unavailable; leave current state
    }
  }
  onMount(() => void refresh())

  async function toggle() {
    if (busy) return
    busy = true
    error = ''
    copied = false
    try {
      status = status.running
        ? await invoke<RemoteStatus>('stop_remote_viewer')
        : await invoke<RemoteStatus>('start_remote_viewer', { port })
    } catch (reason) {
      error = String(reason)
    } finally {
      busy = false
    }
  }

  async function copyUrl() {
    if (!status.url) return
    try {
      await navigator.clipboard.writeText(status.url)
      copied = true
    } catch {
      // Clipboard API unavailable in the WebView: select the text instead.
      copied = false
    }
    setTimeout(() => (copied = false), 1500)
  }
</script>

<section class="remote-settings" aria-labelledby="remote-title">
  <h3 id="remote-title">远程查看</h3>
  <label class="check-row">
    <input type="checkbox" checked={status.running} disabled={busy} onchange={() => void toggle()} />
    <span><strong>开启局域网浏览器查看</strong><small>同一网络下的任意设备可用浏览器打开下方地址查看实时数据。未加密、无鉴权，仅限可信局域网。</small></span>
  </label>
  <label class="check-row sub">
    <input type="checkbox" checked={hideCurvePower} onchange={(event) => onToggleHideCurvePower((event.currentTarget as HTMLInputElement).checked)} />
    <span><strong>本机隐藏曲线和功率</strong><small>桌面端只显示电压 / 电流等数字读数，曲线与功率仅在远程页面显示。</small></span>
  </label>
  <label class="port-row">端口<input type="number" min="1" max="65535" step="1" value={port} disabled={busy || status.running} onchange={(event) => port = Math.max(1, Math.min(65535, Number((event.currentTarget as HTMLInputElement).value) || 8765))} /><small>默认 8765，运行中不可改</small></label>
  {#if status.running}
    <div class="remote-url"><code>{status.url}</code><button onclick={() => void copyUrl()}>{copied ? '已复制' : '复制'}</button></div>
    <p class="remote-note">首次运行若弹出 Windows 防火墙提示，请允许访问以让其他设备可连接。地址为本机局域网 IP，不同网络会变化。</p>
  {/if}
  {#if error}<p class="remote-error" role="alert">{error}</p>{/if}
</section>

<style>
  .remote-settings { padding: 15px; border-top: 1px solid #e5e7eb; }.remote-settings h3 { margin: 0 0 10px; color: #64748b; font-size: 11px; font-weight: 600; }.check-row { display: flex; align-items: flex-start; gap: 9px; cursor: pointer; }.check-row input { width: 16px; height: 16px; flex: none; margin: 1px 0 0; accent-color: #2563eb; }.check-row strong, .check-row small { display: block; }.check-row strong { color: #334155; font-size: 12px; }.check-row small { margin-top: 3px; color: #94a3b8; font-size: 10px; line-height: 1.35; }.check-row.sub { margin-top: 9px; }.port-row { display: grid; grid-template-columns: 1fr 92px; align-items: center; gap: 6px; margin-top: 12px; color: #64748b; font-size: 11px; }.port-row input { width: 88px; padding: 6px 7px; border: 1px solid #d4d7dc; border-radius: 4px; color: #1e293b; background: #fff; font: 12px ui-monospace, monospace; }.port-row small { grid-column: 2; color: #94a3b8; font-size: 10px; }.remote-url { display: flex; align-items: center; gap: 8px; margin-top: 13px; padding: 9px; border: 1px solid #e2e8f0; border-left: 3px solid #2563eb; background: #f8fafc; }.remote-url code { flex: 1; color: #1e293b; font: 11px/1.4 ui-monospace, monospace; overflow-wrap: anywhere; }.remote-url button { flex: none; padding: 5px 9px; border: 1px solid #cbd5e1; border-radius: 4px; color: #334155; background: #fff; cursor: pointer; font-size: 10px; }.remote-url button:hover { background: #eef2ff; }.remote-note { margin: 7px 0 0; color: #94a3b8; font-size: 10px; line-height: 1.4; }.remote-error { margin: 8px 0 0; color: #b91c1c; font-size: 10px; }
</style>
