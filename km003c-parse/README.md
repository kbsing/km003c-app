# km003c-parse

把 USB PD 报文解码成可读（中文）JSON 的库，可直接被 Rust 调用，也编译为
C-ABI DLL（`km003c_parse.dll`），供 C / C++ / C# / Python / Go 等语言使用。

解码本体来自同仓库的 `km003c-core`（无 Tauri / 无 serde 依赖）；本 crate 负责
**展示层**：中文消息名、摘要行、详情行，以及稳定的 C ABI（JSON 进出）。

> 验证边界：主流路径（Source_Capabilities / Request / PPS 扩展 / VDM 等）经
> 真机验证；部分（WITRN / Power-Pico、个别扩展消息）仅 hex 核对。库的使用者应
> 把这条边界转述给自己的用户。

## Rust 用法

```toml
[dependencies]
km003c-parse = { git = "<repo>", package = "km003c-parse" }
```

```rust
use km003c_parse::parse_pd_wire;

let wire = [0x01, 0x11, 0x64, 0x21, 0xDC, 0xC0]; // Source_Capabilities, 一个 PPS PDO
let decoded = parse_pd_wire(&wire)?;
println!("{}", decoded.to_json_string());
```

## 构建 DLL（Windows）

```bash
cargo build -p km003c-parse --release
# 产物：target/release/km003c_parse.dll（+ .lib 导入库）
```

输入是 USB PD 线上字节：**报头 + 对象 + 扩展载荷，不含尾部 4 字节 CRC**（与
`km003c-core::decode_pd_wire` 一致）。

## C ABI

头文件：`include/km003c_parse.h`。所有返回字符串的函数都需用 `km003c_free` 释放。

| 函数 | 说明 |
|---|---|
| `char* km003c_parse_pd_wire(const uint8_t*, size_t)` | 按字节解析，返回 JSON |
| `char* km003c_parse_pd_hex(const char*)` | 按 hex 字符串解析（允许空格 / `0x`） |
| `void km003c_free(char*)` | 释放返回的字符串 |
| `char* km003c_version(void)` | 版本号字符串 |

返回 JSON 结构：

```json
{ "ok": true, "data": {
  "standard_name": "Source_Capabilities", "title_zh": "源端能力（Source_Capabilities）",
  "summary_zh": "源端能力（Source_Capabilities） · 消息 ID 0 · PD 2.0 · 供电端 · 下行端",
  "msg_id": 0, "spec_rev": "2.0", "power_role": "供电端", "data_role": "下行端",
  "object_count": 1, "extended": false,
  "detail": ["PPS 档 #1：3.30–11.00 V，5.00 A"],
  "objects": [3233955172], "wire_hex": "01116421dcc0" } }
```

失败时返回 `{"ok": false, "error": "..."}`。Rust panic 不会穿过 FFI 边界，会被捕获
并转为错误 JSON。

## 各语言调用示例

`examples/` 目录：

| 语言 | 文件 | 说明 |
|---|---|---|
| C | `c_example.c` | `LoadLibrary` 动态加载 DLL |
| Python | `python_example.py` | `ctypes` |
| C# | `csharp_example.cs` | `P/Invoke` |
| Rust | `rust_example.rs` | 直接依赖 crate（`cargo run -p km003c-parse --example rust_example`） |

## 测试

```bash
cargo test -p km003c-parse
```
