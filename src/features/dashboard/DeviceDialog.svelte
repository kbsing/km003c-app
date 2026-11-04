<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import type { DeviceInfo } from '../../types'

  let { open, onSelect, onClose }: { open: boolean; onSelect: (deviceId: string) => void; onClose: () => void } = $props()

  let devices = $state<DeviceInfo[]>([])
  let selectedId = $state('')
  let error = $state('')
  let loading = $state(false)

  async function refresh() {
    loading = true
    error = ''
    try {
      devices = await invoke<DeviceInfo[]>('list_devices')
      if (!selectedId || !devices.some((device) => device.id === selectedId)) {
        selectedId = devices[0]?.id ?? ''
      }
      if (devices.length === 0) error = '未发现受支持的设备（KM003C 或 WITRN）。请确认已连接，然后点击「刷新」。'
    } catch (reason) {
      error = String(reason)
    } finally {
      loading = false
    }
  }

  $effect(() => {
    if (open) void refresh()
  })

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') onClose()
  }

  function confirm() {
    if (selectedId) onSelect(selectedId)
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="backdrop">
    <button class="backdrop-dismiss" onclick={onClose} aria-label="关闭设备选择"></button>
    <dialog open class="device-dialog" aria-labelledby="device-dialog-title">
      <header><h2 id="device-dialog-title">选择设备</h2><button class="close" onclick={onClose} aria-label="关闭设备选择" title="关闭">&#10005;</button></header>
      <div class="device-list">
        {#each devices as device, index (device.id)}
          <label class="device-row">
            <input type="radio" name="device" value={device.id} checked={selectedId === device.id} onchange={() => selectedId = device.id} />
            <span class="device-main"><strong>{device.kind}</strong><small>{device.name}</small></span>
            <span class="device-serial">#{index + 1} · {device.serial}</span>
          </label>
        {/each}
        {#if loading}<p class="hint">正在扫描设备…</p>{/if}
        {#if error}<p class="error" role="status">{error}</p>{/if}
      </div>
      <footer class="actions">
        <button class="secondary" onclick={() => void refresh()} disabled={loading}>刷新</button>
        <span class="spacer"></span>
        <button class="secondary" onclick={onClose}>取消</button>
        <button class="primary" onclick={confirm} disabled={!selectedId}>连接</button>
      </footer>
    </dialog>
  </div>
{/if}

<style>
  .backdrop { position: fixed; z-index: 20; inset: 0; display: grid; place-items: center; padding: 20px; background: rgb(15 23 42 / 28%); }.backdrop-dismiss { position: absolute; inset: 0; padding: 0; border: 0; background: transparent; cursor: default; }
  .device-dialog { position: relative; z-index: 1; width: min(460px, 100%); margin: 0; padding: 0; border: 1px solid #d9dee4; border-radius: 6px; background: #fff; box-shadow: 0 18px 45px rgb(15 23 42 / 22%); }.device-dialog header { display: flex; align-items: center; justify-content: space-between; padding: 13px 15px; border-bottom: 1px solid #e5e7eb; }.device-dialog h2 { margin: 0; color: #1f2937; font-size: 15px; }.close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; border: 1px solid #d4d7dc; border-radius: 4px; color: #64748b; background: #fff; cursor: pointer; font-size: 13px; }.close:hover { color: #1f2937; background: #f8fafc; }
  .device-list { padding: 15px; display: flex; flex-direction: column; gap: 8px; }.device-row { display: flex; align-items: center; gap: 12px; padding: 11px; border: 1px solid #e2e8f0; border-radius: 4px; cursor: pointer; }.device-row:has(input:checked) { border-color: #2563eb; background: #eff6ff; }.device-row input[type="radio"] { width: 15px; height: 15px; flex: none; accent-color: #2563eb; }.device-main { min-width: 0; flex: 1; }.device-main strong, .device-main small { display: block; }.device-main strong { color: #334155; font-size: 12px; }.device-main small { margin-top: 3px; color: #94a3b8; font-size: 10px; }.device-serial { flex: none; color: #64748b; font-size: 10px; }
  .hint { margin: 0; color: #94a3b8; font-size: 11px; }.error { margin: 0; color: #b91c1c; font-size: 11px; line-height: 1.5; }
  .actions { display: flex; align-items: center; gap: 8px; padding: 13px 15px; border-top: 1px solid #e5e7eb; }.actions .spacer { flex: 1; }.actions button { padding: 7px 13px; border: 1px solid #d4d7dc; border-radius: 4px; color: #334155; background: #fff; cursor: pointer; font-size: 11px; }.actions button:hover:not(:disabled) { background: #f8fafc; }.actions button.primary { border-color: #2563eb; color: #fff; background: #2563eb; }.actions button.primary:hover:not(:disabled) { background: #1d4ed8; }.actions button:disabled { cursor: default; opacity: .45; }
</style>
