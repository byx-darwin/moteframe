# UI 配置 r0.2 实现验收记录

开发阶段记录，2026-11-02。实现位于 `crates/moteframe-ui`；对应
[批准的设计](ui-configuration.md)和[开发步骤](plans/2026-11-02-ui-configuration.md)。
没有业务代码、产品固件装配、硬件访问、原厂资源或厂商库。此处记录开发阶段
离线证据，后续用户已明确授权把通用源码提交并推送 GitHub；不包含包／固件发布。

## 已实现的接口和行为

| 层 | 类型／入口 | 已落实行为 |
| --- | --- | --- |
| 基础样式 | Palette、UiSettings、UiState | 显式颜色、私有有效字段、候选整体校验后应用、拒绝非自反颜色比较 |
| 更新分类 | UpdateImpact | Unchanged／PolicyOnly／FullRedraw，不清除以前的请求 |
| 资源 | ResourceKey、ResourceMetadata、ResourceView | 集合＋修订号、空键保留、尺寸元数据校验、帧前匹配、整帧不可变借用 |
| 布局（可裁） | Padding、LayoutSettings、UiContext::stack | 稳定整屏／内容区域、gap/padding 缺省 0、显式绘制参数优先 |
| 刷新（可裁） | RefreshSettings、RefreshSession | 独占工作区、Auto=min(容量,32)、Fixed 超容量报错，原子更新 |
| 绑定／恢复 | DisplayBinding、BindingState、try_rebind | 首次验证接受代次、失配锁定 NeedsRebind、显式重绑定成功后整屏 |
| 后续请求 | DeferredInvalidation | 一矩形并集；整屏请求等价于全屏并集；帧后合并，单次最多一帧 |
| 可选调度 | FrameInterval、PacingState、draw_timed | 开始时间计入失败、Deferred 零绘制、时钟回退新纪元、启停清锚点 |
| 流式容量 | StreamTarget〈D, const RUN〉 | 1..=65535 编译期检查；容量不由运行时申请 |

第一版保持低层 repaint、差分和显式 Stack 接口。默认功能组合保留原有能力，
不要求现有用户迁移成全局配置或 session。关闭 refresh 后不创建隐藏 dirty、
工作区或节流状态，调用方消费 UpdateImpact 自行绘制。

两个 API 落实选择：

- `StreamTarget::new` 保留默认 RUN=32 的类型推断；显式容量用
  `StreamTarget::<_, 8>::with_run`，避免泛型构造器使旧代码无法推断容量。
- `draw` 的签名不随 feature 合并改变。可选 `draw_timed` 接受微秒时钟；
  普通 draw 遇到需要节流的 dirty 工作返回 ClockRequired，无法绕过间隔。

这些选择保留 r0.2 的生命周期／时间契约，没有增加设备或执行器依赖。

## 最小使用示例

```rust
use moteframe_core::Dimensions;
use moteframe_ui::{Palette, ResourceView, UiSettings, UiState};
use moteframe_ui::graphics::{pixelcolor::Rgb565, prelude::RgbColor};

let colors = Palette::new(
    Rgb565::BLACK, Rgb565::WHITE, Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE,
);
let candidate = UiSettings::new(colors);
let resources = ResourceView::empty(&());
let mut ui = UiState::new(candidate, Dimensions::new(240, 240), resources)?;
let impact = ui.try_update(candidate, resources)?;
```

有刷新功能时，创建 session，提交实际驱动和调用方维护的非复用 generation：

```text
session = RefreshSession::new(&mut ui, &mut pixels, RefreshSettings::new())?
session.draw(DisplayBinding::new(&mut display, generation), resources,
             |tile, context| render_stable_scene(tile, context),
             |next| { service_other_work(); next.invalidate(next_damage); Ok(()) })?
```

上述第二段表示装配关系，具体驱动、错误类型和 service 由调用方提供；不是硬件示例。
scene 按 context 整屏区域布局，Tile::bounding_box 只用于当前裁剪。

## 验收覆盖

- 原子性：错误尺寸、padding 溢出／占满内容区、无效元数据、资源不匹配、空区与
  Fixed 超容量，均不替换旧设置／策略；场景错误不被吞掉。
- 绑定：首次资源失败仍 Unbound；首次尺寸失配锁定；干净时检查代次；相同尺寸换代、
  零尺寸、缩小、错误候选与显式恢复，禁止旧几何继续写屏。
- 刷新：首／末块 scene、display、service 六个错误位置；成功和失败均合并未来请求；
  连续新请求不使单次调用循环；FullOnRequest 干净时不持续重画。
- 画面：不同 tile_pixels（1、2、5、8）与显式颜色得到相同手工预期像素。
- 资源读取：通过 ResourceView 借用 ReadAt 提供者，使用两字节窗口，逐一在六次真实
  fixture 读取中注入失败；错误输出被丢弃，重试完整帧；另有实际非法字节解码失败。
  此 fixture 验证接口组合，不是生产资源格式、压缩器、缓存策略或具体 Flash 驱动。
- 调度：100 失败→101 延期→110 重试→90 回退→91 恢复；零时间、u64 回绕、同值更新、
  非零间隔变化、样式／rebind 不清锚点、禁用再启用、Idle／无效环境零时钟调用。
- 裁剪：16 种请求组合分别运行 crate 测试并编译独立 Cortex-M0 消费者；关闭的 API／
  字段不能使用；两条依赖路径合并后普通 draw 仍可编译。
- 所有权：有效字段私有、state／scratch 独占借用、错误颜色、配置／资源生命周期、
  RUN=0／65536 均按预期原因编译失败。总计 30 个独立消费者用例。

完整命令和结果存放在本地忽略目录 `target/ui-config-implementation/`。
实际执行结果见末节；不把此覆盖清单当作执行结果。

## ARM 内存和链接核算

工具链固定 Rust 1.96.0、thumbv6m-none-eabi、opt-level=s、panic=abort，
链接消费者使用 fat LTO。共七组消费者；各自实际消费启用能力，工作量不同，
不是相同画面的性能对比，也不是“框架唯一占用”或产品固件大小。

尺寸单位为字节，RGB565 的逻辑颜色大小为 2；链接统计只计分配段。
只读桶包含 `.ARM.exidx` 等只读分配段；测量用 layout 静态数组在最终链接前排除。

| 组合 | Settings | State | Session | Policy | text | 只读 | data | bss |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| base | 20 | 24 | — | — | 200 | 16 | 0 | 0 |
| layout | 28 | 32 | — | — | 676 | 16 | 0 | 0 |
| refresh | 20 | 24 | 48 | 4 | 2966 | 16 | 0 | 0 |
| damage | 20 | 24 | 48 | 4 | 3306 | 16 | 0 | 0 |
| pacing | 20 | 24 | 88 | 8 | 3334 | 16 | 0 | 0 |
| default | 28 | 32 | 48 | 4 | 3950 | 16 | 0 | 0 |
| all | 28 | 32 | 88 | 8 | 4230 | 16 | 0 | 0 |

基本颜色设置的 ARM 尺寸：无布局时 RGB565=20、RGB888=28、Gray8=16、BinaryColor=16；
有布局时分别为 28、40、24、24。原始报告同时记录 host 尺寸，不能拿 host 指针大小
代替 ARM。没有要求颜色类型实现 Default 或存储编码转换。

分项预算：

- 默认配置下 State=32、Session=48，32 像素 RGB565 工作区=64，总持有量为 144；
  session 借用 state 和数组，不能再次把数组作为 session 内嵌存储重复统计。
- 所有功能下 State=32、Session=88，同一工作区64，总持有量184。PacingState=32，
  已包含在88内；其中有两个 Option〈u64〉，不能仅按两个 u64 的16字节估算。
- 更新瞬间调用方另有 Settings 候选28与 Policy 候选4（节流开启为8）；未计入常驻
  总量。优化器可能消除／重用临时空间，逻辑对象预算不等于编译器栈峰值。
- Context=28、资源视图=20、资源元数据=8、资源键=8、帧后请求累加器=14。资源视图
  已包含键和元数据，不再相加；目录内容由调用方另计，借用不等于零开销。
- RUN=8 的 RGB565 临时数组=16，RUN=32=64。它们属于绘图调用临时量；小 tile 不
  缩小已经声明的数组。240 像素宽两行差分区=960，独立于小块刷新工作区。
- 七组探针 data/bss 均为0，表示该消费者不引入静态可变分配，不表示运行时无需RAM；
  上述 state、session、资源、数组和临时量仍占用调用方内存。

报告还记录编译器 object 单函数栈帧。探针最大本地帧从 base 的144到 all 的480；
它包含调用者场景和工作数组，不是 UI 常驻内存，也不是完整最坏栈。
已知控制流程为 exercise → draw/draw_timed → draw_inner → repaint_cooperative，
后者依次执行 scene、同步 display.write、service；三者不是同时叠在彼此下面。
不得把所有函数帧简单相加；报告未证明任意驱动／场景回调、链接优化、中断叠加或
产品完整调用链上界。没有接硬件，未给出设备栈水位、耗时、功耗或刷新帧率。

原始文件：`target/ui-config-implementation/memory/report.json`；
`make ui-config` 可重建消费者和报告，无需设备或外部素材。

## 首轮独立审查与开发执行结果

新上下文只读审查未发现需修复的正确性或安全问题。审查指出的“小窗口读取与真实
资源故障证据不足”已补上两项fixture集成测试；独立复验9项session测试通过；随后增加的背景／资源版本消费用例也通过，共10项。
未把 fixture 描述为生产 Flash 适配或素材格式实现。

最终执行结果：

| 检查 | 结果 |
| --- | --- |
| workspace 全功能 Rust 回归 | 82 个独立用例通过（既有63＋新增19） |
| Python 回归 | 47 个用例通过 |
| 16 种功能子集 | 各自 crate 测试及独立 ARM 消费者通过 |
| 拒绝使用／feature 合并 | 14 个补充消费者通过；含期望原因的编译失败 |
| 七组 ARM 内存／链接探针 | 已生成布局、链接段和单函数帧原始数据 |
| host／ARM Clippy、Rustdoc | warnings denied 通过 |
| Cargo 包文件及依赖／来源边界 | 四个 crate 通过 |
| 原有离线图像 | 与颜色改造基线逐字节一致 |
| Cortex-M0 release library build | 通过 |
| pre-commit 全部适用检查、依赖政策与密钥扫描 | 通过 |

`make check build`、`make hooks` 和 `make supply-chain secrets` 均已执行；
每 crate 和每功能组合的重复执行不作为额外独立测试数量。
依赖政策基于本地已缓存公告快照，未进行网络公告刷新。
开发阶段结束时 Git 索引没有暂存项，既有颜色改造保留；另外三个 runtime crate 相对本轮前快照
逐文件未改变。硬件效果、产品固件和实测栈水位留在后续授权范围。

后续多角色复验见 [UI 配置 r0.2 多角色复验](ui-configuration-review-r02.md)。
该复验修复编译拒绝门禁的误报风险，未改变 UI 运行时代码；最新回归数量单独记录。
