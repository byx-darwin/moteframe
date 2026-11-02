# I²C 扩展、兼容性与资源成本

本阶段保留单控制器、7 位地址、8 位寄存器指针及 STOP 分隔读取，新增显式地址校验、
严格完成检查和资源取回。不改变旧 Line/new/read/I2cError 或对象字段，无新依赖、
heap、unsafe、隐藏重试、故障锁存或自动恢复。

## 选择读取入口

| API | 地址 | 完成语义 |
| --- | --- | --- |
| `read(u8, register, out)` | 原始 `0x00..=0x7f` | 尝试 STOP、等待 SCL、释放 master 输出；保持旧行为 |
| `read_normal(NormalAddress7, register, out)` | 仅 `0x08..=0x77` | 与旧 read 相同 |
| `read_checked(NormalAddress7, register, out)` | 仅普通目标地址 | 指针阶段和最终阶段 STOP 都额外观察实际 SCL/SDA 为高 |

新设备推荐 read_checked。无需 Cargo feature 或运行时配置开关：调用的方法决定能力，
未用路径可由链接器裁剪。NormalAddress7 字段私有，new 校验范围、get 返回右对齐地址。

```rust
use moteframe_i2c::{BitBangI2c, CheckedReadError, Line, NormalAddress7};

fn read_two<C: Line, D: Line, F: FnMut()>(
    bus: &mut BitBangI2c<C, D, F>,
) -> Result<[u8; 2], CheckedReadError> {
    let address = NormalAddress7::new(0x15).map_err(CheckedReadError::Protocol)?;
    let mut bytes = [0; 2];
    bus.read_checked(address, 0x01, &mut bytes)?;
    Ok(bytes)
}
```

原始地址可能具有特殊系统语义。例如 read(0x00,0x06,out) 的第一阶段会发送
General Call + 软件复位命令，支持者可能复位，后续读取失败也不能撤销。封闭系统
只有确认特殊用途不会被使用时才应有意复用 reserved 地址。参见
[NXP UM10204 Rev.7 §3.1.12–3.1.14](https://www.nxp.com/docs/en/user-guide/UM10204.pdf)。

## 完成、错误和预算

CheckedReadError 独立于旧闭合错误枚举：Protocol(I2cError) 保留原始错误；BusNotIdle
表示 STOP 后未在预算内观察到两根线都为高。每个 STOP 先执行原 SCL 等待和输出释放，
然后最多观察 polls 次；每次观察失败调用一次半周期 delay，包括最后一次失败。
指针 STOP 检查失败后不发送读取地址，仍尝试最终清理；始终保留首次错误。

两线为高是软件观察，不能代替 STOP 波形、电气时序或恢复资格。目标可以在检查后
重新拉低线路。实机仍需验证上拉、电容、速率及 START/STOP 建立保持时间。参见
[NXP START/STOP 和 bus clear 定义](https://www.nxp.com/docs/en/user-guide/UM10204.pdf)。

空输出在 I/O 前拒绝；有效请求 Busy 也会尝试 STOP，因此 Busy 不是零 I/O。任何错误
都应丢弃整个输出，包括已填满但最终 STOP 失败的情况。没有故障锁存或隐藏 retry，
调用方必须调查总线/目标状态，再决定恢复；资源取回或重新构造不代表硬件已修复。

polls 是每个等待/额外检查的次数，不是毫秒 deadline。旧成功 n 字节读取有 31+9n 次
SCL 等待，无 stretch 时 64+18n 次 delay；严格路径再增加两个 STOP 检查。Line 和 delay
本身必须有限完成，循环无法中断阻塞调用。release 必须关闭下拉而非主动拉高，high
必须读取实际输入而非输出锁存，调用方保证单控制器独占和开漏配置。

## 资源取回

- try_new(clock,data,delay,polls) 成功时释放 SDA/SCL；零预算时无 I/O，InitError 保留全部原对象。
- InitError::error 借用原因；into_parts 返回 (I2cError,clock,data,delay)，不要求 pin/closure 实现 Debug。
- BitBangI2c::into_parts 返回 (clock,data,delay)，不执行 I/O、复位或 bus clear。
- 支持 owned 或显式借用对象；自定义 Line 的引用转发由调用方提供，没有新增 blanket impl。
- 原 new 在失败时仍消费并 drop 参数，保持兼容。取回后由设备工程显式调查/修复并重建。

## 固定 Cortex-M0 实测

make i2c-compat 已纳入 make check 和现有 CI。固定 Rust 工具链、thumbv6m-none-eabi、
opt-level=s、fat LTO、单 codegen unit、panic=abort；记录锁文件、构建配置、workload
和原始源码摘要。正常门禁校验冻结 JSON 完整摘要，不自动重写基线。
非默认编译器、Cargo 包装器别名、目标专用 rustflags、增量和 profile 覆盖会被拒绝，
避免实际构建已变却仍报零增量。

| 消费者 | 链接 Flash | 静态 data+bss | exercise 局部帧 |
| --- | ---: | ---: | ---: |
| 原始 read，两字节固定读取 | 1226 B | 0 B | 72 B |
| 普通地址 read_normal，同等读取 | 1226 B | 0 B | 72 B |
| 严格 read_checked，两次 STOP 额外检查 | 1356 B | 0 B | 80 B |
| try_new / into_parts，独立资源取回工作量 | 104 B | 0 B | 28 B |

旧接口相对实施前匹配基线的 Flash、静态 RAM、exercise 和对象内最大局部帧增量均为
0 B。严格读取在该匹配读取工作量中增加 130 B Flash、8 B 局部帧。模拟总线对象布局 40 B、单 pin 16 B，它们不是静态 RAM，已体现于调用方状态/栈，
不能再重复相加。资源取回行工作量不同，不能与读取行相减。泛型不等于任意平台零成本。

局部帧不是完整调用链/中断栈峰值；输出由调用方提供，无随读取长度增长的内部数组。
这些是固定软件模型结果，不代表完整固件、其他架构、真实 HAL 或实机时序/栈水位。
报告在 target/i2c-extensibility/consumers/report.json；冻结参考为
scripts/i2c-compatibility-baseline.json，可运行 make i2c-compat 复验。

## 后续范围

标准 HAL 后端和完整软件 transaction 引擎仍是后续阶段。旧 STOP 分隔不能冒充
embedded-hal write_read 的 repeated START；硬件后端若分两次 write/read，必须保证
整对操作独占，持有共享代理的 &mut 不等于锁住两次调用。器件专用 ReadAt 适配另需
验证容量、偏移、页/bank、自增、回绕和空读规则。
