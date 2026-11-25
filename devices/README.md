# 设备工程

本目录包含 [FR8029D](firmware/fr8029d-bringup/README.md) 和 [Wattch 5](boards/wattch5/README.md) 的 Rust 源码、板级适配和测试。设备工程直接引用仓库根目录的 MoteFrame crates。两个固件保留各自的 Cargo 入口和目标架构，根目录的工作区仍用于 MoteFrame 库和可移植示例。

| 设备 | 入口 | 目标 |
| --- | --- | --- |
| FR8029D GATT 探针 | `firmware/fr8029d-bringup` | Cortex-M33，`thumbv8m.main-none-eabi` |
| Wattch 5 产品运行时和协议自检 | `firmware/wattch5-product` | Cortex-M0，`thumbv6m-none-eabi` |
| Wattch 5 板级测试和候选固件 | `firmware/wattch5-bringup`、`firmware/wattch5-candidate` | Cortex-M0，`thumbv6m-none-eabi` |

从仓库根目录运行主机测试。

```sh
cargo test --locked --manifest-path devices/firmware/fr8029d-bringup/Cargo.toml \
  --features ble-gatt-probe
cargo test --locked --manifest-path devices/firmware/wattch5-product/Cargo.toml \
  --features runtime-trouble,wattch5-source,legacy-health
```

交叉编译使用仓库固定的 Rust 工具链。FR8029D 的 `ble-gatt-board` 是裸机 SRAM 探针。FreeRTOS 版另须安装 Clang，并把 `FREERTOS_KERNEL_DIR` 指向上游 FreeRTOS-Kernel V11.2.0 的固定检出。Wattch 5 的 `arm-image` 入口执行协议自检。实际候选镜像使用板卡专用配置、实验策略以及调用方提供的打包输入。

```sh
rustup target add thumbv8m.main-none-eabi
cargo build --locked --release --manifest-path devices/firmware/fr8029d-bringup/Cargo.toml \
  --target thumbv8m.main-none-eabi --features ble-gatt-board --bin fr8029d-ble-adv
cargo build --locked --release --manifest-path devices/firmware/wattch5-product/Cargo.toml \
  --target thumbv6m-none-eabi --features arm-image,wattch5-source --bin wattch5-product
```

厂商固件、设备读回和试验原始记录由板卡维护者在本地保管。第一方源码使用仓库的 Apache-2.0 许可。发行源码包含合成测试夹具和附带上游许可的字体子集。 [仓库许可证](../LICENSE) · [来源声明](../NOTICE.md) · [字体授权](designs/wattch5-retro-parity/FONT-NOTICE.md)

后续按[事件驱动里程碑](docs/superpowers/plans/2026-11-25-fr8029d-event-driven-runtime.md)推进。FR8029D 下轮实机验收使用 Mac 蓝牙和 J-Link。
