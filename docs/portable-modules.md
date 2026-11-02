# 存储／BLE：扩展、兼容与资源证据

三个角色分别评估了存储边界、BLE 适配及 RAM／发布兼容性。本次新增两个独立 crate
和一个应用级 RAM 示例；历史“首批未纳入存储／BLE”是当时的抽取范围。

| 模块 | 依赖 | 设备工程负责 |
| --- | --- | --- |
| moteframe-storage | 现有 core::ReadAt | 具体 Flash 驱动、写擦、文件系统、格式和产品持久化 |
| moteframe-ble | 无运行时依赖 | BLE 栈／射频、连接安全、GATT 数据库、平台队列和产品协议 |
| ui-low-memory 示例 | core + UI refresh | 板级显示、资源、服务和具体时序预算 |

core／UI／display 不依赖新模块，固件按 Cargo 依赖选择能力。运行库继续 no_std、无堆、禁 unsafe。

## 存储视图

SliceSource 校验切片容量可用 u32 表达并提供精确读取。ReadWindow 建立固定逻辑范围，
每次非空读取前验证长度转换、本地／映射范围和后端当前容量，然后只委派一次。后端增长
不会扩大窗口，缩小后受影响的非空请求会被拒绝。逻辑范围内空读不调用后端，即使后端
后来缩小；越过逻辑末尾的空读也拒绝。

ReadRef 显式支持 borrowed/dyn ReadAt，未给旧 core trait 添加可能冲突的 blanket impl。
WindowCreateError 保留失败构造的原 source，无需 source Debug；error()/into_parts()
和窗口 into_inner 只转移资源，不执行读传输。Source 错误原样保留，错误输出整体丢弃，
无隐藏 retry。有限完成和真实读取能力由适配器保证。
API 示例见 [storage crate](../crates/moteframe-storage/README.md)。没有写擦、codec 或缓存。

## BLE 值处理

CheckedNotifier 每次查询当前目标容量，检查完整长度后最多提交一次。Target 应包含连接
代次、特征／数据库身份及 bearer，不能只用会复用的裸连接句柄。查询验证身份与订阅，
提交时后端再次核对身份／容量以处理竞态。原始错误保留，无隐藏重试或缓存全局 MTU。
NotificationRef 显式支持借用/dyn；new/into_inner 不操作后端，也不复制 BLE 栈。

QueueAccepted 仅表示后端接受或复制输入，切片可立即复用；不代表空口完成或对端确认。
ATT 属性值最多 512 字节，通知还受 bearer／特征／栈当前限制，512 不是保证可发送长度。
依据：[Bluetooth Core 5.4 ATT](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-54/out/en/host/attribute-protocol--att-.html)。

WriteSlot<T,N> 保存一次完整且已提交的接收写。槽满、超长或输出不足均不截断、不覆盖
原事件；输出不足也不修改目标数组，成功仅写有效前缀。零长度事件区别于没有事件；clear
显式丢弃，不把旧 Target 改成新连接。N=0 支持空事件；N>512 可分配，但有效 payload
仍限制为 min(N,512)。没有默认强制分配 512 字节、后台接收、片段拼装、配对或产品协议。
详见 [BLE crate](../crates/moteframe-ble/README.md)。

## 固定 Cortex-M0 消费者

工具链固定，opt-level=s、fat LTO、单 codegen unit、panic=abort，八种运行输入产生
可观察结果。owned/borrowed 存储结果相同，BLE 的容量／所有权变体结果也相同。

| 消费者 | 链接 Flash | 静态 data+bss | exercise 局部帧 | 对象布局 |
| --- | ---: | ---: | ---: | --- |
| owned 窗口 | 378 B | 0 B | 136 B | window 16 B |
| borrowed 窗口 | 378 B | 0 B | 128 B | window 12 B |
| owned notifier + 8 B 槽 | 956 B | 0 B | 88 B | notifier 16 B，slot 24 B |
| owned notifier + 32 B 槽 | 956 B | 0 B | 88 B | notifier 16 B，slot 48 B |
| borrowed notifier + 8 B 槽 | 956 B | 0 B | 96 B | 引用包装 4 B，slot 24 B |

对象不是静态 RAM，不能重复加到已包含它的局部帧。大槽对象多 24 B，但此消费者只读
有效前缀，优化后局部帧相同，不代表任意应用扩容免费。借用也不总节省栈。数字包含
模拟后端和消费逻辑，不是完整 BLE 栈／存储系统或最坏调用链／IRQ 栈成本。
make portable 生成 target/portable-modules-delivery/resources/report.json。

## 原成本基线与持续验证

原 core/I²C 冻结 JSON、历史源码摘要和成本值保持原字节。新增成员改变根 manifest／lock，
独立且固定摘要的 scripts/build-context-migrations.json 只放行逐项记录的原 reference hash
及精确前后 Cargo 图。编译器、profile、probe、linker、非网络配置仍须完全一致；任意其他
图变更和旧消费者资源正增量仍拒绝。迁移不是重新测量历史源码，也不是提高预算。
BASELINE_MANIFESTS 与当前六 crate 指纹分开，context(root) 使用该 root 的锁文件。
当前 dedicated/shared core 和 I²C legacy 的 Flash、静态 RAM、局部帧增量仍为 0。

[受维护的 RAM 示例](../examples/ui-low-memory/README.md) 每档分别运行 14 项测试和 ARM
构建，检查真实数组、逐像素画面、写入次数、错误及待处理请求。内置场景 Infallible，
可失败资源／scene 由现有 UI session 测试覆盖，不加入测试专用生产开关。跨帧使用持久
session，一次性函数也显式返回未来/失败请求。运行 make ui-low-memory 复验。

模块、包、供应链、宿主/ARM 和成本门禁纳入 make check build；提交前运行 make hooks。
这仍是离线软件证据；真实 Flash、BLE 互操作、射频／电气时序、功耗和完整固件内存由设备工程验证。
