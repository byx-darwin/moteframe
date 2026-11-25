# syd881x-radio

芯片专用 `no_std`、无 heap、`forbid(unsafe_code)` 算法层。运行时零依赖，不链接厂商库、不调用 ROM，不改变通用 `device-ble` 接口。板级适配器实现 `ByteRegisters`；公开地址只能由五种 `Bank` 和 `u8` offset 构成，不接受任意 RAM/MMIO 整数地址。适配器必须串行拥有硬件、每次一次 byte 访问、保持顺序，IRQ/NVIC、时钟、电源和复位由集成层管理。

## 证据与范围

固定分析输入：32768 字节 ROM，SHA256 `1b32ead7d8689857af6986ed7cd790ae425209da1c3989300c424d70749797e9`。源文件位于 `target/hardware/2026-10-27/{radio-assessment,boot-analysis}`；这里的 ROM 地址仅标识静态证据，运行时不调用它们。

| 算法 | 固定 ROM 指令 / 元数据 | 实现含义 |
| --- | --- | --- |
| RF 读 / 写 | `0x61c4..0x61fc` | MISC+1a OR1，+18 地址，写用+19；读用+1c（含丢弃初样），随后等待+1a bit0=1，有限 budget |
| bank 地址 | 配置 executor `0x0c26..0x0c58`；ROM `0x77dc/0x77e0` | LL `40020000`、MD `40030000`、MISC `40000000`、PWRON `50001000`、INTF `50010000`，只允许 byte offset |
| 表执行 | `0x0bfa..0x0c64` | 六字节头；+4 LE16 count，逐条 type/address/value；RF 7f 自然改变后续 bank，不排序、不合并 |
| 广告参数 | `0x565e..0x569a` | +15 channel selection，+11 低7bit，+12/13 interval，+10 OR2，+16=0 关闭过滤 |
| 设备地址 / AA | `0x579e..0x57c0` | MAC→LL+80，AA→LL+88；本实现使用 byte copy 路径，ROM 对齐时也可能 word copy；标准广告 AA 按字节 `d6 be 89 8e` |
| PDU / FIFO0 | `0x3f7a..0x3fd0`，`0x60d8..0x6102` | 两字节头+六字节 AdvA+AD；头低6bit length+2 是完整FIFO长度；+3 bit0满则无写，+c0全部字节后仅一次+c8=1 |
| 数据包待发提示 | `0x114c..0x11f8` | 发送前按待发状态设置/清除头bit4；`DataPdu::with_more_data`只改MD，队列提交时依据是否还有后续包决定，末包清零 |
| 广告触发 | `0x56a6..0x56aa` | LL+10 OR1，不写 ROM bookkeeping RAM |
| 状态 / 应答 | `0x6294..0x6296`；DWARF `LL_CTRL_TYPE` 的 INTST/TX_FULL/ADV_EN | 原始 LL+1、+3、+10 顺序采样；应答是重新读+1并写回同值 |

`AdvertisingPdu::nonconnectable` 只编码 ADV_NONCONN_IND，TxAdd=1，RxAdd=0，RFU=0；静态随机地址按空口低字节在前，最高两位必须11且随机46bit不能全零/全一。AD 由调用者提供，最长31字节；不加入业务内容，Debug只显示长度。完整最大PDU是39字节，不含 preamble/access address/CRC。

`ConfigurationTable::new` 在任何硬件访问前拒绝未知type、截断、尾随字节或超过4096字节；这是比ROM更严格的输入校验，ROM自身会跳过未知type。调用者若读完整4KiB区域，须先按照头部count截取精确表长，再构造table。`apply_configuration` **仅执行记录loop**，不是完整初始化：不执行前后校准、头部clock helper、唤醒/trim，不执行ROM软件状态。

`AdvertisingParameters` 的interval单位和channel selection到真实频点的映射仍是候选；API不把bit1等同某标准频道。无证据的CRC/whitening/power/clock/自动sleep位不在此层猜写。AA固定常量不代表已经证明硬件采用该AA。RF bit0返回1仅说明满足ROM观察条件；未证明其电气含义。`Status`和`wait_interrupt`只返回原始观测，不称TX/CRC/空口成功。`acknowledge_interrupt`复现指令顺序，不假定W1C清除已实测。

所有poll有次数上限；RF超时不返回早期数据。FIFO满没有任何写，中途适配器失败不提交；已有部分FIFO数据及配置写不会自动回滚，集成层必须记录失败并复位/恢复，不能无条件重试提交。调用者负责确保FIFO写期间没有其他ISR/处理器并发操作。

## 验证记录（2026-10-27）

TDD的实际RED/GREEN均已观察：静态地址首次1失败→2通过；PDU首次2失败→4通过；RF顺序/最终采样/超时/错误首次4失败→8通过；广告身份/参数/触发/FIFO/状态首次4失败→13通过；配置验证/顺序/超时首次3失败→16通过。所有测试命名 `test_should_*`。测试用有序字节寄存器fixture，断言算法发出的访问、预算、完整PDU和失败时没有后续副作用；不使用真实设备。

本crate `cargo fmt -p syd881x-radio`、`cargo test -p syd881x-radio`（16测试）、`cargo clippy -p syd881x-radio --all-targets -- -D warnings`、`cargo check -p syd881x-radio --target thumbv6m-none-eabi`通过；实现后 `cargo test --workspace` 全部47测试通过。硬件/独立冷启动/外部空口验证未由本crate实现者执行，须查看主线实机交付记录。

## 初始化硬件子集

`Radio::initialize(&ConfigurationTable)` 依据 `target/hardware/2026-10-27/probe-init-analysis/PROBE-INIT.md` 增加如下有限硬件子集，仍不调用ROM或启用IRQ：

1. 在任何MMIO前验证 `header[1]&15 <= 8` 且 `(header[1]>>4)&7 <= 2`；其余头部字节不作新的时钟/trim解释。只复用已经完整校验的本板记录。
2. ROM `0x5fec..0x5ff8` 支持 `LL[0] |= 1`。随后最多4096次读取等待bit0自清是**探针新增观测门槛**，ROM没有该等待；不是已证明的硬件复位时序规范。
3. 按 `0x0b82..0x0bbe`、`0x0b4c..0x0b7e` 选择RF bank0，RF[3d] OR40，RF[3b]=1，最多256个完整RF读取检查bit1；成功后以及失败清理中均尝试bank0 RF[3d]&bf。RF桥本身每笔事务仍受独立预算限制。
4. MCU时钟8写bank0 RF[40]=1；0..7无条件写bank0 RF[40]=0，然后保留PWR[0]高5bit并设置低3bit。依据 `0x6b44`，无条件退出XO路径是Rust消除ROM RAM时钟缓存依赖的确定性组合，不能描述成ROM逐指令重放。未虚构时钟稳定延时。
5. 按原始顺序执行全部本板记录。随后按 `0x0b18..0x0b4a` 选择RF bank1，RF[0a]=2，最多256个完整RF读取要求 `(value>>2)&3 == 1`；这项校准不擅自命名为SDK的LPOCalibration。
6. 按 `0x6bf0` 选择RF bank2，保留RF[0]高6bit写低2bit的LPO字段；选回bank0并按LPO 0/1/2分别写RF[3/4]=(0,0)/(15,0)/(0,15)。这里的字段是bits4..6，修正旧boot报告的bits5..6。
7. 清LL[1a]低3bit、PWR[0a] bit0、PWR[8] bit5。LL bit0/1有 `0x5766(0)` / `0x574e(0)` 路径，bit2来自同版寄存器布局；PWR[0a]为FSM_SLEEP_EN（`0x6598`），不是“RF上电”；PWR[8] bit5为RFMODE_MANUAL，清位仅选择硬件模式，不代表模拟RF从DTM/载波状态完全复位。

成功返回 `InitializationReport`，只含 reset/pre/post gate 的实际poll次数。失败返回 `InitializationError` 的 `stage` 和 `cause`；复位超时、前/后校准超时分别保留末次status及poll次数，桥超时通过 `Radio(RfTimeout)` 单独报告。硬件错误后再尝试有界RF bit6清理（复位阶段失败尚未触RF时跳过），并清LL[10] ADV_EN；`calibration_cleanup` / `advertising_cleanup` 分开记录后续清理失败，不覆盖主错误。不支持的头部在任何硬件访问前直接失败。

调用者必须先关闭全局IRQ及NVIC相关IRQ，提供自己的RAM栈/状态，记录初始化报告并在失败时停止进入广告。这个API不复位模拟RF；运行APP中暂停后注入的执行始终是 **warm analog state**。冷验证必须由主线建立SoC硬复位、ROMBoot-only且未运行原APP的前置状态，并独立验证空口。

新增7个初始化行为测试实际观察RED（6失败/1通过）→GREEN（7通过）。包含按序完整写入、MCU/LPO确定性分支、未知clock预拒、4096复位预算、前后256校准预算、bit6失败清理、桥超时与secondary清理错误。加上前述16测试共23测试。主线独立硬件验证不由这些主机fixture替代。

实机续测发现原APP可能停在manual sleep，故新增 `prepare_standby(settle_us)`：先关闭LL/PWR自动睡眠，再按ROM0x4712/0x6a46的顺序请求待机复位、等待100µs、LL复位、等待500µs、选择手动待机。调用者提供满足最小时间的延时；不访问RF桥。新增状态/掩码/延时行为测试从缺方法RED到GREEN，当前本crate共24个测试。探针额外等待2ms后再初始化，在PWR[8]=0x22前态实际通过；这仍是已有APP运行后的warm模拟域，空口尚未验证。最新结果见硬件记录。

后续Mac接收实验已收到同一探针的`RUST-PROBE`不可连接广播，完成独立接收验证。冷启动、原始空口频道/CRC字段和RX/连接尚未验证；详细接收计数及对照见上述硬件记录。
