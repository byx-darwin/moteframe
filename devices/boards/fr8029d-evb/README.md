# FR8029D 开发板功能与实测状态

核对基线为 MoteFrame `3331950`，日期为 2026-11-27。芯片标称能力见[富芮坤型号表](../../chips/freqchip/README.md)，构建和实测记录见[固件 README](../../firmware/fr8029d-bringup/README.md)。

硬件依据为《FR8029D 开发板使用手册》V0.1（文件名 DEV0.2，2026-01-08），第 3～8 页为框图、资源、接口和原理图，第 11～16 页为跳线示例。资料位于厂商的 FR8029DP 目录，实测记录按工程名 FR8029D 归档。实物丝印、PCB 版本及选装件仍待现场核对。“待板测”表示此次核对未找到对应的 Rust 实机验收记录。

| 功能 | 芯片 / 手册板上的实际资源 | 当前 Rust 接入 | 已有板测与待验边界 |
|---|---|---|---|
| 启动、调试、串口 | 24 MHz 晶振、复位键、SWD（PB6/PB7）、USB 转 TTL（MCU RX PC10、TX PC11） | SRAM 探针、Flash XIP 启动、FreeRTOS 单应用任务 | J-Link 启动读回、裸机及 FreeRTOS 运行有记录；通用 UART 收发待验。[启动记录](../../firmware/fr8029d-bringup/README.md#flash-冷启动试验) |
| BLE 射频、GATT | PCB 天线 | Rust 射频与调度、1M/2M、可选 251 B 链路包、20 B 回显、FFF0 服务 | Mac 验证两版连接和 Notify；长包在两版 SRAM 镜像实测，两版 Flash OTA 后也验证过 20 B 回显。FFF0 连续 8 轮记录对应裸机长包 Flash 版。[长包及服务记录](../../firmware/fr8029d-bringup/README.md#可选的-251-字节长包) |
| Flash 和 OTA | 实测板 1 MiB Flash、Boot ROM | 启动头、暂存与搬运边界检查、新应用启动恢复写保护 | 两版 Mac OTA、保留区逐字节比对、SDK 全片恢复和写保护已验；ROM 搬运中物理断电续搬待验，启动失败自动回退属于设计项。[OTA 实测](../../firmware/fr8029d-bringup/README.md#ota-后自动恢复写保护实测2026-11-27) |
| 配置 A/B | 产品布局划出两个 4 KiB 扇区 | QSPI 后端、双槽及延后保存已接入 | 跨页掉电重试已有离线模型。两版实机写入 A/B 两代，复位读回最后一代。OTA 前后空配置槽已核对一致。带真实配置的断电重试和擦写期间 BLE 时序待板测。[离线模型](../../firmware/fr8029d-bringup/README.md#双槽配置的-qspi-后端) |
| 触摸、指示、蜂鸣器 | 两颗触摸键接 PC2（TK2）、PC3（TK6），红色 D7 接 PA5。另有复位键、滑条、圆盘和蜂鸣器 | 两颗 TouchKey 和 D7 已接入。蜂鸣器示例短接 PA4 与 BEEP | 两版完成真实空闲扫描及注入样本的 GPIO 输出核对。真实触摸、红灯目视、滑条、圆盘和蜂鸣器待验。 |
| 采集与通用总线 | I/O 扩展；芯片有 I²C、SPI、UART、ADC、PWM；具体外接传感器型号待选 | 通用 I²C 接口可复用；本板传感器、定时采样和 DMA 采集链待接 | 待板测已知信号或外接传感器、丢样和总线异常恢复。ADC 通道数在选型表与板册分别为 13 和 11，按目标封装及引出核实 |
| 显示 | LCD 接口含 CLK、CS、D0～D3、RST、TE；芯片另列段码和 RGB565 等能力 | 通用 display/UI 可复用；实装屏型号、面板驱动及引脚待核 | 接口已列入板册；待接实际屏验收刷新。段码上限在选型表与板册也有差异（8×34 / 8×32） |
| 音频 | 模拟 MIC、功放输出；板册示例将 PA2 接 PWMRP、PA3 接 AUEN；芯片有 I²S | MIC1 已接入 SARADC 定时采样和 BLE Notify；播放和 I²S 板级驱动待接 | MIC1 实测见下方 M3；功放输出和音频带宽连续传输待测 |
| 原生 USB、CAN FD | USB OTG 与 USB 转 TTL 分开；CAN 示例要求 PA14 接 CANRX、PA15 接 CANTX | USB 设备/主机、CAN FD 板级驱动待接 | 原生 USB 按角色测试；CAN 加第二节点验证。板册 USB 示例复用了 CAN 接线文字，接线以原理图核对 |
| 定时、低功耗、其他芯片外设 | 芯片定时器、PWM、硬件加密等；唤醒源取决于板级接线 | BLE 有专用时钟和事件唤醒；通用定时、看门狗、整板电源管理及加密接口待逐项接入 | 待验深睡电流、唤醒延迟、复位原因和加密测试向量；BLE 唤醒记录仅适用于对应固件模式 |

## M3 采集接线核对

开发板手册第 7 页将 **J7 的 1 脚（PA0）** 标为 I²C0 时钟、**J7 的 2 脚（PA1）** 标为 I²C0 数据。厂商 SDK v0.3.6 的 `examples/peripheral_demo/i2c/Src/i2c_demo.c` 对应 GPIOA 的复用功能 2。该示例对 EEPROM 进行读写；传感器驱动需按实物型号核对地址和识别寄存器。PA0、PA1 也连接原生 USB 数据网络，使用这组线前需确认原生 USB 数据口空闲。SWD 仍使用 PB6/PB7，日志串口为 PC10/PC11。

M3 使用板载模拟麦克风 MIC1。原理图第 7 页中，MIC1 经 C45/C46 接芯片专用差分输入 MIC_INP/MIC_INN（46、47 脚），由 MIC_BIAS（60 脚）经 R71 供偏置。PC14/PC15 属于另一组 ASP 输入。裸机和 FreeRTOS 共用 SARADC 及 BLE 采集实现。[M3 配置与验收记录](../../firmware/fr8029d-bringup/README.md#m3-板载-mic1-采集)

外接 I²C 模块的实装状态、上拉到的电压和跳线仍待核实。若使用 BME280，先核对模块电源和总线电平，在 7 位地址 `0x76` 或 `0x77` 读取寄存器 `0xD0`，预期芯片 ID 为 `0x60`。该识别步骤可在断线重接后复用。[Bosch BME280 数据手册](https://www.bosch-sensortec.com/media/boschsensortec/downloads/datasheets/bst-bme280-ds002.pdf)

## 实施顺序与验收

截至 2026-11-27，多厂商目录已登记芯片能力与板级资料。FR8029D 的 MIC1 由芯片层和板级层接入，QSPI 擦写与射频硬件操作由芯片驱动提供，固件管理分区和链路调度。

| 里程碑 | 状态 | 交付与验收 |
|---|---|---|
| M1 · 资料和实物对版 | 资料已整理，实物待补 | 已对照 FR802x、FR803x 资料与现有实现。现场补录 PCB 版本、丝印、跳线和屏幕装件；取得 FR803x 具体料号资料后补充型号行。 |
| M2 · 裸机和 FreeRTOS 基线 | 已合入 main，提交 `35b1d42` | 两版构建、BLE 重连、写入、Notify 和资源基线已记录。FreeRTOS 短包、长包 SRAM 版 BLE 栈实测分别剩余 908 B、704 B。[M2 实测](../../firmware/fr8029d-bringup/README.md#m2-驱动拆分前基线) |
| M3 · MIC1 采集与配置保存 | 板载 MIC1 验收通过 | 同一台 Mac 收齐两版 SRAM 镜像各 60 秒的采样。两版 Flash 镜像均完成 10 分钟采集和 10 轮重连、重新订阅，断连后保存最后一版配置，复位读回通过。裸机主栈剩余 12,420 B，FreeRTOS BLE 任务栈剩余 6,488 B。[逐轮数据和资源占用](../../firmware/fr8029d-bringup/README.md#m3-flash-版长时间采集与重连) |
| M3.1 · 驱动分层 | 实机验收通过 | MIC1、FreeRTOS 桥接、QSPI 和射频已分层。拆分后的两种 Flash 镜像通过采集、重连、20 B Notify 回显、断连保存和复位读回。两版 SRAM 探针通过固定 1M 和协商 2M 复测，FreeRTOS BLE 任务栈余量超过 512 B。[M3.1 收尾记录](../../firmware/fr8029d-bringup/README.md#m31-收尾与射频拆分复测) |
| M4 · 整板与外设验收 | TouchKey 和 LED 自动板测通过，实触待验 | [TouchKey 和 LED 构建与结果](../../firmware/fr8029d-bringup/README.md#m4-touchkey-与-led)。显示、音频输出、USB、CAN 和低功耗逐项验收。拿到外接传感器后补测断线、重接和恢复采样。 |
| M5 · 第二型号移植 | 待样板 | 优先选有完整资料的 FR803x 具体型号，验证启动、采集、射频、存储和 OTA，再据两块板的结果确定共用驱动范围。其他厂商沿用相同的板级与固件装配边界。 |

### M3.1 驱动分层验收

1. **芯片层**在 `devices/chips/freqchip/` 提供 FR8029D 已验证的 SARADC、时钟、模拟前端及 FIFO 操作。**板级层**在 `devices/boards/fr8029d-evb/` 选择 MIC1 的输入与偏置。**固件层**保留采样策略、缓冲、BLE 上报、延后保存及运行入口，由固件装配采集会话，解除 GATT 运行流程对具体 MIC1 会话的直接引用。
2. **MIC1 拆分后**重建裸机与 FreeRTOS 的 SRAM、Flash 镜像，用同一台 Mac 对两版各测 60 秒采集、BLE 重连和 20 B Notify。核对板端生成数、Mac 收到数、序号缺口、FIFO 异常和软件覆盖；Flash 版再验证连接期间待保存、断连后提交、重启读回。逐镜像对比 M3 的 Flash、静态 RAM、栈预留与同工况栈水位，FreeRTOS BLE 任务栈实测余量至少 512 B。
3. **QSPI 拆分轮次**验证了两版 Flash 的配置 A/B 换代。**射频拆分轮次**验证了两版 Flash 的断连保存和复位读回，A 槽写入第 1 代，B 槽保持擦除态。SRAM 探针验证固定 1M 和协商到双向 2M 时的重连与 Notify。SDK v0.3.6 全片及写保护均恢复并回读确认。物理断电仍待独立验证。

项目实现可从[固件入口](../../firmware/fr8029d-bringup/src/main.rs)、[芯片射频驱动](../../chips/freqchip/fr802x/fr8029d/src/radio.rs)、[固件链路适配](../../firmware/fr8029d-bringup/src/gatt_board_radio.rs)、[配置接入](../../firmware/fr8029d-bringup/src/qspi_config.rs)、[Flash 布局](../../firmware/fr8029d-bringup/src/flash_layout.rs)和 [OTA 实现](../../firmware/fr8029d-bringup/src/ota.rs)核对。厂商 SDK v0.3.6 示例只作为硬件行为参考。
