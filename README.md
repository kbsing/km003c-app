# Star monitor

USB 功率计第三方上位机，支持多个品牌设备（当前：ChargerLAB POWER-Z KM003C、WITRN 维简）。
实时电压/电流曲线 + USB PD 报文解码。

**不需要认证**：全部功能运行在设备的 auth level 0，不涉及 AES、不读设备内存、不用 AdcQueue。

![Rust](https://img.shields.io/badge/Rust-1.85%2B-orange) ![Tauri](https://img.shields.io/badge/Tauri-2-blue) ![Svelte](https://img.shields.io/badge/Svelte-5-ff3e00)

---

## 功能

- **50 Hz 电压/电流曲线**，分辨率 1 mV / 1 mA，x 轴用设备侧单调毫秒时间戳；电流与功率显示幅值，`DIR +/-` 保留仪表原始方向
- **可配置曲线颜色 + 可开关功率曲线**：设置中可分别调整电压/电流/功率曲线颜色；功率曲线开关在曲线卡片图例行上，开启后在右侧叠加独立功率轴与功率曲线；偏好会持久化到本地，重启保留
- **USB PD 报文解码**：Source_Capabilities（含 PPS APDO）、Request（含 PPS RDO）、扩展消息（PPS_Status）、attach/detach、SOP' 线缆流量；跟踪常规协商和 Swap 控制交换
- **温度、CC1/CC2** 电压（来自 ADC，约 2 Hz）
- **WITRN 专属扩展**：连接维简设备时，顶部仪表区自动扩展为 4 格，显示 D+/D- 电压与
  累计 Ah/Wh（其余品牌保持 3 格）
- **本地累计量**：容量（`∫|I|dt`）、能量（`∫V×|I|dt`）和实际采样运行时间；断联空档不计入
- **热插拔恢复**：断开后自动重连；保留已有曲线并在新会话前插入断点
- **调试导出**：当前保留窗口的 V/I CSV，以及含 PD 解码、`raw`/`wire` hex 和传输统计的 JSON；曲线可导出为高分辨率 PNG（浅色/深色样式、三档分辨率、报告风格：可自定义标题、Voltage/Current/Power 三轴图例与时长/最大功率摘要）
- **局域网浏览器查看**：设置中开启后，同一网络下的任意设备可在浏览器打开指定地址（`http://<局域网IP>:8765/`）查看实时数据。远程页面为独立的移动端友好页面（大数字读数、铺满整宽的 V/I 曲线），不显示 PD 事件、不照搬桌面界面。未加密、无鉴权，仅限可信局域网；首次运行 Windows 可能弹防火墙提示

## 支持的设备

「连接设备」会先弹出设备选择器，枚举所有已连接的支持设备：

| 品牌 | VID:PID | 传输 | 测量源 |
|---|---|---|---|
| ChargerLAB POWER-Z KM003C | `5FC9:0063` | USB Bulk（WinUSB 免驱） | PD 块 50 Hz + ADC 块 ~2 Hz |
| WITRN 维简（K2/K2C/K3 等） | `0716:*` | USB HID（免驱，打开即推送） | general 报告（V/I/T/CC） |
| Power-Pico | `0483:5740` | USB CDC 虚拟串口 | ADC 包（V/I，10 kHz 采样） |

- 维简全系（K2、K2C、K3、KM20 等）共享 VID `0x0716` 与同一套 HID 报告协议，因此按厂商
  枚举，不限定单一型号。WITRN 的 PD 报告分帧与 CRC 尚未经真机验证；
  解码失败时会保留原始字节并标注错误，不会静默输出错值。
- Power-Pico 的 CDC 描述符是 ST 通用型号（`STM32 Virtual ComPort`），枚举按 VID/PID
  匹配、连接时校验 `0xAA 0x55` 数据流，不是 Power-Pico 的端口会被拒绝。它只上报 V/I，
  无 CC、温度与 PD 报文。换算用固件默认标定值，可在设备上运行时校准但主机读不到。
  **尚未经真机验证**（开发时无此设备），详见 [POWERPICO.md](POWERPICO.md)。
- 接口抽象见 `core/src/device.rs` 的 `Device` trait，新增品牌只需实现一个后端并注册进
  `list_devices` / `open_device`。

每条 PD 事件都保留原始字节（`raw` 完整记录 / `wire` 仅 PD 线上字节），解码结果可随时与 ground truth 对照。
导出范围与 UI 历史一致：本次连接的完整原始测量数组与最近 300 条 PD 事件。

---

## 快速开始

### 前置条件

| 项 | 版本 | 说明 |
|---|---|---|
| Rust | 1.85+ | `core` crate 用 edition 2024 |
| MSVC 链接器 | — | VS Build Tools 或 VS Community，cargo 通过 vswhere 自动发现 |
| Node | 20+ | |
| pnpm | 11+ | 设置项在 `pnpm-workspace.yaml` |
| WebView2 运行时 | — | Win11 自带；Win10 可能需要单独安装 |

Windows 上**不需要 Zadig 或任何驱动安装**。设备带 Microsoft OS 2.0 描述符，IF0 会自动绑定 `winusb.inf`。

### 运行

```bash
pnpm install
pnpm tauri dev
```

窗口出现后点 **Connect**。接上负载并拔插一次即可看到 PD 协商过程。

### 常用命令

```bash
pnpm check        # svelte-check 类型检查（重要，见下）
pnpm build        # 只构建前端到 dist/
cargo build       # 只构建 Rust
cargo tauri dev   # 完整开发模式（前端热重载）
pnpm tauri build  # release 打包：单文件 exe + NSIS 安装包
```

> **`pnpm build` 成功不代表代码正确。** Vite 用 esbuild 剥离类型时不做类型检查，
> 组件参数名写错这类问题它不会报。改完代码请跑 `pnpm check`。

### 发布打包

`pnpm tauri build` 产出两个可分发文件：

| 产物 | 路径 | 说明 |
|---|---|---|
| 安装包（推荐） | `target/release/bundle/nsis/Star monitor_<版本>_x64-setup.exe` | 向导式安装；目标机缺 WebView2 时自动引导下载 |
| 单文件 exe | `target/release/km003c-app.exe` | 免安装；目标机必须有 WebView2 运行时（Win11 自带） |

exe 未签名，首次运行时目标机的 SmartScreen 会拦截，需点击「更多信息 → 仍要运行」。
正式对外分发前建议补代码签名证书。

---

## 架构

```
core/                    独立 Rust crate，不依赖 Tauri
  protocol.rs            纯解析/编码，无 I/O（PD/VDM 解码、协商状态机、事件分帧）
  device.rs              USB bulk 传输 + 错误分类
  acquisition.rs         后台轮询线程 + 有界历史 + 批量输出

src-tauri/
  src/main.rs            DTO 转换 + 事件转发 + Tauri 命令 + 会话记录

src/                     Svelte 5 前端
  App.svelte             应用状态与页面编排
  features/dashboard/    顶栏、指标、曲线和状态弹窗
  features/pd/           PD 事件日志与供电能力
  features/settings/     设置弹窗（曲线颜色、会话记录）
  features/recording/    会话记录类型与设置
  features/remote/       局域网远程查看设置（RemoteSettings）
  lib/Chart.svelte       曲线（ECharts）与范围导航
  lib/ChartNavigator.svelte  底部导航条（ECharts dataZoom slider）
  lib/chartOption.ts     共享的 ECharts 配置（轴/序列/颜色/主题）
  lib/chartLoader.ts     ECharts 懒加载（动态 import，避免首屏阻塞）
  types.ts               DTO 与 UI 类型定义
```

### 数据流

```
20ms 轮询 ──► core::acquisition ──► 80ms 攒批 ──► Tauri emit("batch") ──► Svelte
   (Rust 线程)      ring buffer          IPC              约 12 次/秒
```

三个设计约束：

1. **采集循环必须在 Rust**。若由前端定时器驱动，webview 的调度抖动会叠加到 OS 抖动之上，污染时间戳。
2. **必须批量发射**。50 Hz 逐样本过 IPC 是浪费——人眼和屏幕都吃不下，而 JSON 序列化有成本。
3. **协议解释全在 Rust**。前端只渲染 Rust 给它的字符串，从不自己解析协议字节。这样解码逻辑单点维护，且 `core` 可被 CLI 或其他 GUI 复用。

`core` 不依赖 Tauri 和 serde，是刻意的——换 GUI 框架或做命令行工具时可直接复用。

---

## 实测协议结论

这些数字来自真机 KM003C（固件 V1.9.9）的实测，不是抄文档。

### 两个测量块的刷新率相差 250 倍

| 数据源 | 请求 | 中位刷新间隔 | 有效速率 | 编码分辨率 |
|---|---|---|---|---|
| **PD 12 字节测量块** | `attr=0x0010` | **21 ms** | 43.9 Hz | 1 mV / 1 mA |
| ADC 44 字节块 | `attr=0x0001` | 498 ms | 1.9 Hz | 1 µV / 1 µA（仅编码单位） |

同一设备、同一传输、同一会话下测得。对照指标：194 次轮询拿到 194 个不同的 PD 时间戳，
证明传输层能跑 50 Hz，ADC 的迟滞是设备侧行为而非轮询或解析问题。

**结论：实时曲线的 V/I 必须来自 PD 块，不能来自 ADC 块。** ADC 只用于温度。
设备本身不是精密仪器，实测精度基本在毫安级——ADC 块的 `1 µV / 1 µA` 只是上报的
编码单位，不代表真实测量精度，不能用于 µA 级电流测量（低电流精测模式不值得做）。

这一点上游研究仓库没有记录——它的 `adcqueue.md` 写 ADC "typical rate ~5 Hz" 并推荐
用 AdcQueue 做图表，而 AdcQueue 需要认证。

### 标定验证

同一时刻三个独立来源的数字互相印证：

| 来源 | 电压 | 电流 |
|---|---|---|
| Sink 请求（PPS RDO） | 9.70 V | 3.30 A 上限 |
| 充电器自报（PPS_Status 扩展消息） | 9.62 V | 3.20 A |
| 表实测（ADC） | 9.34 V | 3.23 A |

电压沿链路单调下降，符合物理；9.62→9.34 的降落在 3.2 A 下对应约 87 mΩ 线缆+接触电阻。
电流两个独立来源差 1%。

辅助验证：温度空载 23.9 °C → 20 W 负载 27.8 °C；`cc1=1.62 V` 落在 USB-C 规范
3.0 A Rp 带内（1.31–2.04 V），`cc2=0 V` 说明线缆未翻转。

### 与上游文档不符的三处

1. **attach/detach 事件码是 `0x11`/`0x12`**，而 `protocol_reference.md` 记的是 `0x21`/`0x22`。
   低位半字节一致（1=连接、2=断开），高位不同。已按实测值实现，未知码保留原值不猜测。
2. **存在 SOP' 线缆流量**（e-marker 线缆的 Discover_Identity），而 `pd_analysis.md` 称
   "All observed wrapped events were SOP (sop=0)"。
3. **未认证 PD 块可达 50 Hz**（见上）。

---

## 相关项目

- [km003c-protocol-research](https://github.com/okhsunrog/km003c-protocol-research) — 协议逆向研究，本项目的知识来源
- [km003c-rs](https://github.com/okhsunrog/km003c-rs) — 同作者的 Rust 实现（含 CLI 和 egui GUI）

本项目未使用 km003c-rs，`core` 是独立实现，以便完全控制轮询策略和错误分类。

## 可复用的解码库 `km003c-parse`

USB PD 报文解码可独立复用：`km003c-parse` crate 把「原始 PD 字节 → 中文/JSON 解释」
封装成库，同时编译为 **C-ABI DLL**（`km003c_parse.dll`），供 C / C++ / C# / Python /
Rust 等语言调用。详见其自带 [README](km003c-parse/README.md) 与 `examples/`。

- 输入：USB PD 线上字节（报头 + 对象 + 扩展载荷，不含尾部 CRC）
- 输出：JSON（`{"ok":true,"data":{standard_name, title_zh, summary_zh, detail[], ...}}`）
- 解码本体是 `core` 的纯函数；`main.rs` 与库共享同一套中文渲染函数，不会两份漂移

---

## 文档

- [使用说明.md](使用说明.md) — 面向使用者的安装、操作、导出与常见问题说明
- [POWERPICO.md](POWERPICO.md) — Power-Pico 设备协议与接入方式（基于上游固件逆向，未经真机验证）
- [km003c-parse/README.md](km003c-parse/README.md) — PD 解码库的用法与 C-ABI 说明

---

## 暂停、供电能力与私有协议

- 顶部“暂停显示”只暂停曲线、累计量和 PD 解码的界面更新，设备采集和后台解析继续运行；选择“继续显示”后会按顺序分帧补齐暂停期间的数据，补齐时界面仍可操作。系统状态卡中的“断开连接”才会真正停止采集并关闭设备。
- 供电能力区域展示最近一次收到的 USB PD `Source_Capabilities`。对于使用私有快充协议的设备，该列表仅供参考，不能据此认定设备当前实际申请或使用的供电档位。
- 应用会识别 USB PD `Vendor_Defined` / `Vendor_Defined_Extended` 消息。普通 `Vendor_Defined` 会显示标准 VDM 外层字段，例如 SVID、结构化类型、命令和 ACK/NAK/BUSY 状态；厂商扩展消息与未知载荷会保留原始数据，不会猜测字段含义。
- 这不代表应用已经支持 SuperVOOC 或其他品牌私有快充协议。当前设备接口提供的是 PD 事件流；若私有协商不在该流中，软件无法凭电压、电流推导出协议内容。

遇到厂商消息时，可打开 PD 面板的“原始字节”或导出 JSON，保存事件的 `wire_hex` 与 `raw_hex`。只有取得真实样本并通过回归测试验证后，才应为特定 SVID 增加载荷解析。

## PD 合约与会话记录

### 供电状态的证据等级

“充电器供电能力”并列展示两类协商信息：

- **可见 PD 请求**：已经在线上观察到的 RDO，橙色档位只表示请求正在等待结果。
- **已确认 PD 合约**：仅在同一条协商链已收到 `Request`、`Accept` 和 `PS_RDY` 后显示，绿色档位才表示已确认。`Reject`、`Wait`、新的 `Source_Capabilities` 或 `Soft_Reset` 会清理不再有效的状态。

KM003C 的 VBUS/IBUS 实测值仍在主仪表区显示。私有快充或不可见的协商场景应以实际测量为准，不能反向推断某个私有协议或 PD 档位；供电能力卡不重复显示该数值。

Fixed 和 PPS RDO 会显示可由 PDO 上下文明确得出的电压、电流。Battery/Variable PDO 或缺少 `Source_Capabilities` 上下文的请求不会伪造数值，只显示其 PDO 位置和信息不足状态。

### 完整会话记录

连接设备后点击顶栏「开始」才会绘制曲线并开始记录，否则各卡片照常显示但内容为空（曲线区
显示提示占位、PD 分析无事件、供电能力无档位）。数据在后台
写入本地磁盘的 NDJSON 会话文件（默认每批即时落盘，意外关闭也能恢复），前端内存只保留
滚动窗口。点击「结束」停止记录并隐藏曲线，此时才可导出 CSV / 诊断 JSON（从磁盘文件导出）。

- 默认电流阈值为 `10 mA`、持续时间为 `10 秒`。启用“满足条件自动停止”后，电流绝对值持续不高于阈值才停止**记录**，不会断开设备，也不会停止采集。
- 可选择把本机 Unix 开始时间写入文件头；可选择每批边采集边保存。关闭边采集保存时，缓存达到设定上限（默认 `100 MB`）或停止记录时会写入，缓存满不会丢弃样本。
- 文件保存在 Tauri 应用数据目录下的 `recordings/`，扩展名为 `.ndjson`。首行为含选项的 `header`，后续每行是完整的后端 `Batch` 快照，包含测量、PD 事件、供电能力、合约状态和采集统计，适合流式处理与问题复现。
- 意外关闭后重新打开会恢复上一次会话的数据（曲线与 PD 分析一起显示），可继续导出；点「开始」则开始新的记录。

### 完整曲线范围

曲线保留本次连接的完整原始测量历史（供曲线显示与 CSV 导出）；当数据很长时，画面会自动按时间桶抽样，但保留每段的 VBUS/IBUS 峰值和断线点，因此时间轴仍覆盖从连接开始到当前的完整范围。底部缩略导航条显示整个会话的电压走向，拖动选区两端可缩放范围，拖动选区中部可平移范围。在曲线上滚轮可缩放（以光标为中心，向上放大、向下缩小，受会话范围与 0.2 s 最小窗口钳制），缩放后会同步到底部选区；点击图表标题栏的“全览”可回到完整时间范围。

CC 线路、系统状态和累计量收纳在曲线标题栏的“状态”弹窗中，断开连接和重置累计量操作也在该弹窗内。

画面抽样只影响显示性能，不改变测量或录制精度。极端超长会话仍会占用前端内存；需要长期保存完整原始数据时，应同时启用 NDJSON 会话记录。

## 运行限制

请使用桌面模式运行：

```bash
pnpm tauri dev
```

不要单独启动浏览器中的 Vite 页面来连接设备：浏览器页面没有 Tauri 的 USB 调用能力。开发时 Vite 固定使用端口 `1420`；如出现端口被占用错误，停止遗留的 Vite/`pnpm dev` 进程后再运行 `pnpm tauri dev`，不要改用其他端口。

## 局域网远程查看

设置 → 「远程查看」里开启「局域网浏览器查看」，服务监听 `0.0.0.0:<端口>`（默认 `8765`，运行中不可改端口）。开启后设置面板会显示访问地址，例如 `http://192.168.1.10:8765/`；把该地址发给同一网络下的任意设备（手机、平板、另一台电脑），用浏览器打开即可看到实时数据。

- 远程页面是独立的移动端友好页面：顶部连接状态与 V/A/W 大数字读数、电压/电流曲线（ECharts，双轴 + 十字光标提示）。数据不足 60 秒时曲线自动铺满整宽显示全部数据，之后滚动显示最近 60 秒。**它不是桌面界面的移植，不显示 PD 事件**，功能刻意精简。
- 设置里可勾选「本机隐藏曲线和功率」：开启后**桌面端**不显示曲线与功率读数（曲线区收起、读数区收为两列），曲线和功率仍通过远程页面查看。适合桌面只作为采集端、用手机/平板远程观察的场景。
- 数据经浏览器轮询 `/state` 获取（约每 400 ms），后端维护最近约 10 秒样本的滚动快照，因此轮询不会丢数据。
- 服务**未加密、无鉴权**，仅供可信局域网使用。首次启动若 Windows 防火墙弹窗，需允许访问才能让其他设备连接。
- 停止「远程查看」或关闭应用后服务随即关闭。
