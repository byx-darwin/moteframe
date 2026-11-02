# Assets/input 实现、兼容与资源核算

本轮落实架构评审：先收紧 Debug 与公共契约，再新增两项可独立选择的通用能力。
不迁入产品业务、原素材、厂商库、寄存器或设备转储；不接硬件。

## 依赖与兼容

| 新组件 | 生产依赖 | 外部适配责任 |
| --- | --- | --- |
| moteframe-assets | core::ReadAt、embedded-graphics-core | 字库／目录、资源身份、内容稳定、reader 和实际存储 |
| moteframe-input | 无 | 真实采样、坐标映射、阈值选择、时间源、跨中断同步、页面动作 |

现有 core/UI/display/storage/BLE/I2C 没有新增模块的反向依赖，旧 trait、错误值、默认
feature 和对象字段保持不变。Debug 文本特意改为脱敏元信息，不作为稳定序列化格式。
调用方的完整后端错误仍保留，应用必须审查其日志输出。

UI 的 ResourceKey/Metadata/View 保持原位置和 API，不复制到 assets。组合测试把
不可变 Raster 放入 ResourceView，闭包独立借用可变 ReadWindow 和 scratch；读取错误
成为 scene error，完整 dirty 区域保留，重试可得到手工推导的精确像素。
内容变化显式切换 revision；旧 view 被拒绝，随后新版本整帧重绘。

## 行为测试

- Debug：当前字节、clear/take 后旧字节、长后短复用、嵌套 provider、无 Debug 的后端／target。
- assets：RGB565 BE/LE、RGB888、Gray8、单色及自定义非 Default 颜色；不同 scratch
  输出相同；dyn ReadAt、非法 decoder 长度、零字节格式、空几何、溢出、跨行、截断、
  scratch 不足、首读错／解码错停止和 glyph 指标。
- input：FIFO／回环、N=0、去抖抖动和精确边界、一次长按和禁用长按、按下／长按／
  释放／取消队满后的原样重试、非法坐标与顺序、时钟回退、四方向滑动、移动再返回
  不能变成点击、无 tick 的释放判长按及 tick 后不会重复发手势。
- 固定消费者验证 2／16 B scratch 输出等价，以及 1／4／16 槽的相同已消费输入序列；
  off 消费者带／不带新增库的链接段和输出一致。

## Cortex-M0 固定消费者

Rust 1.96，opt-level=s、fat LTO、单 codegen unit、panic=abort，与旧成本门禁相同。
每个消费者在宿主执行八组输入，再读取真实 ARM 链接段、对象布局和局部栈帧。
命令：`make assets-input`；原始报告：`target/assets-input/resources/report.json`。

| 消费者 | 链接 Flash | 静态 data+bss | exercise 局部帧 |
| --- | ---: | ---: | ---: |
| 未使用新增模块 | 64 B | 0 B | 12 B |
| 素材，2 B scratch／32 RGB565 像素 | 482 B | 0 B | 288 B |
| 素材，16 B scratch／32 RGB565 像素 | 1120 B | 0 B | 304 B |
| 输入，1 槽按键＋1 槽触摸队列 | 1486 B | 0 B | 200 B |
| 输入，4 槽按键＋4 槽触摸队列 | 1598 B | 0 B | 232 B |
| 输入，16 槽按键＋16 槽触摸队列 | 2318 B | 0 B | 384 B |

Raster 为 16 B，Button／Touch 各 56 B。按键队列 1／4／16 槽的布局为
12／12／24 B；小容量可能因对齐而相同，不能推断队列扩容免费。
触摸队列 1／4／16 槽分别为 16／32／104 B，Glyph 为 24 B，当前切片来源的只读
窗口为 16 B，均由编译器 marker 读取。Input 消费者同时包含两种队列。
像素输出为 64 B，scratch 为实际 2／16 B，没有库内固定整屏或 256 B 数组。

这些数值包含合成数据源与消费逻辑；Flash 差异也受优化决策影响，不能据此断言刷新速度。
对象、数组已经可能包含在局部帧中，不得再次相加。局部帧不能代替完整调用链、动态
后端、IRQ 栈或固件峰值；静态 data+bss=0 不代表没有 RAM 消耗。

## 原有门禁

旧 core/I2C 冻结 JSON、源摘要与历史资源预算保持原字节。只追加从原基线图到当前
八 crate 的精确 Cargo manifest/lock 迁移，继续拒绝其他编译器／profile／probe／配置
变化及旧消费者资源正增量。新测量发现原布局解析器会混淆同名前缀 marker，已先用
行为回归复现再修复；不改原布局或成本值。

完整验证执行 `make check build supply-chain secrets hooks`，覆盖源文件／包边界、
主机及 ARM、旧消费者、UI feature 组合与功能合并、独立 crate、合成像素和新内存探针。
本轮不发布版本，不作硬件资格声明。

## 交付记录（2026-11-03）

- `make check build supply-chain secrets hooks` 执行成功，包含主机／ARM lint、Rust
  测试、文档、96 项 Python 回归、包边界、离线像素、功能消费者及内存探针。
- 最终源码复验：175 项 Rust workspace 测试和 96 项 Python 测试通过；补充项完成后，
  重新通过 fmt、host/ARM lint、test、doc、boundary、build 和 packages。资源报告源／probe
  摘要与最终文件一致。
- UI 组合消费者 30 项通过，覆盖 16 种功能子集、预期编译拒绝、所有权与 feature 合并；
  原有 core/I2C 资源增量门禁通过，冻结基线文件未修改。
- 独立代码评审未发现 Critical/Important 项；补充了释放优先于同刻长按的规则与测试，
  以及解码第二像素失败后停止读取、保留整区重绘和修复 revision 后重画前缀的组合测试。
- 源码候选分支 `codex/assets-input-foundations`。离线交付后，用户另行授权提交、
  合并到 main 并推送；集成结果以 Git 记录和远端检查为准，未创建发行版。
  原设备工程和硬件均未改动。详细本地运行日志位于
  `target/assets-input/offline-regression.log`，不作为源码包内容。

## 后续扩展边界

初版 raster 是未压缩、固定字节宽格式，Mono8 不是位打包字库；字符映射、压缩、索引色
与完整字体排版未实现。Input 是单触点；缺少 Down/Up 的厂商手势不能凭空转换为原始流。
写擦／原子持久化、通用 framing、BLE Host/Controller、调度执行器和传感器算法不纳入本轮。
