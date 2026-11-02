# MoteFrame（微构）

**面向小型嵌入式设备的模块化 Rust 基础框架。**

Modular Rust foundations for small embedded devices: portable interfaces,
allocation-free graphics, display transport, bounded buses, storage views and BLE value handling.

源码仓库：[byx-darwin/moteframe](https://github.com/byx-darwin/moteframe)。
采用 Apache-2.0 开源许可证。

当前 `main` 包含 **0.2.0 源码候选**：类型化颜色接口、UI 配置、资源版本、
可裁剪功能及刷新恢复，以及只读存储视图、BLE 通知校验和接收写缓冲。
各 crate 的 Cargo 版本为 `0.2.0`，保持 `publish = false`；
接入时使用仓库源码，尚未发布到 crates.io。本版本包含源 API 变更，
从 0.1 迁移请参阅[颜色类型迁移指南](docs/color-types.md)。

## 组件

| Crate | 职责 |
| --- | --- |
| [moteframe-core](crates/moteframe-core/README.md) | `Display::Color`、几何、GPIO／延时／`ReadAt`、HAL 桥接与共享 SPI 资源取回 |
| [moteframe-display](crates/moteframe-display/README.md) | 通用 DCS 地址窗口、RGB565 分块写入，独立和共享 SPI 传输 |
| [moteframe-ui](crates/moteframe-ui/README.md) | 类型化绘图与配置、可选布局／刷新／差分／节流、资源版本和恢复 |
| [moteframe-i2c](crates/moteframe-i2c/README.md) | 有限时钟等待的开漏 I²C 指针读取、普通地址校验、可选 STOP 检查与资源取回 |
| [moteframe-storage](crates/moteframe-storage/README.md) | 基于 `ReadAt` 的切片数据源、借用适配与有界只读范围视图 |
| [moteframe-ble](crates/moteframe-ble/README.md) | 通知容量／目标校验、后端入队边界和完整接收写的单槽缓冲 |

```text
application + board adapters (outside this project)
  ├── moteframe-ui ────────── embedded-graphics
  │       └── moteframe-core ── embedded-hal
  ├── moteframe-display ─────── moteframe-core + embedded-hal + embedded-graphics-core
  ├── moteframe-i2c ─────────── caller-supplied open-drain pins + delay
  ├── moteframe-storage ─────── moteframe-core::ReadAt
  └── moteframe-ble ─────────── caller-supplied GATT notification backend
```

所有库均为 `no_std`、无堆、禁止 unsafe。主机示例和测试使用 `std`。
产品业务、具体板级引脚、芯片寄存器、BLE 栈／射频实现、具体 Flash 驱动、产品持久化与算法、
原机素材及厂商资源不在仓库范围内。通用存储／BLE 模块由固件按依赖选择，不要求 UI 使用它们。
显示初始化序列和电气时序由调用方提供。

## 架构范围与平台适配

**MoteFrame 不限定 Cortex-M0。** 通用运行库没有绑定该架构的寄存器、指令或启动代码，
通过接口隔离芯片与板级实现。当前固定的嵌入式验证目标为 Cortex-M0，适配范围与
已验证范围如下：

| 平台范围 | 当前状态 |
| --- | --- |
| 主机环境 | 用于单元测试、离线绘图及工具检查 |
| Cortex-M0（`thumbv6m-none-eabi`） | 已纳入交叉编译、Clippy、功能组合消费者及内存／链接探针；未做实机验证 |
| 其他架构，例如 Cortex-M3／M4／M7、RISC-V | 可按通用接口开展适配，尚未纳入当前交叉编译或硬件验证范围 |

框架维护通用接口和可复用实现，不预先为每种芯片补齐板级代码。具体适配放在使用
MoteFrame 的设备工程中；有明确平台需求时，再补充该平台的编译与验证覆盖。
适配过程中发现的通用接口缺口，可在框架中完善。

| 设备工程负责 | 接入方式 |
| --- | --- |
| 启动、时钟、内存与中断 | 配置芯片 HAL、启动代码和链接脚本，不放入通用运行库 |
| SPI、GPIO 与延时 | 包装为 `SpiWrite`、`OutputPin`、`DelayMs`；共享 SPI 路径使用 `embedded-hal` 总线接口 |
| 显示屏 | 兼容 RGB565 DCS 的屏幕可复用 `SpiPanelBus`／`SharedPanelBus` 与 `DcsDisplay`，由设备工程提供初始化序列；其他屏幕实现 `Display` |
| 外部资源（按需） | 实现 `ReadAt`，并提供资源内容、元数据及版本 |
| 帧间隔时钟（按需） | 启用 `frame-pacing` 后，为 `draw_timed` 提供有界、单调的微秒时钟 |

标准 `embedded-hal` 1.0 对象可通过 `moteframe_core::hal` 中的 `HalSpiBus`、
`HalOutputPin`、`HalDelay` 桥接，支持持有或借用。`HalSpiBus` 对每次写入尝试一次
flush，保留首错；它包装 SPI 总线，不包装已自行管理 CS 的 `SpiDevice`。
共享总线新增 `try_device`，失败时返还原 CS；显式资源取回与重建流程见
[core 扩展与资源核算](docs/core-extensibility.md)。

I²C 新接入可使用 `NormalAddress7`（`0x08..=0x77`）和 `read_checked`，在指针阶段和
最终阶段 STOP 后额外检查实际 SCL／SDA。原 `read` 保持 STOP 分隔和输出释放语义；
`read_normal` 只增加普通地址约束。`try_new` 失败保留引脚／延时，`into_parts` 无 I/O
取回资源。通过调用的方法选择能力，无需新增 feature 开关。使用示例、错误处理、
有限等待义务及实测 Flash／局部栈帧见 [I²C 扩展与资源成本](docs/i2c-extensibility.md)。

SPI 适配必须在返回前完成实际传输、提供有限等待并如实返回错误；底层使用 DMA
时也须满足同步完成约定。芯片、屏幕和接线的具体电气行为由设备工程验证。

安装相应 Rust target 后，可覆盖默认构建目标。例如以下命令用于检查
`thumbv7em-none-eabi` 的库构建与 lint，目标须按实际芯片及浮点配置选择：

```sh
rustup target add thumbv7em-none-eabi
make build TARGET=thumbv7em-none-eabi
make lint TARGET=thumbv7em-none-eabi
```

这些是适配命令示例，不是该目标已经通过验证的声明。`TARGET` 目前只影响 Makefile
中的目标库构建与目标 Clippy；`make memory`、`make ui-config`、`make core-compat`、`make i2c-compat` 的探针和消费者仍固定为
`thumbv6m-none-eabi`。因此 `make check TARGET=...` 也不会把整套验收切换到新架构。
新平台需另行补充验证，并重新核算内存、栈和硬件时序。

## 支持的颜色类型

通用接口通过 `Display::Color` 关联逻辑颜色；UI 使用原生颜色的 `Tile<C>` 和工作区，
具体显示驱动负责编码。当前已有离线测试覆盖的颜色类型如下：

| 颜色类型 | 表达能力 | UI 层支持 |
| --- | --- | --- |
| `Rgb565` | 16 位彩色，红／绿／蓝分别为 5／6／5 位 | 配置、绘图、刷新与差分 |
| `Rgb888` | 24 位真彩色，RGB 各 8 位 | 配置、绘图、刷新与差分 |
| `Gray8` | 8 位灰度，256 级 | 配置、绘图、刷新与差分 |
| `BinaryColor` | 黑白／开关两种颜色 | 配置、绘图、刷新与差分 |
| 自定义颜色 | 由调用方定义颜色语义；已有非 Default 类型测试 | 满足相应接口约束后使用 |

配置状态 `UiState<C>` 要求颜色满足 `Copy + PartialEq`；实际绘图还要求实现
`embedded-graphics` 的 `PixelColor`。无需实现 `Default` 或 `IntoStorage`。
颜色类型由泛型参数确定，无需开启对应颜色的 Cargo 功能。

**显示驱动支持范围**：仓库自带的 DCS 驱动目前只输出 RGB565，完成 MSB-first 字节编码。
RGB888、灰度或单色屏需要对应的 `Display` 驱动适配；上表的 UI 离线覆盖不代表
这些物理屏幕已经完成驱动或实机验证。

## 绘图缓冲

`StreamTarget::new` 默认使用 32 像素缓冲；自定义容量使用
`StreamTarget::<_, 8>::with_run`，有效范围为 1..=65535，由编译期检查。
流式绘图缓冲与刷新工作区分别由调用方核算；减小刷新块不会缩小已声明的数组。

## UI 配置与刷新

`UiSettings<C>` 描述候选设置，`UiState<C>` 持有通过校验的设置和逻辑尺寸，
`UiContext` 为整帧提供稳定的设置与资源视图。调色板显式提供五种角色颜色；
布局默认 gap／padding 为 0，显式绘图参数优先于配置默认值。

| Cargo 功能 | 默认启用 | 提供能力 |
| --- | --- | --- |
| `layout` | 是 | 布局设置、padding、栈式布局 |
| `refresh` | 是 | 小块刷新、dirty 区域、`RefreshSession` 和显示绑定恢复 |
| `damage` | 是 | 差分刷新辅助；同时启用 `refresh` |
| `frame-pacing` | 否 | 微秒时钟驱动的帧间隔控制；同时启用 `refresh` |

使用 `default-features = false` 可仅保留基础绘图、配置与资源接口。
关闭功能会同时移除相应字段和 API，不保留隐藏的 dirty 或调度状态。
Cargo 功能会在整个依赖图中合并。

- **配置更新**：候选整体校验后才提交；返回 `Unchanged`、`PolicyOnly` 或
  `FullRedraw`。校验失败保留原状态；样式或资源变化请求整屏重绘。
- **资源版本**：`ResourceView` 校验目录元数据，`ResourceKey` 使用集合与修订号
  标识内容。内容、度量或索引改变须换键；内容身份和格式由提供者保证。
  仓库未提供生产 Flash、codec 或缓存实现。
- **刷新与恢复**：`RefreshSession` 独占借用 state 和实际工作区。
  `DisplayBinding` 使用调用方维护的非复用 generation；尺寸或代次变化会锁定
  `NeedsRebind`，须经有效的 `try_rebind` 恢复并请求整屏重绘。
- **失败与后续请求**：scene、显示写入或 service 失败保留完整本轮请求。
  service 通过 `DeferredInvalidation` 登记后续请求，帧后合并，单次调用最多处理一帧。
- **可选节流**：`draw` 签名不随功能合并改变；开启帧间隔后使用 `draw_timed`。
  普通 `draw` 遇到需节流的 dirty 工作返回 `ClockRequired`；失败的帧开始也计入间隔。

完整契约与接入示例见 [UI 配置设计](docs/ui-configuration.md)和
[实现及内存核算](docs/ui-configuration-implementation.md)。低层显式绘图、布局和
刷新接口仍可单独使用，无需统一迁移到 session。

### 省 RAM 编译开关

受维护的 [独立应用示例](examples/ui-low-memory/README.md) 已提供完整、可运行的装配：
它自带 `[workspace]` 和锁文件，以相对路径依赖框架，禁止发布，不需要板级驱动即可做
主机验证和 ARM 库构建。应用拥有 `ui-low-memory = []`，默认功能为空；库的 feature、
默认接口以及 `StreamTarget::new` 的 RUN=32 保持不变。

唯一的容量常量同时控制**实际数组**与流式 RUN：

```rust
const UI_PIXELS: usize = if cfg!(feature = "ui-low-memory") { 8 } else { 32 };
let mut scratch = [palette.background(); UI_PIXELS];
let mut session = RefreshSession::new(&mut state, &mut scratch, RefreshSettings::new())?;
// 流式路径：StreamTarget::<_, UI_PIXELS>::with_run(&mut display)
```

该片段由固件提供 `state` 和 `palette`；完整类型、配置及错误处理见示例。
Auto 采用实际容量；`Fixed(UI_PIXELS)` 可用，更大的 Fixed 返回 `WorkspaceTooSmall`。
只调小刷新块上限不会缩小已声明的数组。

从**本仓库根目录**运行两档主机验证与 Cortex-M0 库构建：

```sh
cargo test --manifest-path examples/ui-low-memory/Cargo.toml --offline --locked
cargo test --manifest-path examples/ui-low-memory/Cargo.toml --offline --locked --features ui-low-memory
cargo build --manifest-path examples/ui-low-memory/Cargo.toml --release --target thumbv6m-none-eabi --offline --locked
cargo build --manifest-path examples/ui-low-memory/Cargo.toml --release --target thumbv6m-none-eabi --offline --locked --features ui-low-memory
```

新环境先显式运行 `cargo fetch --manifest-path examples/ui-low-memory/Cargo.toml --locked`。
也可用 `python3 scripts/check_ui_low_memory.py` 自动执行并比较两档最终图像。
在**固件工程**加入同名应用 feature、真实数组与 RUN 选择后，则在固件根目录运行：

```sh
cargo build --release --target thumbv6m-none-eabi --offline --locked
cargo build --release --target thumbv6m-none-eabi --offline --locked --features ui-low-memory
```

Cargo feature 在同一依赖图中会合并；其他依赖开启的 UI 功能不会被本应用的
`default-features = false` 关闭。应用级开关只控制本应用的容量，两档必须分别构建；
`--all-features` 不能替代默认档测试。

真实 RGB565 scratch 从 64 B 缩到 16 B。历史固定离线消费者的 UI 对象预算为
144／96 B，局部帧为 288／240 B；这些不是当前示例或任意固件的固定占用。工作区和
局部帧是不同口径，不能相加作为整机最坏栈。相同 64×8 图像的 write／service 各为
16／64 次，不表示耗时恰为四倍。历史测量条件、功能合并影响和硬件限制见
[UI 内存档位验证](docs/ui-memory-profiles.md)。

## 本地开发

需要已安装的 Rust 1.96.0（rustfmt、Clippy、`thumbv6m-none-eabi` target）、Python 3.9+、Make。
第一次准备依赖可显式运行 `cargo fetch --locked`。已有依赖后，回归命令均离线执行。

```sh
make check           # 格式、主机/ARM lint、测试、文档与边界检查
make build           # 默认 Cortex-M0 release 库，可通过 TARGET 覆盖
make example         # 生成 target/moteframe-demo.ppm，并检查实际像素
make memory          # 宿主/ARM 颜色布局、对象代码及单函数栈帧
make ui-config       # 功能组合、编译拒绝及配置内存／链接探针
make core-compat     # core 消费者、所有权拒绝与固定 ARM 体积对比
make i2c-compat      # I²C 旧接口零增量门禁及严格读取／资源取回实测
make portable        # 存储／BLE 固定消费者、布局和链接／局部帧证据
make ui-low-memory   # 持续验证应用示例的真实 32／8 像素两档
make packages        # 检查 Cargo 计划打包的文件，不发布
make supply-chain    # 依赖许可证、advisory 和依赖来源检查
make secrets         # 本地工作树密钥扫描
make hooks-install   # 安装 pre-commit / commit-msg hooks
make hooks           # 对实际源文件执行 hooks，不暂存或提交
```

`cargo fetch` 需要网络；Rust 回归使用 `--offline --locked`。
`make supply-chain` 使用已有 advisory 缓存；更新 advisory 数据库属于另一次显式网络操作。
pre-commit 首次准备远端 hook 环境也可能需要网络。

离线图形示例只绘制程序生成的文字、矩形、圆和线段，不读取外部素材，不访问设备。
软件 I²C 当前支持 7 位地址、8 位寄存器指针读取，指针写与读取之间使用 STOP；
它不是通用多主机／任意 I²C transaction 实现。

## 使用边界

- `moteframe-storage` 只读视图复用 `ReadAt`，校验本地／映射范围及后端当前容量，支持显式
  borrowed/dyn 包装与原资源取回；没有写擦、文件系统、codec 或缓存。
- `moteframe-ble` 的通知成功表示后端接受入队，不表示已发到空口或对端确认。目标身份、
  连接代次、订阅和 bearer 的当前容量由设备适配器验证；写事件缓冲仅接收完整已提交值。
  没有内置 BLE 栈、广播发现、配对、后台收包或产品协议。

- SharedBus 只用于同一前台所有者，各设备必须采用同一已配置 SPI 模式／频率。
  错误会锁存总线故障；不会自动重试不确定写入，也没有未经验证的复位恢复。
- 同步 SPI 实现必须在返回前完成传输，并在底层提供有限等待。
  UI 协作式预算限制逻辑工作量，不保证实际无线、电气或实时延迟。
- DCS 显示需要明确支持相关命令、RGB565 字节顺序及跨分块连续写。
  代码没有内置适用于所有屏幕的初始化表。
- 硬件无关表示接口可适配，不表示任意芯片、屏幕无需适配即可运行。

## 验证与文档

当前 0.2 离线验收覆盖类型化颜色、配置更新与资源、功能裁剪、真实 ARM 消费者的
编译成功／预期拒绝，以及刷新失败、恢复和节流边界。详情见
[UI 配置 r0.2 多角色复验](docs/ui-configuration-review-r02.md)。

ARM 布局、对象代码、链接消费者和单函数栈帧均为离线证据；不代表完整产品固件大小、
最坏调用链／中断栈、设备栈水位、刷新率、功耗或电气时序。尚未完成本组合的硬件资格验证。

| 文档 | 内容 |
| --- | --- |
| [颜色类型迁移](docs/color-types.md) | 0.1 → 0.2 API 变化、颜色覆盖与内存证据 |
| [UI 配置设计](docs/ui-configuration.md) | 配置、资源、裁剪、刷新与恢复契约 |
| [UI 配置实现验收](docs/ui-configuration-implementation.md) | 接入示例、实现覆盖与 ARM 内存预算 |
| [UI 配置 r0.2 多角色复验](docs/ui-configuration-review-r02.md) | 最新离线验收、编译拒绝门禁修复与证据边界 |
| [UI 内存档位验证](docs/ui-memory-profiles.md) | 默认／省 RAM 两档的同画面对照与应用级编译开关建议 |
| [core 扩展与资源核算](docs/core-extensibility.md) | HAL 桥接、资源取回、兼容消费者及 Flash／RAM／局部栈帧实测 |
| [I²C 扩展与资源成本](docs/i2c-extensibility.md) | 普通地址、严格 STOP 检查、资源取回和兼容性／固件成本 |
| [存储／BLE 设计与验证](docs/portable-modules.md) | 扩展边界、兼容方式、资源成本及冻结基线迁移 |
| [初始抽取验证](docs/verification.md) | 0.1 抽取、开源准备及历史验证记录 |
| [项目进度](docs/progress.md) | 分阶段开发与源码交付记录 |
| [通用组件抽取边界](specs/foundation-extraction.md) | 仓库范围与适配责任 |
| [贡献指南](CONTRIBUTING.md) | 开发、检查与贡献规范 |

## 许可证

第一方代码采用 **Apache-2.0**，完整条款见 [LICENSE](LICENSE)，来源声明见 [NOTICE](NOTICE)。
允许商用、修改和闭源集成。对外分发源码、固件或其他衍生产品时，应遵守
Apache-2.0 第 4 条，包括提供许可证、保留适用的版权和 NOTICE 来源声明，
并在修改过的文件中标明修改。

商用产品也需要保留 **MoteFrame（微构）** 的适用来源声明和项目地址。
NOTICE 可随产品文档／第三方开源声明提供，也可放在通常展示第三方声明的界面；
不要求增加启动画面。复制 NOTICE 时保留其适用内容，不能只写一句项目名称代替。

每个 crate 均携带 LICENSE 和 NOTICE。上游依赖保留其自己的许可证，见
[NOTICE.md](NOTICE.md)。本仓库的许可证不适用于被排除的私有产品、素材或厂商软件。
