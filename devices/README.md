# 设备工程

本目录包含 FR8029D 的 Rust 源码、板级适配和测试。固件直接引用仓库根目录的 MoteFrame crates，并使用独立的 Cargo 入口，目标为 Cortex-M33（`thumbv8m.main-none-eabi`）。[FR8029D 板级说明](firmware/fr8029d-bringup/README.md)

设备工作区包含可复用的 BLE 控制器接口 `ble-controller` 和 Host 适配 `ble-host-trouble`。[设备 crates](crates/)

从仓库根目录运行主机测试。

```sh
cargo test --locked --manifest-path devices/Cargo.toml --workspace --all-features
cargo test --locked --manifest-path devices/firmware/fr8029d-bringup/Cargo.toml \
  --features ble-gatt-probe
```

交叉编译使用仓库固定的 Rust 工具链。`ble-gatt-board` 构建裸机 SRAM 探针。`ble-gatt-freertos,ble-gatt-xip` 构建 FreeRTOS Flash 执行版。FreeRTOS 版需要 Clang 和上游 FreeRTOS-Kernel V11.2.0。已有内核检出可通过 `FREERTOS_KERNEL_DIR` 指定。直接使用下面的检出目录时，固件构建会自动找到内核。

```sh
rustup target add thumbv8m.main-none-eabi
cargo build --locked --release --manifest-path devices/firmware/fr8029d-bringup/Cargo.toml \
  --target thumbv8m.main-none-eabi --features ble-gatt-board --bin fr8029d-ble-adv
git clone --depth 1 --branch V11.2.0 https://github.com/FreeRTOS/FreeRTOS-Kernel.git \
  devices/target/fr8029d-freertos/kernel
cargo build --locked --release --manifest-path devices/firmware/fr8029d-bringup/Cargo.toml \
  --target thumbv8m.main-none-eabi --features ble-gatt-freertos,ble-gatt-xip --bin fr8029d-ble-adv
```

厂商固件、设备读回和试验原始记录由板卡维护者在本地保管。第一方源码使用仓库的 Apache-2.0 许可。[仓库许可证](../LICENSE) · [来源声明](../NOTICE.md)

后续按[事件驱动里程碑](docs/superpowers/plans/2026-11-25-fr8029d-event-driven-runtime.md)推进。FR8029D 下轮实机验收使用 Mac 蓝牙和 J-Link。
