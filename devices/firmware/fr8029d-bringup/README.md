# FR8029D 板级启动探针

这个工程用 Cortex-M33 裸机 Rust 探针验证 Boot ROM 到应用入口的交接，读取启动寄存器，并把结果放在应用 SRAM。`probe` feature 启用基础探针的固件入口。默认构建只包含可在主机测试的寄存器字段解析和 Packet RAM 地址检查。厂商 SDK、ROM 反汇编和原厂镜像只用于取证。

## 构建与检查

在本目录运行。

```sh
rustup target add thumbv8m.main-none-eabi
cargo fmt --check
cargo test --offline --locked
cargo clippy --offline --locked --all-targets -- -D warnings
cargo clippy --offline --locked --release --features probe --target thumbv8m.main-none-eabi -- -D warnings
cargo build --offline --locked --release --features probe --target thumbv8m.main-none-eabi
```

产物为 `target/thumbv8m.main-none-eabi/release/fr8029d-bringup` ELF。可用 `llvm-objdump --private-headers --section-headers --syms -s -j .vectors <ELF>` 检查段和向量，再用 `llvm-objdump -d <ELF>` 查看入口和栈帧。

## 离线测试包与恢复验证

在本目录运行打包工具。两个输入是同一块板的两份 1 MiB 只读 Flash XIP 采集，输出目录由脚本新建。

```sh
python3 -B -m unittest discover -s tools -v
python3 -B tools/pack_probe.py \
  --original "$first_capture" \
  --repeat-capture "$repeat_capture" \
  --elf target/thumbv8m.main-none-eabi/release/fr8029d-bringup \
  --output "$package_dir"
```

工具依据实测的主启动头格式更新应用长度、应用 CRC 和头 CRC。输出 `sector-02000-probe.bin` 和 `sector-00000-probe.bin` 对应物理 Flash 偏移 `0x2000` 和 `0x0`，各 4 KiB。每个扇区配有 `-restore.bin`，对应原厂字节。OTA 元数据扇区 `0x1000` 保留。输出文件是按地址使用的扇区数据。

首轮产物在本地仓库 `target/fr8029d-probe-package-20261124`。已从落盘的扇区文件重新验证探针 CRC，并将恢复扇区回填到离线镜像，结果与原厂 1 MiB 备份逐字节一致。J-Link 重新读回板上的 `0x10000000..0x10003000`，与生成测试包时使用的原厂备份一致。

## Flash 冷启动试验

OpenOCD 通过 J-Link 操作 QSPI 控制器，并在 SRAM 运行 `tools/program_flash_page.s`，每次写入一个 256 B 对齐页。脚本只擦写 `0x0000` 和 `0x2000` 两个 4 KiB 扇区。实机预检验证了临时解除保护并恢复，以及 SRAM 例程拒绝 `0x1000` 的 OTA 扇区地址。试验脚本在每次擦除和编程后检查 Flash 状态并读回内容，先验证应用扇区，再更新启动头。写保护恢复后才复位。

离线输入可用 `xcrun clang --target=thumbv8m.main-none-eabi -mcpu=cortex-m33 -c tools/program_flash_page.s` 构建页写入目标文件，再用 `python3 -B tools/prepare_flash_trial.py --help` 查看所需的备份、ELF、打包清单及目标文件参数。生成的 Tcl 数据与实机使用的输入一致。已验证的 SWD 连接配置在 `tools/fr8029d_openocd.cfg`，擦写逻辑在 `tools/flash_trial.tcl`。用 `tclsh tools/test_flash_trial.tcl` 验证地址限制、回滚的扇区顺序及诊断字段检查。

实机写入和逐扇区读回后，先用系统复位确认 Rust 探针运行，再清除 SRAM 标记。用户给开发板断电再上电后，通过 SWD 读到新的 `FR29`、版本 `1`、结果码 `1` 及 `VTOR=0x10002000`。随后回填原厂扇区并启动原厂应用。复位后写保护读回为 `SR1=0x7c` 和 `SR2=0x02`。最后以 1 MHz SWD 读回整片 1 MiB Flash，与原厂备份逐字节一致。实机记录位于本地 `target/flash-trial/cold-power-on.log`、`restore.log`、`stock-protection.log` 和 `restore-verification.json`。

QSPI 配置寄存器 bit 31 是实时 idle 标志，XIP 取指期间可为 0。诊断验证会单独忽略这一位，并检查其余 QSPI 配置、容量配置和重映射值。

## SRAM 启动预检

可先构建由调试器从 SRAM 运行的版本，复用同一启动入口和诊断代码。

```sh
cargo build --offline --locked --release --features ram-probe \
  --target thumbv8m.main-none-eabi --target-dir target/sram
```

SRAM 版 ELF 只有一个可加载段，向量与代码位于 `0x20003000..0x20004000`，应用数据区从 `0x20004000` 开始。当前 628 B 负载的 Thumb 入口是 `0x20003141`，MSP 为 `0x20008000`。打包工具会拒绝该 ELF；原 Flash 版与测试包中记录的 ELF 指纹一致。

实机预检在原厂应用已建立 Flash XIP 后进行。暂停 CPU，将 ELF 的负载写入 SRAM 并读回核对，再设置栈与 Thumb 入口。读到诊断区的 `FR2R` 和结果码 `1` 后，复位回原厂应用并核对原厂向量和启动区。SRAM 预检只验证探针在芯片上执行的路径，Boot ROM 对试验镜像的冷启动还需验证。

实机预检已通过 J-Link/SWD 完成。SRAM 负载读回一致，诊断返回 `FR2R`、版本 `1`、结果码 `1`，VTOR 曾切换到 SRAM 的 `0x20003000`。系统复位后，VTOR 恢复为 `0x10002000`。完成全部预检后以 1 MHz SWD 读回完整 1 MiB Flash，与原厂备份逐字节一致。记录位于本地 `target/sram/ram-acceptance-openocd.log` 和 `target/sram/stock-postflight.json`。

QSPI 的两次 JEDEC ID 读取一致，字节顺序为 `85 60 14`，对应 Puya P25Q80 系列的 8 Mbit（1 MiB）Flash。状态寄存器读数 `SR1=0x7c`、`SR2=0x02` 对应该系列保护表的全片写保护。记录位于本地 `target/sram/flash-status-openocd.log`。 [Puya P25Q80U 数据手册](https://www.puyasemi.com/download_path/%E6%95%B0%E6%8D%AE%E6%89%8B%E5%86%8C/Flash/P25Q80U_Datasheet_V1.8.pdf)

## 启动和内存

FR8029D 板型由板主确认。只读 SWD 记录中的 CPUID `0x410fd210` 对应 Cortex-M33，运行中的 VTOR 为 `0x10002000`。原固件向量内的 MSP 为 `0x2000b058`，Thumb 入口为 `0x10002121`。ROM 的 `0x7420..0x744c` 将应用向量地址写入 VTOR，取首字写入 MSP，再通过 `0x7574..0x757c` 跳转第二个向量。Flash XIP 取样在 `+1 MiB` 处镜像，链接上限与后来通过 JEDEC ID 确认的 1 MiB 容量一致。采集记录在本地 `target/fr802x-readonly/fr8029d-board-20261124-d67yknib`。

| 区域 | 半开区间 | 用途 |
| --- | --- | --- |
| Flash XIP | `0x10002000..0x10100000` | 向量表、代码、只读数据和 `.data` 初值。跳过基址后的 8 KiB 启动区 |
| 应用 SRAM | `0x20003000..0x20006f00` | `.data`、`.bss`。下方保留给 ROM，实测 ROM 初始 MSP `0x20002818` 落在保留区内 |
| 诊断区 | `0x20006f00..0x20007000` | 保留 256 B，当前格式占用 32 B，不随 ELF 加载 |
| 栈 | `0x20007000..0x20008000` | 保留 4 KiB，MSP 初值为 `0x20008000` |

当前 release ELF 的 Flash 加载段为 **628 B**，`.data` 和 `.bss` 均为 0 B。反汇编中的 Rust 诊断入口固定栈帧为 **48 B**，另有 Cortex-M33 异常入栈等潜在开销。4 KiB 是链接预留的栈空间。

## SRAM 诊断格式

启动入口保存交接时的 VTOR，安装本固件的向量并初始化应用内存。探针只读取 CPUID 和 QSPI0 配置，屏蔽可屏蔽中断，完成后进入 `WFI`。以下字段按 32 位小端顺序从 `0x20006f00` 排列。

| 字索引 | 内容 |
| --- | --- |
| 0 | 最后写入的完成标记。Flash 版为 `0x39325246`（`FR29`），SRAM 版为 `0x52325246`（`FR2R`） |
| 1 | 格式版本 `1` |
| 2 | 结果 `1` 表示启动寄存器符合预期，`2` 表示 CPU 不符，`3` 表示交接 VTOR 不符，`4` 表示 QSPI0 配置不符 |
| 3–4 | CPUID、交接时的 VTOR |
| 5–7 | QSPI0 配置、设备大小配置和重映射寄存器。CPU 或交接 VTOR 不符时这三项为 `0` |

样本的 QSPI0 寄存器依次为 `0x80890087`、`0x00101002` 和 `0`。探针验证直接访问已启用、AHB 重映射已禁用、重映射地址为零、命令地址字段使用 3 字节。

## BLE 基带取证

以下来自本机重复读取一致的 256 KiB ROM，SHA-256 为 `af28da3883ea49629e416e9ec61f6398d80c8d4fc4aa0542d97a062372731e11`。这些值用于确定后续硬件适配的边界。

| 项目 | 已确认的 ROM 行为 |
| --- | --- |
| 连接控制区 | `0x9ec0..0x9ef2` 以 `0x94`（148 B）乘连接相关配置计数，为每项预留空间。补丁的连接事件更新路径也按此步长寻址 |
| RX 描述符区 | `0x9f0c..0x9f3c` 把 RX 区偏移写到 `0x200006f4`，再按每项 `0x24`（36 B）计算下一段的位置 |
| 另一组槽位 | `0x1dcb4..0x1dcca` 使用 Packet RAM 基址 `0x40010000`、表中 `0x200006f6` 对应的偏移和 `index << 4`。该路径支持 16 B 步长，字段及归属待确认 |
| 基带中断 | `0x38b14..0x38b84` 读取 `0x4000001c`，分别检测位 5、6、0，并往 `0x40000020` 写 `0x20`、`0x40`、`0x01` |
| 发射测试路径 | ROM `lld_test_start` 在 `0x339b4..0x339ca` 分配发送缓冲，把其偏移写入上述槽首项 `+0x04`，在 `0x33c28..0x33c32` 把槽字节偏移右移 2 后写入控制区 `+0x24`。该单位转换属于此路径，普通广播的字段和所有权仍待核对 |
| 时间读取 | ROM `rwip_time_get` 在 `0x3902c..0x39078` 向 `0x40000100` 置 bit 31 后读取快照，并对 `0x40000104` 的读数做 `0x270 - 值` 的转换。独立实现仍需确认完成条件和等待上限 |
| RF 校准 | SDK `patch_rf.o::rf_init_patch+0x164` 调用 `rf_calibration`。后者调用 TX/RX 校准并配置发射功率表，仍需结合板上观测确认本型号的时钟配置、校准完成条件和失败处理 |

主机测试覆盖 16 KiB Packet RAM `0x40010000..0x40014000` 内的 RX 和连接控制区寻址，以及对齐、溢出、越界和最后一项边界。探针固件未访问 Packet RAM 或基带寄存器。

早期实验先确认 RF 上电、校准、广播描述符和所有权，再处理连接中的 SN/NESN、ACK、重传、去重及中断时序。后续事件驱动工作见 [当前里程碑](../../docs/superpowers/plans/2026-11-25-fr8029d-event-driven-runtime.md)。

在仓库根目录运行 `cargo test --locked --manifest-path devices/Cargo.toml -p ble-controller -p ble-host-trouble`。测试覆盖匹配的空口 ACK 完成事件，以及通过脚本后端实现的 GATT MTU、读写、订阅和通知。

## BLE 1M 广播探针

```sh
cargo test --offline --locked
cargo build --offline --locked --release --target thumbv8m.main-none-eabi \
  --features ble-adv-probe --bin fr8029d-ble-adv
```

实验从 SRAM 执行，沿用原厂启动后留下的 RF 校准状态。原厂 CPU 调度停止后，由 Rust 写入广告数据、TX 描述符和调度槽，使用队列提交寄存器提交事件，随后读回匹配槽号与结束状态。链接中只有 SRAM 加载段，直接调用目标均在该段内。实验期间保持原厂 Flash 内容，结束后复位回原厂应用。

目标为 1M legacy `ADV_NONCONN_IND`，本地名 `FR8029D-1M`，测试静态随机地址 `C3:25:01:29:80:9D`。基带事件结束状态只用于验收调度队列。诊断区 `0x20006f00` 的前三字为 `FA1M`、版本和状态，后续记录提交数、结束数、时间、结束状态和槽号。

实机对比中，原厂广播 `802x_Ble_Periphreal` 能被复用自 `scripts/ble_scan.swift` 的 Mac 接收程序扫到，RSSI 为 −51 dBm。Rust 探针在独立扫描期间提交并收到 192 次基带事件结束，Mac 同时处理了 285 条 BLE 回调，目标名出现 0 次。仅替换调度、保留原厂控制和 TX 描述符的对照同样未收到目标。复位回原厂应用后，Mac 再次扫到原厂广播。

本地取证位于仓库根目录的 `target/fr8029d-adv-stage3`。重点记录为 `stock-scan-diagnostic.log`、`compare-schedule-scan.log`、`detach-receiver.log` 和 `detach-finish.log`。Mac 接收使用 CoreBluetooth，频道和原始 CRC 需用独立空口抓包设备确认。

### J-Link 发射使能与包格式对照

临时将 PC5 复用为 `ANT_TXEN`，通过 GPIO 上升沿锁存观察短脉冲，再用 J-Link 读取。普通 GPIO 输入对照为 0/80。下表统计出现过上升沿的观察窗口，一个窗口可能包含多次发射使能。

| 调度与数据 | 锁存到发射使能的窗口 | Mac 扫描结果 |
| --- | ---: | --- |
| 原厂调度与原厂广播 | 151/1200 | 2 条目标回调，RSSI −53、−54 dBm |
| Rust 调度与 Rust 广播 | 146/1200 | `FR8029D-1M` 0 条 |
| Rust 调度与原厂控制块、TX 描述符和广播数据 | 146/1200 | 原厂目标 0 条 |
| 原厂调度与 Rust 包头、地址和广播数据 | 226/1800 | `FR8029D-1M` 0 条 |
| 原厂调度与原厂地址和数据，仅将包头改为 `ADV_NONCONN_IND` | 271/2200 | 原厂目标 0 条 |

Rust 探针的 Packet RAM 读回包头为 `42 15`，对应 `ADV_NONCONN_IND`、随机地址位和 21 字节载荷。载荷包含 6 字节静态随机地址与 15 字节广播数据，Flags 和完整设备名字段的长度均匹配。原厂对照的控制块、16 字节 TX 描述符及 7 字节广播数据均与原厂快照逐字节一致。

两组 Rust 调度均触发了 `ANT_TXEN`，保留原厂 PDU 后 Mac 仍未收到目标。反向对照中，原厂调度只将包头从 `60 0d` 改为 `42 0d`，同样触发了 `ANT_TXEN`，Mac 仍未扫到原厂目标。优先核对基带广播事件模式与 TX 描述符中 PDU 类型的配合，以及 Rust 调度所需的 RF 状态。实际空口波形、广播频道、白化、CRC 和手机发现仍待验证。试验结束后复位恢复原厂应用和 PC5 配置，Mac 重新扫到原厂广播。

本地取证位于 `target/fr8029d-tx-trigger-20261125`。各组实验分别保留 J-Link 和 Mac 扫描日志，Packet RAM 解析见 `packet-ram-and-tx.json`。

### 广播模式复核

延长 Mac 扫描后，Rust 调度沿用原厂 `ADV_IND` 描述符时收到两次原厂目标回调。Rust 提交前读到 `0x4000001c` 的 bit 5 置位，原厂处理程序会确认该调度计时器中断。单独清掉该位时 Mac 收到一次原厂目标回调，保留该位时也能收到。这次对照修正了前一轮短窗口里未收到目标的观测。

让原厂 GAP 流程生成不可连接广播时，原厂写入 Packet RAM 的描述符字是 `0x1562`，同名包的事件槽 `+0x0a` 为 `0x0895`。Rust 初版对应 `0x1542` 和 `0x0acf`。只把 Rust 描述符改成 `0x1562` 后，Mac 在该轮仍未报告目标，但上报了 18 条其他不可连接广播。描述符字是芯片的内存接口，空口位值仍需独立确认。

手机先扫到原厂对照和 Rust 探针都用过的 `FR8029D-1M`，RSSI 约 −61 dBm。随后只在 Rust SRAM 探针中使用 `FR29-RUST1`，保留原包头 `0x1542` 和事件槽配置，手机扫码截图确认该新名字，RSSI 约 −58 dBm。J-Link 在 5600 个窗口中有 680 个锁存到 `ANT_TXEN`，Rust 在观察期间完成了 680 个基带事件。Mac 在同一轮扫描中收到 603 条回调，其中 77 条为不可连接广播，探针目标回调为 0 条。手机已验证借用原厂 RF 校准状态的最小 Rust 广播。Mac 未上报的具体原因、原始空口帧和独立 RF 初始化仍待验证。

实验完成后复位回原厂固件，Flash 起始 12 KiB 与实验开始前读回一致。详细日志位于 `target/fr8029d-rf-diagnosis-20261125`，其中 `rust-unique/` 对应手机确认的 Rust 专属广播试验。

### 原厂应用前的 RF 初始化试验

`ble-adv-cold` 启用 SRAM 探针的独立初始化路径。在原厂入口 `0x10002120` 的第一条指令前停住，确认基带 ID 尚未可读，再写入 Rust 探针并跳转。Rust 自行读取 Puya P25Q80 安全区的版本 2 出厂校准，校验 CRC 及 RF 有效位，加载合成器、调制、RX 直流偏置和功率表，再配置基带时钟、Packet RAM 和广播调度。构建不链接厂商库。该路径使用芯片固有 Boot ROM 的交接和本机的出厂校准数据。

```sh
cargo build --offline --locked --release --target thumbv8m.main-none-eabi \
  --features ble-adv-cold --bin fr8029d-ble-adv
```

基带寄存器 `0x400000e0` 是启动对照中定位到的必要配置。加入 `0x014000dc` 之前，首个事件以诊断 `15` 和状态 `0x800` 退出。补写后，基带连续完成广播事件，J-Link 也观测到 `ANT_TXEN`。单频道试验 `FR29-CLD02` 在手机和 Mac 上均未被发现。恢复三个主广播频道后，手机扫到专属名称 `FR29-CLD03`，RSSI 约 −63 dBm。J-Link 在 7500 个窗口中有 886 个锁存到 `ANT_TXEN`，探针在这段时间完成 888 个基带事件。Mac 当轮目标回调为 0 条。

`ble-adv-cold-xip` 构建独立启动的候选镜像，专属广播名为 `FR29-BOOT1`。启动代码把 QSPI 命令例程复制到 SRAM，操作安全区时不会从 Flash 取指。3892 字节的镜像临时写入启动头和应用入口所在的两个 4 KiB 扇区，两个扇区均读回一致。软件复位后，Rust 从 Flash 启动，基带完成 2500 个广播事件。

冷启动复测时，仅断开开发板供电，J-Link 仍测得约 3.06 V，广播计数继续增加。断开 J-Link 的 USB 并等待 10 秒后，单独给开发板上电，手机重新扫到 `FR29-BOOT1`，原厂广播 `802x_Ble_Periphreal` 未刷新。重新连接 J-Link 后读到 Rust 的 Flash 启动向量和重新生成的 `FA1M` 标记，广播完成计数从 743 增至 748。Rust 通过芯片 Boot ROM 从 Flash 启动，并使用板载出厂校准数据初始化射频，无需先运行原厂应用。

试验结束后恢复了原厂固件及写保护，整片 1 MiB Flash 读回与试验前备份逐字节一致。

本地取证位于 `target/fr8029d-cold-rf-20261125`。`pre-app-v2-trim.log` 和 `pre-app-timing-word.log` 为基带时序配置对照，`phone/` 为单频道实验日志，`three-channels/` 对应手机验收的三频道实验。`xip-boot1/retest.IcWsZb/` 对应 Flash 冷启动复测、写保护检查和完整读回校验。

### 可连接广播与最小 GATT

`ble-connect-probe` 从 SRAM 运行独立的 1M `ADV_IND`、扫描响应、连接请求接收和数据事件探针。`ble-gatt-board` 在同一板级初始化之上实现 `ble-controller::LinkBackend`，复用现有 HCI Controller 和 TrouBLE Host。GATT 接收的完整写入经 `moteframe_ble::WriteSlot` 交给应用。依赖只在启用 GATT 特性时引入。

```sh
cargo test --offline --locked --features ble-gatt-probe
cargo build --offline --locked --release --target thumbv8m.main-none-eabi \
  --features ble-gatt-board --bin fr8029d-ble-adv
```

GATT 探针名为 `FR29-GAT2`。Notify 版使用以 `A0` 结尾的测试地址，便于手机重新发现新增的描述符。主服务 UUID 为 `2f8029d0-7ff7-48b0-a64e-d6ebc735b251`，特征值 UUID 为 `2f8029d1-7ff7-48b0-a64e-d6ebc735b251`。特征值初始为 8 个零字节，支持读取、带响应的 8 字节写入和通知，值保存在 SRAM，下一次启动时清零。

板端范围为单连接外设、三频道 legacy 广播、LE 1M 和 CSA1。连接间隔与信道图更新在指定事件计数生效。硬件探针通过 J-Link 加载到 SRAM，在原厂应用入口之前启动，复位即可回到原厂应用。配对、加密、2M/Coded 及多连接未纳入此探针。

实机复测先用原厂连接核对了 1M 的 PHY 控制字 `0x1100`。手机随后连接 Rust 探针，完成服务发现和 ATT 读取。板端记录两次连接间隔更新均已生效。宿主测试覆盖带响应写入、应用收到完整 8 字节以及写后读回。信道图更新、丢包重传和长期稳定性待独立验证。

手机写入复测时，J-Link 抓到发往值句柄 `0x0009` 的 Write Request（`0x12`）。App 把 16 个十六进制字符按 ASCII 文本发送，板端按固定 8 字节长度校验返回了 `0x0D`（属性值长度无效）。直接用文本模式发送八个字符 `FR29OK42` 后，板端完成应用写入并返回 Write Response（`0x13`）。后续 Read Response（`0x0B`）的值为 `46 52 32 39 4F 4B 34 32`，与写入逐字节一致。支持 HEX 输入的 App 也可以选择 HEX 模式并发送这 8 个字节。原始记录见本地 `target/fr8029d-gatt-write.uxGs8n` 下的 `att-reconnected.log` 与 `att-readback.log`。

手机在同一特征值上开启 Notify 后，可以用 Write Request 以文本模式发送 `FR29NT75`，板端在写入成功确认之后主动通知这 8 个字节。退订后停止回显，重连时重新订阅。实机用 J-Link 记录了订阅（CCCD 写入 `01 00`）、手机发送的 8 字节写入，以及随后返回的 Write Response（`0x13`）和 Handle Value Notification（`0x1B`）。从写入到通知之间没有新的 Read 请求。手机截图显示 UTF-8 文本 `FR29NT75` 和对应的 HEX `46 52 32 39 4E 54 37 35`。记录见本地 `target/fr8029d-gatt-notify.J8Em4v` 下的 `phone-notify.log`。

Notify 版 SRAM 镜像含代码与初始化数据共 58984 字节，比读写版增加 904 字节。栈预留 22272 字节，此轮手机订阅、写入和通知期间的栈填充值覆盖深度为 11464 字节。实验未写 Flash。

## FreeRTOS 单应用任务

`ble-gatt-freertos` 把同一个 Rust BLE 循环放进一个静态分配的 FreeRTOS 任务。内核另有一个 Idle 任务。射频、Host 和 GATT 继续使用 Rust 实现，RTOS 部分使用上游 FreeRTOS-Kernel V11.2.0 的 Cortex-M33 端口和 C 内核。BLE 任务栈为 16 KiB，Idle 栈为 1 KiB，中断主栈为 1792 B。内核使用静态分配。

从仓库的 `devices/` 目录构建 SRAM 实验镜像。需要能交叉编译 Cortex-M33 的 Clang，可通过 `FREERTOS_CLANG` 指定。已有内核检出可以通过 `FREERTOS_KERNEL_DIR` 提供绝对路径。

```sh
git clone --depth 1 --branch V11.2.0 https://github.com/FreeRTOS/FreeRTOS-Kernel.git \
  target/fr8029d-freertos/kernel
cargo build --locked --release --manifest-path firmware/fr8029d-bringup/Cargo.toml \
  --target thumbv8m.main-none-eabi --features ble-gatt-freertos \
  --target-dir target/fr8029d-freertos/build --bin fr8029d-ble-adv
python3 firmware/fr8029d-bringup/tools/gatt_notify_memory.py pack \
  target/fr8029d-freertos/build/thumbv8m.main-none-eabi/release/fr8029d-ble-adv \
  target/fr8029d-freertos/rtos-trial.tcl
openocd -f firmware/fr8029d-bringup/tools/fr8029d_openocd.cfg \
  -f target/fr8029d-freertos/rtos-trial.tcl \
  -f firmware/fr8029d-bringup/tools/gatt_notify_sram_trial.tcl \
  -c 'run_notify_trial 240 target/fr8029d-freertos/stock-before-trial.bin' -c shutdown
```

手机连接 `FR29-RTOS`，在同一个 GATT 特征值上开启 Notify，再以 Write Request 写入 UTF-8 文本 `FR29NT75`。验收以手机自动收到相同 8 字节和 J-Link 记录的板端通知为准。退出时脚本复位回原厂固件并核对 Flash 前 12 KiB。

分别用 `ble-gatt-xip` 和 `ble-gatt-freertos,ble-gatt-xip` 构建 Flash 执行版，交给 `gatt_notify_memory.py compare <裸机 ELF> <FreeRTOS ELF>` 比较 Flash 负载和 SRAM 预留。栈用量取自 SRAM 实验的填充值覆盖深度。

实机复测中，手机连接并订阅 `FR29-RTOS` 后带响应写入 `FR29NT75`。J-Link 记录到 Write Response（`0x13`）、带相同 8 字节的 Notify（`0x1B`）和发送完成。手机显示的特征值为 HEX `465232394e543735`。复测期间有一次连接在服务发现时断开，重连后完成验收。实验在 SRAM 中执行，结束后复位回原厂固件。

| 项目 | 裸机 Notify | FreeRTOS 单应用任务 |
| --- | ---: | ---: |
| Flash 执行版负载 | 58,988 B | 61,560 B |
| Flash 执行版静态 SRAM 含对齐 | 228 B | 540 B |
| 栈总预留 | 22,272 B | 19,200 B |
| Flash 执行版应用 SRAM 总预留，含栈和诊断区 | 22,756 B | 19,996 B |
| SRAM 实机的 BLE 执行栈覆盖深度 | 11,464 B（主栈） | 11,472 B（BLE 任务栈） |
| SRAM 实机的中断主栈和 Idle 栈覆盖深度 | 共用主栈 | 152 B 和 80 B |

裸机栈覆盖深度来自前一轮同板 Notify 实验。FreeRTOS 版的 SRAM 总预留较小，主要来自这次选用的栈尺寸。Idle 栈数值包含内核初始化任务上下文，不能单独证明空闲任务曾运行。Flash 执行版的两个数值列由 ELF 计算，冷启动及运行栈尚未上板核对。
