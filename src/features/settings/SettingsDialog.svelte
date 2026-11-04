<script lang="ts">
  import RecordingSettings from '../recording/RecordingSettings.svelte'
  import RemoteSettings from '../remote/RemoteSettings.svelte'
  import type { RecordingOptions, RecordingStatus } from '../recording/types'
  import type { CurveColors } from '../../types'

  let { open, curveColors, recordingOptions, recordingStatus, hideCurvePower, onClose, onCurveColorsChange, onRecordingOptionsChange, onToggleHideCurvePower }: { open: boolean; curveColors: CurveColors; recordingOptions: RecordingOptions; recordingStatus: RecordingStatus | null; hideCurvePower: boolean; onClose: () => void; onCurveColorsChange: (colors: CurveColors) => void; onRecordingOptionsChange: (options: RecordingOptions) => void; onToggleHideCurvePower: (checked: boolean) => void } = $props()

  function handleKeydown(event: KeyboardEvent) {
    if (open && event.key === 'Escape') onClose()
  }

  function updateColor(key: keyof CurveColors, event: Event) {
    onCurveColorsChange({ ...curveColors, [key]: (event.currentTarget as HTMLInputElement).value })
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div class="backdrop">
    <button class="backdrop-dismiss" onclick={onClose} aria-label="关闭设置"></button>
    <dialog open class="settings-dialog" aria-labelledby="settings-title">
      <header><h2 id="settings-title">设置</h2><button class="close" onclick={onClose} aria-label="关闭设置" title="关闭">&#10005;</button></header>
      <div class="setting-group">
        <h3>曲线</h3>
        <label class="setting-row"><span><strong>电压曲线</strong><small>VBUS 曲线与左轴颜色</small></span><input type="color" value={curveColors.voltage} onchange={(event) => updateColor('voltage', event)} aria-label="电压曲线颜色" /></label>
        <label class="setting-row"><span><strong>电流曲线</strong><small>IBUS 曲线与右轴颜色</small></span><input type="color" value={curveColors.current} onchange={(event) => updateColor('current', event)} aria-label="电流曲线颜色" /></label>
        <label class="setting-row"><span><strong>功率曲线</strong><small>P 曲线与功率轴颜色</small></span><input type="color" value={curveColors.power} onchange={(event) => updateColor('power', event)} aria-label="功率曲线颜色" /></label>
        <label class="setting-row"><span><strong>温度曲线</strong><small>温度曲线与温度轴颜色</small></span><input type="color" value={curveColors.temperature} onchange={(event) => updateColor('temperature', event)} aria-label="温度曲线颜色" /></label>
      </div>
      <RecordingSettings options={recordingOptions} status={recordingStatus} disabled={recordingStatus?.active ?? false} onChange={onRecordingOptionsChange} />
      <RemoteSettings {hideCurvePower} onToggleHideCurvePower={onToggleHideCurvePower} />
    </dialog>
  </div>
{/if}

<style>
  .backdrop { position: fixed; z-index: 20; inset: 0; display: grid; place-items: center; padding: 12px; background: rgb(15 23 42 / 28%); overflow: auto; }.backdrop-dismiss { position: absolute; inset: 0; padding: 0; border: 0; background: transparent; cursor: default; }  .settings-dialog { position: relative; z-index: 1; display: block; width: min(600px, 100%); max-height: calc(100vh - 24px); margin: 0; padding: 0; border: 1px solid #d9dee4; border-radius: 6px; background: #fff; box-shadow: 0 18px 45px rgb(15 23 42 / 22%); overflow: auto; overscroll-behavior: contain; }.settings-dialog header { position: sticky; top: 0; z-index: 2; display: flex; align-items: center; justify-content: space-between; padding: 13px 15px; border-bottom: 1px solid #e5e7eb; background: #fff; }.settings-dialog h2 { margin: 0; color: #1f2937; font-size: 15px; }.close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; border: 1px solid #d4d7dc; border-radius: 4px; color: #64748b; background: #fff; cursor: pointer; font-size: 13px; }.close:hover { color: #1f2937; background: #f8fafc; }.setting-group { padding: 15px; }.setting-group h3 { margin: 0 0 10px; color: #64748b; font-size: 11px; font-weight: 600; }.setting-row { display: flex; align-items: center; justify-content: space-between; gap: 14px; padding: 11px; border: 1px solid #e2e8f0; border-radius: 4px; cursor: pointer; }.setting-row + .setting-row { margin-top: 8px; }.setting-row span { min-width: 0; }.setting-row strong, .setting-row small { display: block; }.setting-row strong { color: #334155; font-size: 12px; }.setting-row small { margin-top: 4px; color: #94a3b8; font-size: 10px; line-height: 1.4; }.setting-row input[type="color"] { width: 46px; height: 28px; flex: none; padding: 2px; border: 1px solid #d4d7dc; border-radius: 4px; background: #fff; cursor: pointer; }
</style>
