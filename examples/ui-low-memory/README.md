# 应用级 UI 省 RAM 开关

这是受维护的独立固件消费者示例，提供可测试的 `no_std` 库，不含板级入口、显示驱动或
堆分配。它有自己的 `[workspace]`、锁文件和 `publish = false`，依赖使用仓库相对路径。
框架库未新增 `ui-low-memory` feature，原有默认功能与 `StreamTarget::new` 的 RUN=32 不变。

应用的 `default = []`，`ui-low-memory = []`。唯一的 `UI_PIXELS` 编译期常量同时决定
`App` 内的真实 `[Rgb565; UI_PIXELS]` 与 `StreamTarget::<_, UI_PIXELS>`：

| 应用档位 | 实际像素容量 | RGB565 scratch | 64×8 刷新 write／service | 64 像素流式 write |
| --- | ---: | ---: | ---: | ---: |
| 默认 | 32 | 64 B | 各 16 次 | 2 次 |
| `ui-low-memory` | 8 | 16 B | 各 64 次 | 8 次 |

`Auto` 根据真实工作区选择容量。`Fixed(UI_PIXELS)` 可用；更大的 `Fixed` 会返回原始
`WorkspaceTooSmall`，不改变已有数组或会话配置。只降低 tile 上限不会缩小已有数组。
流式临时数组在绘图函数中，scratch 属于调用方；不能将它们或各函数局部帧直接相加，
宣称整个固件的最坏栈峰值。具体历史测量见 [UI 内存档位验证](../../docs/ui-memory-profiles.md)。

## 从仓库根目录运行

先安装仓库指定工具链及 `thumbv6m-none-eabi`。新环境可显式联网获取依赖一次：

```sh
cargo fetch --manifest-path examples/ui-low-memory/Cargo.toml --locked
```

随后分别测试两个编译状态并构建 ARM 库；命令均使用示例自己的锁文件，且离线执行：

```sh
cargo test --manifest-path examples/ui-low-memory/Cargo.toml --offline --locked
cargo test --manifest-path examples/ui-low-memory/Cargo.toml --offline --locked --features ui-low-memory
cargo build --manifest-path examples/ui-low-memory/Cargo.toml --release --target thumbv6m-none-eabi --offline --locked
cargo build --manifest-path examples/ui-low-memory/Cargo.toml --release --target thumbv6m-none-eabi --offline --locked --features ui-low-memory
```

也可运行 `python3 scripts/check_ui_low_memory.py`。检查器校验独立 workspace、禁止发布、
相对依赖与功能解析，分别执行两个档位的主机测试和 ARM 构建，再比较最终图像校验值；
构建缓存和结果写入仓库 `target/ui-low-memory/`。主机测试逐像素对照独立计算的四组
64×8 图像，并检查真实数组大小、写入上限／次数、错误、显式重试、Idle 与延后失效请求。

## 接入固件

把该示例作为应用装配参考：将本地 `ui-low-memory = []` 添加到固件的 `[features]`，
保留固件已有默认功能；使用同一个 `UI_PIXELS` 创建数组与流式适配器。具体显示实现、
资源和 service 的耗时由固件负责。库的 `refresh` 功能需要显式启用。

`App::new(dimensions)` 创建 state 和真实 scratch；`app.session(RefreshSettings::new())`
借用它们。跨帧保留 session，才能保留 Idle 状态、失败后的完整重绘请求与 service
登记的下一帧失效区域。失败后由调用方显式决定何时重试；不要重建会话来假装恢复。

`render_once` 展示完整的一次性装配，依次验证、流式绘制第一行、刷新一帧。其
`RenderFailure.error` 中的 `RenderError::{Config, Stream, Session}` 区分阶段并保留原始
显示／service 错误；`RenderFailure.pending` 返回失败后完整待处理区域，供调用方显式
安排重试。成功的 `FrameReport.pending` 则报告尚未消费的未来请求，不会在同次调用绘制
第二帧。配置拒绝尚无有效会话，pending 为 None。需要持续跨帧或恢复时，直接使用持久
session。内置场景为 Infallible；带资源读取等可失败场景可通过持久 session 的 draw 传入。

在**固件工程根目录**，完成上述 feature 和数组装配，并维护自己的锁文件后执行：

```sh
cargo build --release --target thumbv6m-none-eabi --offline --locked
cargo build --release --target thumbv6m-none-eabi --offline --locked --features ui-low-memory
```

Cargo feature 会在同一构建依赖图中合并：若其他依赖启用 UI 的 layout、damage 或
frame-pacing，`default-features = false` 不会关闭它们。这些附加字段／代码需要重新测量。
应用级 RAM feature 只选择本应用的容量，不会全局重设其他使用者或默认构造器。两档需要
分别构建；`--all-features` 只覆盖启用省 RAM 的一档。

ARM 构建证明库可用于该目标；它没有链接完整板级固件，也没有验证硬件栈水位、总线时序、
真实刷新率、功耗或所有中断调用链。
