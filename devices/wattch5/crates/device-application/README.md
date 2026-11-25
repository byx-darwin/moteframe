# 可选应用协议装配

本层只把通用协议服务、typed schema和真实后端端口接起来，不包含产品UI、传感器算法、Flash地址、模拟生产后端或硬件访问。默认feature为空；无堆、no_std、禁止unsafe。

- `SchemaProperties`：对真实存储读回做正文校验；写入先做完整字段校验，再把实时授权传给原子CAS后端。格式正确不等于已经持久化，持久承诺仍由实际后端提供。错误读回清空输出。
- `SchemaObjects`：完整对象提交前校验module/kind/产品字典。旧schema失败不会透传给其他provider。
- `ContentSchemas`：接通10条外部属性和6条对象的具体codec。独立通话蓝牙状态只读；钱包未知provider读回保留、新写拒绝。实际F000的字符串/对象容量与content manifest取较严格值；集合对象数量不能误当对象正文的数组元素数量。
- `ServiceCell`/`ServiceHandle`：多个路由/资源桥短借同一个actor，转发所有生命周期hook。checked RefCell不panic，重入显式失败；不适用于未同步的跨中断访问。
- `HistoryPort`：核心GRANT/DATASET和历史命令/DATA共享同一HistoryHandler、真实身份和DATA owner。权限与流不存在的状态分开判断，不能把ACCESS状态当资源权限。
- `ResourceRouter`：累计credit分给当前真实history/raw producer，数据集身份由实际持久存储读取；没有临时UUID或零身份成功兜底。
- `SystemHandler`：02.C30/C31仅在完整成功回复实际入发送队列后调真实延迟执行端口；排队重试不重复执行。02.C32使用可信默认名称并在原子持久提交后成功，E90必须来自该真实版本。未准入动作在断开时丢弃，已接受的计时动作执行时仍需复核权限。
- `ControlEvent`：冻结00.E80、通用E90/E91精确正文；没有CRC或RESP公共前缀。

`device_application::actions` 提供命令正文的编解码。`system` 启用重启、关机和恢复名称。查找、手机心跳、通话、媒体和通知动作分别由 `action-find`、`action-phone`、`action-call`、`action-media` 和 `action-notification` 启用。这些开关独立于 `find`、`phone`、`media` 等内容服务开关。

`COMPILED_MODULES`只列被选源码合同和稳定编号，不是运行时可用性目录。新的业务模块初次实现版本1.0；00/10/70为1.2，71为1.1；值/记录schema版本另外解释。真实注册必须来自实际绑定后端与容量，不能仅因存在codec就advertise可用。

产品组合参考`config/products/wattch5-build.json`。心率、血压、血氧、血糖、温度和Wellbeing策略已拆为独立configuration leaf feature；`health-policy`仅为开发时全组选项，wattch5使用实际选定的heart-rate/temperature。商用固件顶层负责禁止70–73及其别名，不使用全feature打商用包。

Host测试和ARM检查只验证软件合同；所有真实Flash、BLE Host/Controller/GATT、安全身份、采集器、持久历史/对象及电源端口仍须后续产品硬件接入与验收。


`service-routing`启用各类处理器的依赖；对应模块leaf feature才开启该处理器内的具体模块。`objects`选通用对象服务，`device-jobs`仅选独立商用持久作业，不带入研发维护引擎。`handlers`提供类型化再导出，不能替代运行时后端注册。

`CommonEvents`接受真实提交/观察端口，生成能力、属性、对象和可选电量事件；每片重新核对真实事件generation和scope，整条事件入队后才确认。`EventPair`静态组合多个source，保留具体子source的token，在整条事件准入后轮转优先级；若候选源准备阶段明确NotReady且尚未启动消息，下一次尝试另一源；关闭会尝试全部清理，并允许只重试失败的清理。不创建第二份DATA通道或虚构revision。所有事件通过同一个物理连接的EventPump发送。


R2 content candidate addition: ContentSchemas delegates `30.P0004/schema1` to the actual notification feature codec, separately from the original content route catalogue. The property uses exact source-app keys, independent boolean switches and sorted unique UTF-8 entries. Its descriptor maximum is actual `Manifest.property_max` in1..512 and validation also applies actual F000 string limits. Object storage may be absent or zero; F000 max_object_bytes/max_objects do not constrain this property. Session and PropertyService separately enforce actual negotiated carrier/container capacity. The generic property service still owns authorization, revision and durable storage; a compiled schema alone does not advertise an operational notification backend.
