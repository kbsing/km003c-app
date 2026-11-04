// Lazy loader for the ECharts-dependent chart modules.
//
// Keeping the ~600 KB of chart code out of the startup bundle means first paint
// does not wait for ECharts to parse. The chart initializes a moment later —
// while the user is connecting a device — so nothing on screen blocks.
// Everything in `chartOption.ts` (ECharts + option builders) becomes its own
// dynamic chunk; components call `loadChartDeps()` and wait.
export type ChartDeps = typeof import('./chartOption')

let pending: Promise<ChartDeps> | null = null

export function loadChartDeps(): Promise<ChartDeps> {
  pending ??= import('./chartOption')
  return pending
}
