# UI 内存档位离线验证

历史测量日期：2026-11-02；维护示例补充：2026-11-03。下列资源数字保留原固定消费者、
编译器与配置的离线结果，不是当前维护示例的新测量，也不是任意固件的预算。
历史研究验证现有 UI 接口能否配置省 RAM 档，未修改 UI 运行时或全局默认值，
未接硬件。结论是：通过编译期选择实际工作区与流式缓冲容量，可以提供默认 32 像素／
省 RAM 8 像素两档；只降低刷新块上限不会缩小已有数组。

## 同画面验证

目标为 Cortex-M0 `thumbv6m-none-eabi`，Rust 1.96.0，opt-level=s、codegen-units=1、
panic=abort，最终链接使用 fat LTO。源文件、锁、编译条件一致，只改变明确的容量／存储装配选项。

固定场景为 64×8 RGB565 图像，另执行一行 64 像素的流式绘制。两阶段都保留可观察的颜色
校验值；主机还记录实际最终像素，验证它与已接受的逐像素输出一致。四组 seed 使用独立计算
的预期图像校验值，不能靠跳过绘制使小缓冲获得更好测量结果。

本轮共 14 组：默认功能／编译启用 frame-pacing 两组，各自比较默认、仅 tile=8、仅工作区=8、
仅 RUN=8、工作区与 RUN 同为 8、调用方结构打包、打包后的 8 像素档。frame-pacing 组使用
Unlimited 策略，不采样时钟；它验证字段／代码组合，不表示测过真实帧间隔耗时。

## 默认功能的最终结果

单位均为字节。UI 对象预算为 UiState＋RefreshSession＋实际 scratch，不包含显示驱动、
资源提供者和其他临时量；session 已含引用，scratch 只计算一次。

| 档位 | scratch | StreamTarget 临时颜色数组 | UI 对象预算 | exercise 局部帧 | 链接 Flash | 本轮全屏 write／service 次数 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 默认：32 像素工作区、RUN=32 | 64 | 64 | 144 | 288 | 1452 | 16 |
| 仅设置 tile=8，仍用 32 像素数组 | 64 | 64 | 144 | 288 | 1452 | 64 |
| 省 RAM：8 像素工作区、RUN=8 | 16 | 16 | 96 | 240 | 1290 | 64 |

因此，本组省 RAM 档的对象预算少 48，exercise 的局部帧少 48，链接 Flash 少 162。
代价是这张相同图像的 write 和 service 调用各从 16 增至 64。没有计时，不能据此宣称
刷新耗时恰为四倍；真实速度还取决于驱动、总线、场景和回调。

补充对照：

- 只把 scratch 缩为 8、保留 RUN=32，UI 对象预算也是 96，exercise 帧为 240。
- 只把 RUN 缩为 8，scratch 仍为 32，UI 对象预算仍为 144，exercise 帧仍为 288；
  StreamTarget::draw_iter 自身的诊断局部帧从 136 降至 88。
- 把调用方的 state 和 scratch 打包到一个结构，未改变总对象预算；默认组的 exercise
  帧反而从 288 增至 304。这仍是 session 借用结构字段，不能称为“拥有型 session”。
- 编译启用 frame-pacing 时，32／8 像素档的对象预算分别为 184／136，exercise 帧
  分别为 384／336，链接 Flash 分别为 1480／1314。功能裁剪与缓冲档位是两个独立选择。

局部帧采自诊断 object，并不包含被调用者、IRQ 或最终 LTO 后的完整调用链。
不要把不同函数帧或两个缓冲数组机械相加，解释成整个固件的最坏栈峰值。
本组 data/bss 均为 0，表示没有引入静态可变分配，不表示上述调用方对象和栈不占 RAM。

## 其他颜色的实际数组大小

由同一目标编译器核算原生逻辑颜色数组，尚未扩展物理显示驱动：

| 颜色 | 32 像素 | 8 像素 | 少占 |
| --- | ---: | ---: | ---: |
| Rgb565 | 64 | 16 | 48 |
| Rgb888 | 96 | 24 | 72 |
| Gray8 | 32 | 8 | 24 |
| BinaryColor | 32 | 8 | 24 |

BinaryColor 的这里是 UI 原生颜色数组，未按屏幕传输格式进行位打包。
上表不代表 RGB888／灰度／单色物理面板已适配。

## 受维护的应用级开关

[examples/ui-low-memory](../examples/ui-low-memory/README.md) 将两档装配纳入源码与持续检查。
它是有独立 `[workspace]`、`Cargo.lock`、相对路径依赖和 `publish = false` 的应用消费者。
运行时为 `no_std`、无堆并禁止 unsafe；主机测试的像素记录仅是离线显示替身。
库本身未新增 `ui-low-memory` feature，也未改变 `StreamTarget::new` 的默认 RUN=32。

示例的 feature 为 `default = []`、`ui-low-memory = []`，仅有一个容量常量：

```rust
const UI_PIXELS: usize = if cfg!(feature = "ui-low-memory") { 8 } else { 32 };
```

该常量既创建 `App` 的实际 `[Rgb565; UI_PIXELS]`，也实例化
`StreamTarget::<_, UI_PIXELS>::with_run(display)`。Auto 以真实容量与 32 的较小值绘制。
`Fixed(UI_PIXELS)` 接受，`Fixed(UI_PIXELS + 1)` 在开始时拒绝，已有会话的超容量更新
也不会改变有效配置或待绘制请求。降低 tile 上限无法缩小已经分配的数组。

| 维护示例 | 默认档 | 省 RAM 档 |
| --- | ---: | ---: |
| 真实 scratch 像素 | 32 | 8 |
| RGB565 scratch 字节 | 64 | 16 |
| 64×8 全帧 write／service | 各 16 次 | 各 64 次 |
| 64 像素首行 stream write | 2 次 | 8 次 |

这些尺寸和次数由维护测试验证；未重新宣称上方历史研究的链接 Flash 或局部帧数字。
`App::session` 借用 state 与真实数组，跨帧持有 session 可保留 Idle、失败重试和 service
延后登记的请求。一次性 `render_once` 分别返回原始配置错误、stream 写错误或完整
`SessionError`，不把传输／service 失败伪装成 `InvalidResources`。`FrameReport.pending`
报告成功后未消费的未来请求；错误由 `RenderFailure` 携带原始阶段／cause 和完整 pending
区域，调用方可显式安排重试。配置拒绝尚无有效会话，pending 为 None。内置场景是
Infallible；其他可失败场景通过持久 session 的 draw 传入，不增加另一套重试策略。

从**仓库根目录**执行；首次准备依赖可显式先执行 `cargo fetch --manifest-path
examples/ui-low-memory/Cargo.toml --locked`：

```sh
cargo test --manifest-path examples/ui-low-memory/Cargo.toml --offline --locked
cargo test --manifest-path examples/ui-low-memory/Cargo.toml --offline --locked --features ui-low-memory
cargo build --manifest-path examples/ui-low-memory/Cargo.toml --release --target thumbv6m-none-eabi --offline --locked
cargo build --manifest-path examples/ui-low-memory/Cargo.toml --release --target thumbv6m-none-eabi --offline --locked --features ui-low-memory
python3 scripts/check_ui_low_memory.py
```

检查器将日志、固定构建缓存和结果保存至 `target/ui-low-memory/`。它验证独立 workspace、
禁止发布、相对依赖与实际功能解析，再分开执行两档的完整主机测试及 ARM 库构建，比较
四组场景的最终图像校验值。每组图像另由主机测试逐像素对照独立的预期输出。

接入**外部固件**时，在固件现有 `[features]` 中添加 `ui-low-memory = []`，保留已有
默认功能；显式启用 UI 的 refresh，并像示例一样用同一常量控制真实 scratch 与 RUN。
完成这些装配并维护固件自己的锁文件后，在固件根目录分别执行：

```sh
cargo build --release --target thumbv6m-none-eabi --offline --locked
cargo build --release --target thumbv6m-none-eabi --offline --locked --features ui-low-memory
```

Cargo feature 在同一构建依赖图中合并。若其他依赖启用 layout、damage 或 frame-pacing，
本应用的 `default-features = false` 不能关闭它们，需要按实际字段／代码重新测量；
frame-pacing 的历史结果也表明它会影响预算。应用级容量选择不会全局改变其他实例，
`--all-features` 只构建省 RAM 档，不能替代两个独立命令。

## 历史验证记录与当前限制

每组执行四种输入，并检查：

- 流式输出、最终画面与独立预期一致，完整帧像素数固定为 512。
- write／service 次数符合所选容量，单次最多处理一帧。
- 同一坐标注入写失败后完整请求保留，重试成功后像素一致。
- service 登记下一帧请求后帧后合并，再次调用才能处理，不在同次调用循环消费。
- Idle 不调用绘制或 service，Fixed 超容量立即拒绝。

历史研究另用独立应用消费者真实编译了开关关闭／开启两种 Cortex-M0 组合。
既有 UI 全功能测试的 63 个维护用例也重新通过；临时场景不累加到维护用例数。
这些都是离线软件证据，不是完整固件 RAM 上界、设备栈水位、刷新率或功耗结论。

本地临时复现脚本与原始结果：

- `target/ui-memory-profile-study/run.py`、`probe.rs`
- `target/ui-memory-profile-study/report.json`
- `target/ui-memory-profile-study/app-switch/`：应用级开关示例及两种编译结果

上述历史 target 产物仍未进入源码包或持续 CI；可复现入口现在是受维护的
`examples/ui-low-memory/` 与 `scripts/check_ui_low_memory.py`，不依赖历史临时文件。
ARM 库构建不是完整板级链接或硬件测试；没有自动迁移外部设备工程，也没有增加全局 UI
功能开关。真实栈水位、IRQ 调用链、刷新率、功耗和电气行为仍需固件／设备侧验证。
