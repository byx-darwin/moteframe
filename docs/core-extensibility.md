# core 扩展与资源核算

2026-11-02，本地实现候选。依据 [core 设计](../specs/core-extensibility.md)和
[实施计划](plans/2026-11-02-core-extensibility.md)，保留 0.2 既有接口，新增显式 HAL
桥接和资源取回。未接硬件，未发布 crate、固件或 release。

## HAL 桥接

新增 `moteframe_core::hal`：

| 类型 | 包装对象 | 提供的框架接口 |
| --- | --- | --- |
| `HalSpiBus<S>` | `embedded_hal::spi::SpiBus<u8>` | `SpiWrite`，write 后尝试 flush，首错优先，不重发 |
| `HalOutputPin<P>` | `embedded_hal::digital::OutputPin` | `OutputPin`，保留 high/low、底层错误和极性 |
| `HalDelay<D>` | `embedded_hal::delay::DelayNs` | `DelayMs`，将 u16 毫秒安全扩宽后转发 |

每个包装器只有原对象字段，使用 `new` 构造、`into_inner` 取回，均不在构造／取回时做 I/O。
可以包装 owned HAL，也可以包装 `&mut HAL`。未新增依赖、feature、静态缓存或堆。

`HalSpiBus` 包装整条 SPI 总线，CS 由外层控制。不要将自行控制片选的 `SpiDevice` 当成该输入。
write 和 flush 同时失败时保留 write 错误；只有 write 成功后才可能返回 flush 错误。
错误后的物理总线状态可能不确定；包装器不能中止一个已经阻塞的 HAL 调用，有限等待由底层实现保证。

## 资源取回与恢复

| 接口 | 行为 |
| --- | --- |
| 既有 `SharedBus::device` | 保持旧签名；错误时仍消耗 CS 引脚对象 |
| 新增 `SharedBus::try_device` | Busy、Faulted、初始 CS-high 失败时，返回原错误与原 CS |
| `DeviceCreateError::error` | 借用原错误，保留引脚所有权 |
| `DeviceCreateError::into_parts` | 取回 `(原错误, 原 CS)` |
| `Device::into_select` | 取回 CS |
| `SharedPanelBus::into_parts` | 取回 `(Device, DC)` |
| `SharedBus::into_parts` | 取回 `(SPI 控制器, 延时对象)` |

Busy/Faulted 构造失败不进行 I/O；初始 CS-high 失败仍锁存故障。所有取回方法只转移所有权，
不 flush、不切引脚、不清故障、不重试。错误容器也不要求 pin 可复制。

共享显示的完整恢复顺序：

```text
DcsDisplay.into_bus → SharedPanelBus.into_parts → 每个 Device.into_select
→ 取回全部相关 device 后 SharedBus.into_parts
→ 设备工程停止并恢复控制器、确认全部 CS 安全、按屏幕要求 reset/init
→ 重建总线和显示驱动
→ 使用 RefreshSession 时更新为未复用 generation，显式 try_rebind 后全屏重绘
```

取回不等于已经恢复：CS 仍可能为低、控制器状态和之前写入的效果可能不确定。
框架没有新增 reset_fault 开关或自动恢复。借用仍阻止在旧 device 后续要使用时搬走 owner。

## 契约与兼容性

`Display::Color: Copy` 保持不变，core 不依赖 graphics。RGB888、Gray8、BinaryColor、自定义
Copy 颜色可以通过各自 Display 实现接入；UI 绘图另需 PixelColor。DCS 驱动仍固定 RGB565。

Display 成功返回表示本次逻辑写入及其同步传输已完成；成功和失败后都可复用输入。
失败可能已有部分输出，不能假定回滚或无条件重试。该接口不保证光学画面更新完成，也未新增
统一 present 方法。UI Drawn 的文档同步明确为逻辑写入完成。

DisplayBus 保留命令／数据字节流与连续 RAM 写前提。SharedBus 保留前台独占、同 SPI
模式／频率及故障锁存；Result 路径清理不代表 panic unwind 后可以继续通信。

ReadAt 保留 u32 容量／偏移。读取前验证整个范围；长度转换与加法须检查；错误输出丢弃。
明确空读取在 offset≤capacity 时成功且零 I/O，超过末尾即使空读也拒绝。
仓库测试中的读源示例已按此边界更新；未新增生产存储后端。

## 固定 ARM 消费者实测

工具链 Rust 1.96.0，`thumbv6m-none-eabi`，opt-level=s、codegen-units=1、panic=abort，
最终链接使用 fat LTO 与同一链接脚本。输入为运行时 seed，所有传输／错误／清理进入可观察校验值。
以同一探针在原提交 `8727225c79ffe8d993a1efc4419f42122ae365f1` 和候选源码上重建旧路径。

字节数如下；Flash 为 text＋只读分配段＋data 初值，本组 data/bss 均为 0。

| 消费者 | Flash | 相对匹配旧基线 | 静态 RAM | exercise 局部帧 |
| --- | ---: | ---: | ---: | ---: |
| 旧独立 SPI 路径 | 1314 | 0 | 0 | 160（增量 0） |
| 旧共享 SPI 路径 | 1660 | 0 | 0 | 176（增量 0） |
| 新 HAL 包装器持有对象 | 1314 | 对正确手写桥接 0 | 0 | 160（增量 0） |
| 新 HAL 包装器借用对象 | 1278 | 对持有对象基线 −36 | 0 | 176（相对增加 16） |
| 显式取回，小 pin 结构 | 312 | 新恢复工作量，未作等价旧路径比较 | 0 | 120 |
| 显式取回，pin 多携带 40 字节 | 878 | 新恢复工作量，未作等价旧路径比较 | 0 | 352 |

旧路径和 owned HAL 的 wire／错误校验值一致。借用方案虽链接尺寸更小，局部帧却更大，
因此不能笼统称其更省资源。两个恢复消费者测的是 core 事务与资源取回，没有执行同一 DCS
绘图工作量，不能拿它们与上面的显示消费者相减得到“恢复只增加多少字节”。

布局测量区分对象与其 wrapper；每组 wrapper size/alignment 均与持有或借用的对象相同。

| ARM 对象 | 小 pin | 多携带 40 字节的 pin |
| --- | ---: | ---: |
| Pin／HalOutputPin | 12 | 52 |
| Device | 16 | 56 |
| DeviceCreateError | 16 | 56 |
| TryDeviceResult | 20 | 60 |
| SharedBus（当前 fixture 的 Bus/Delay） | 20 | 20 |

这些是实例尺寸，不能把 Result 与其中的 pin/error 再重复相加。取回失败资源会影响临时返回值
和局部帧；不能仅用 Device 成功实例估计恢复所需栈。真实 HAL 类型大小可能不同。
局部帧来自诊断 object，排除 callees、间接回调、IRQ 和最终链接优化差异，不是完整最坏栈。

## 栈空间优先时的接入选择

对同一绘图、输入和传输／错误工作量，进一步比较所有权装配方式；三组实际执行的校验值一致：

| 装配方式 | Flash | exercise 局部帧 | 相对全部借用 |
| --- | ---: | ---: | --- |
| SPI、CS、DC 全部借用 | 1278 | 176 | 基准 |
| 只借用 SPI，持有 CS／DC | 1324 | 168 | 少 8 字节局部帧，多 46 字节 Flash |
| SPI、CS、DC 全部持有 | 1314 | 160 | 少 16 字节局部帧，多 36 字节 Flash |

设备工程能交出这些外设的所有权时，本组测试中全部持有较省栈。若 SPI 需要保留在外部，
可以只借用 SPI 并持有控制引脚，装配关系如下；具体 HAL 对象由设备工程提供：

```rust
let panel = SpiPanelBus::new(
    HalSpiBus::new(&mut spi),
    HalOutputPin::new(cs),
    HalOutputPin::new(dc),
);
```

借用路径需要同时保留原始外设对象和引用；编译器输出显示这些对象进入调用者局部帧。
此结论只适用于固定 target、工具链和 fixture 类型，不能承诺任意 HAL 都节省相同字节。
本轮没有改变绘图块大小、缓冲区、flush、错误处理或清理语义，也未将对象移到全局 RAM。

另在本地临时副本尝试了 HAL 普通／强制内联和面板传输内联，均未降低这条借用路径的局部帧，
且部分增加 Flash，故没有保留运行时改动。无需新增框架配置即可选择上述所有权方式。
临时实验记录位于 `target/core-stack-experiments/`；它们未替换既有冻结基线或正常门禁探针。

## 持续检查与复现

```sh
make core-compat
make check build
```

`core-compat` 已纳入 make check，因此既有 CI 和本地完整检查都会执行。它检查六组固定消费者、
两个正向 ARM 外部消费者及一个 E0505 所有权拒绝案例；错误码、来源和 primary span 使用
结构化 Cargo 诊断验证，拒绝附加无关错误。正向 core 消费者只依赖 core 与 embedded-hal，
证明接入自定义 Copy 颜色无需 graphics。HAL 包装器的功能行为另由 Rust 集成测试维护。

冻结参考为 `scripts/core-compatibility-baseline.json`，包括原源码摘要、编译器、target、
lock/probe/linker 摘要、全部 workspace manifest 与影响编译的 Cargo 配置摘要，以及旧路径结果。
库构建显式固定 release 编译选项；正常检查还核验整个参考文件的固定 SHA-256，不能通过删来源
字段或悄悄调大预算放行。网络／注册表镜像专用配置不参与代码生成指纹，也不暴露其内容。
正常检查不会更新参考，也不需要历史提交存在于 CI 的
浅克隆中；环境、工作量、布局、旧行为不符或旧成本正增量均失败。禁止未记录的编译／profile 环境覆盖。
新 opt-in 路径只报告实测，不表示已有产品内存预算接受。

可选重现原参考时，将上述原提交用 git archive 解出到一个本地忽略目录，再运行：

```sh
python3 scripts/check_core_compatibility.py --baseline-root target/core-extensibility-implementation/baseline-source
```

脚本核对原源码字节后重建报告，不覆盖冻结参考。若以后有意改变编译器或探针，应重新评审
基线来源、工作量与成本，而不是为了通过检查自动抬高参考。

独立最终审查发现的两项 Important 均位于成本门禁：库构建 profile 未充分固定／记录，
以及来源摘要未被正常检查消费。已先复现再补回归，显式固定构建选项、记录 manifest／配置，
补齐原始显示 manifest 验证，并锁定整个参考文件。未因此修改运行库或放宽成本上限。

本地原始报告在 `target/core-extensibility-implementation/consumers/`，不随源码打包。
这些证据不代表其他 CPU、完整产品固件大小、设备栈水位、实机显示、时序、功耗或硬件恢复。

## 本轮验证结果

审查门禁修复后重新执行，重复运行不累加为新用例：

| 检查 | 结果 |
| --- | --- |
| workspace 全功能 Rust 测试 | 91 项通过：既有 82＋本轮新增 9 |
| 维护中的 Python 回归 | 67 项通过：既有 57＋本轮新增 10 |
| 既有 UI 功能／消费者及内存探针 | 随 make check 重建通过，未扩大其硬件证据范围 |
| 新 core 消费者与成本门禁 | 六组固定消费者、两个正向外部消费者、一个指定原因的拒绝通过 |
| Host／ARM Clippy、Rustdoc、ARM release 库 | 通过 |
| 源码／依赖边界与四 crate 包文件清单 | 通过，未发布 |
| 离线依赖政策、密钥扫描、拼写、差分空白 | 通过 |

执行命令为 `make check build`、`make supply-chain secrets`、`typos`、`git diff --check`。
依赖公告沿用本地缓存，没有宣称网络刷新。最终审查的两项门禁问题均已用失败回归复现后修复，
随后完整检查通过；运行库审查未发现需修复的新行为问题。变更仍在本地功能分支，未提交或推送。
