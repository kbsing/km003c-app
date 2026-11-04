# Power-Pico 设备行为说明

本文记录 Power-Pico 的上位机协议与其在 Star monitor 中的接入方式。
内容全部从上游固件源码逆向，**开发时没有该设备，未经真机验证**。

- 上游：https://github.com/kingham/Power-Pico （`PC_Client/` 只有打包好的安装程序，无上位机源码）
- 固件：`software/Power_Pico/`，协议定义在 `User/user_AdcDataStrategy.{c,h}`

## 设备是什么

Power-Pico 是 STM32F411 便携 USB 电流表 / 低功耗分析仪：三档分流自动切换，
量程从 1 µA 到 5 A，支持 PPS 诱骗。它有两个角色：仪表（本文覆盖）与 PD 诱骗源（仅本机 UI，不上报主机）。

## 传输

- **USB CDC 虚拟串口**（Windows 显示为 COM 口），无需安装驱动。
- VID `0x0483` / PID `0x5740`，产品字符串「STM32 Virtual ComPort」——**ST 通用 CDC
  描述符**，任何 STM32 VCP 设备都是同样的 VID/PID，无法靠枚举区分是否 Power-Pico。
- 波特率对 CDC 无实际意义（本应用用 115200）。

## 数据流

设备上电后**持续向串口推送**数据包，无握手、无主机请求，主机只读。

### 数据包 `USB_ADC_Packet_t`（packed，总长 `11 + data_count*7` 字节）

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 2 | header | `0xAA 0x55` |
| 2 | 8 | timestamp | u64 LE 微秒；设备开机单调计数，断电清零 |
| 10 | 1 | data_count | 本包采样行数 |
| 11 | `data_count*7` | samples | 每行 7 字节，见下 |

每行 sample（7 字节，小端）：

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| 0 | 1 | range | 量程：`1`=Low，`2`=Mid，`3`=High |
| 1 | 2 | vol_adc | 电压 ADC 码 |
| 3 | 2 | cur_adc | 电流 ADC 码（当前量程那一路） |
| 5 | 2 | ref_adc | 参考 ADC 码 |

- 当前固件（v1.0.5）`data_count = ADC_TIMES = 10` → **81 字节/包**，约 **1000 包/秒
  （内部 10 kHz 采样）**。
- README 里「711 字节 = 100 组数据」是旧格式（`data_count = 100`），结构相同。
  实现用 `data_count` 字段动态定包长，两种固件都兼容。

## 物理量换算（固件默认标定）

- 电压：`V = vol_adc * (3000/4095 * 11) / 1000`（11 倍分压，3.0 V ADC 基准）
- 电流：`I(µA) = (cur_adc − ref_adc) * scale(range) + offset(range)`，scale 为
  `3.0 / 4096 / 50 / shunt`（50 = INA190 增益）：
  - Low：`shunt = 50 Ω` → ≈0.293 µA/LSB
  - Mid：`shunt = 0.5 Ω` → ≈29.3 µA/LSB
  - High：`shunt = 0.005 Ω` → ≈2929.7 µA/LSB
- 再扣电压分压漏流：`I −= V(mV) / 1100 µA`（与固件监视器 `Data_Monitor` 一致）。
- 默认 offset 为 0。**设备可在运行时校准**（`ADC_Set_Calibration`），主机无法查询
  校准值，用户设备若校准过会与上述默认值有偏差。

## 主机命令

- 只识别一个命令：向串口发送字符串 `update` → 设备写 EEPROM 标志、断开 USB、复位进入
  bootloader（Ymodem 固件升级）。本应用只读数据，不发送该命令。

## 在本应用中的接入行为

- `core/src/powerpico.rs` 实现 `Device` trait；枚举走 `serialport`，设备 id 为
  `powerpico:<COMx>`。
- 连接时校验 `0xAA 0x55` 数据流：非 Power-Pico 的 STM32 VCP 端口会在 `connect()`
  报「未返回有效数据流」被拒绝。
- 每次 `poll()` 保留最近一个完整包的**最后一行采样** → 约 50 Hz 进入采集管线，
  避免把 10 kHz 原始流灌进 IPC（前端显示也只需要 50 Hz）。
- `device_ms` 取设备微秒时间戳 `/1000`（设备单调计数，与 KM003C 语义一致，前端处理 u32 回绕）。
- **只有 V/I 两路**：`cc1_v/cc2_v = 0`、`temp_c = None`、无 PD 报文流。
  供电能力区与 PD 事件面板为空属正常，不是 bug。

## 未验证 / 待办

1. **无真机测试**：包解析、流重组、换算只做过单测（构造字节），未在真实设备上跑过。
   拿到设备后按第 4 节连 `pnpm tauri dev` 验证曲线与数值。
2. **默认标定值**未经真机核对；若数值整体有系统偏差，多半是设备被校准过，需要一份
   该设备的原始 ADC 码样本来修正，不要凭猜测改公式。
3. `data_count = 100`（711 字节）旧格式按 README 与固件注释推断，未在旧固件上验证。
4. **枚举误报**：任何 STM32 VCP 设备（不限于 Power-Pico）都会出现在设备选择器，
   靠连接时数据流校验区分。若误报困扰，可改为连接前先读流校验再列入列表。

## 参考

- 固件源码 `software/Power_Pico/User/user_AdcDataStrategy.{c,h}`
- 本应用后端 `core/src/powerpico.rs` 与其单测
