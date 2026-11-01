# MoteFrame（微构）

**面向小型嵌入式设备的模块化 Rust 基础框架。**

Modular Rust foundations for small embedded devices: portable interfaces,
allocation-free graphics, display transport and bounded buses.

当前提供首批通用组件，采用 Apache-2.0 开源许可证。尚未发布到 crates.io，
也未为新组合完成硬件资格验证。

## 组件

| Crate | 职责 |
| --- | --- |
| `moteframe-core` | 屏幕／GPIO／延时／只读接口，前台共享 SPI 事务 |
| `moteframe-display` | 通用 DCS 地址窗口、RGB565 分块写入，独立和共享 SPI 传输 |
| `moteframe-ui` | embedded-graphics 绘图适配、布局、裁剪、脏区域及协作式重绘 |
| `moteframe-i2c` | 有限时钟等待的开漏软件 I²C 寄存器读取 |

```text
application + board adapters (outside this project)
  ├── moteframe-ui ────────── embedded-graphics
  │       └── moteframe-core ── embedded-hal
  ├── moteframe-display ─────── moteframe-core + embedded-hal
  └── moteframe-i2c ─────────── caller-supplied open-drain pins + delay
```

所有库均为 `no_std`、无堆、禁止 unsafe。主机示例和测试使用 `std`。
产品业务、具体板级引脚、芯片寄存器、蓝牙、存储、原机素材及厂商资源不在首批范围内。
显示初始化序列和电气时序由调用方提供。

## 本地开发

需要已安装的 Rust 1.96.0（rustfmt、Clippy、Cortex-M0 target）、Python 3.9+、Make。
第一次准备依赖可显式运行 `cargo fetch --locked`。已有依赖后，回归命令均离线执行。

```sh
make check           # 格式、主机/ARM lint、测试、文档与边界检查
make build           # Cortex-M0 release 库
make example         # 生成 target/moteframe-demo.ppm，并检查实际像素
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

- SharedBus 只用于同一前台所有者，各设备必须采用同一已配置 SPI 模式／频率。
  错误会锁存总线故障；不会自动重试不确定写入，也没有未经验证的复位恢复。
- 同步 SPI 实现必须在返回前完成传输，并在底层提供有限等待。
  UI 协作式预算限制逻辑工作量，不保证实际无线、电气或实时延迟。
- DCS 显示需要明确支持相关命令、RGB565 字节顺序及跨分块连续写。
  代码没有内置适用于所有屏幕的初始化表。
- 硬件无关表示接口可适配，不表示任意芯片、屏幕无需适配即可运行。

贡献和编码规范见 [CONTRIBUTING.md](CONTRIBUTING.md)。
抽取边界见 [specs/foundation-extraction.md](specs/foundation-extraction.md)，
验证结果见 [docs/verification.md](docs/verification.md)。

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
