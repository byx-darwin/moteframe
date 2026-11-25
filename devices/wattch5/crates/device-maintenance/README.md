# 可选研发维护引擎

本 crate 为独立 workspace，零依赖、`no_std`、无堆、禁止 unsafe。默认 feature 空；商业产品应只使用默认构建，模块70–73不进入商业路由、能力目录或传递依赖。显式开启 raw-data/self-test 后，构造器仍先拒绝 `Profile::Commercial`。这不替代产品构建器对商业 feature 的硬排除。

| feature | 本次实现 | 边界 |
|---|---|---|
| raw-data | 模块70采样会话引擎 | 实际 port、共享 DATA lane、可信授权均必须注入 |
| self-test | 模块71判据/证据作业引擎 | 必须有真实 test port 和版本化产品判据；无内置产品通过阈值 |
| engineering | 72命名持久作业准入/查询/输出接口 | 真实执行器、产品参数/输出schema、原子持久后端与恢复均须注入 |
| factory-config | 73命名持久作业准入/查询/输出接口 | 未提供真实身份写入、校准、修复或Flash恢复实现 |

70状态取V0.6提案：1运行、2停止排空、3已停止、4失败。71保持冻结0..7。提案是 `new-design-choice`，不默改wire1.2冻结消息。没有新增核心status数字；本地 `Error` 由有权限的外层路由映射到既定状态码。当前wattch5候选仅core0，此crate未接入它，不代表模块已启用或真实外设已经可用。

## 原始采样端口

`RawEngine::new(profile, Some(&mut actual_port), &mut shared_lane, &authority, data_max)` 接受固定 `N` 字节批缓冲（1..466），无port、无scope、商业profile或DATA lane已占用均拒绝。实现 `RawPort` 的适配器必须提供真实静态source、channel格式/单位/比例、支持配置、实际接受频率/固定周期，以及实际采样起点与样本。不存在默认成功或示例采样后端。

- `list` 按包含式索引分页；`channel` 检查真实通道和格式；`configure` 只允许停止态，保留原14B合同语义并绑定非零修订。正式算法冲突由实际port返回Busy。
- `start` 必须先从外部 `DataLane` 获得真实排他租约，再启动port。`DataLane` 必须与历史服务共用同一连接owner，管理不复用的stream与DATA message ID。本crate不提供内部连接分配器。capture_id为该唯一stream ID扩展到u64，仍满足连接内唯一。
- `poll` 由port提供完整实际批、有效性和硬件丢样数；必须遵守buffer、样本上限和剩余u32 produced预算。无数据返回None；无效数据只计dropped，不导出成功样本。适配器必须在硬件/ISR层同样遵守剩余预算，违约不具备可靠最终计数证明。
- 一个pending批、一个已经开始的固定批和一个输入scratch构成有界缓存。新有效批替换最旧未开始批并计dropped；开始批的bytes/时间/序号不变。始终保持 `produced = emitted + dropped + queued`，queued含已开始但末片尚未接纳的样本。
- `grant` 接受累计whole-message额度；相同值幂等、递减冲突。`begin_batch` 在完整事件首片前预留一次额度与共享message ID。`current_batch` 用于FIFO重试，不二次消费额度。外层必须按冻结BATCH布局及CRC完整编码，再进行分片。
- **只有末片已按序接纳到同一DATA通知队列后才调用 `finish_batch`。** 此时才增加emitted并推进屏障。它不证明手机已收到、CRC正确或持久保存。
- `stop` 真实停止采样、丢弃pending并计dropped；有已开始批则返回None并保持Stopping，无需新GRANT继续该批。末片接纳后释放租约，返回严格最终计数和last_data_message_id；重复STOP返回同一结果。
- 每次服务操作前以及排空等待期间，调用 `advance(trusted_mono_ms)`。STOP后10秒仍未完成则清会话并报Timeout，不假称排空。同步引擎本身不拥有timer。时钟回退也结束引擎。

授权撤销或显式 `disconnect` 停止port、释放租约并清批缓冲；cleanup失败返回明确错误。不要只drop正在运行的引擎，必须先调用并检查disconnect。真实连接级owner还必须在通知关闭、断链、分片超时及发送失败时执行相同结束流程；此crate不接管BLE队列。若外部owner因inflight拒绝release，必须结束整个连接owner，不能重开该连接或假称排空完成。`finish_batch` 必须成功调用外部 `finish_message(stream, message)` 才能清除inflight并开始下一批。

## 自检端口与受控判据

`SelfTestEngine::new(profile, Some(&mut actual_port), &specs, &authority)` 要求按ID排序的非空 `TestSpec` 清单。每项绑定test_id、criterion_rev、method、允许timeout范围、危险scope、最多8个具体metric阈值及外部证据种类/origin。这里没有从旧C的 `*_ok` 标志推导阈值或PASS。

`run(test, options=0, timeout_ms, trusted_mono_ms)` 只表示接纳非零递增job。`poll(now)` 用实际port观察推进；自动测试必须返回全部且唯一的清单指标，单位/scale必须一致，由引擎按明确阈值判断。空指标、未知/重复指标或超8项都不能自动PASS。混合测试自动项成功后仍等待人工；人工setup完成只进入WAIT_MANUAL。

`ExternalRequest` 中的pass/fail、station和evidence是待验证字段，不能产生身份。`EvidenceVerifier` 必须独立解析实际证据，验证job/test/criterion版本/当前认证工站/证据类型，并返回可信origin与verdict。只有当前WAIT_MANUAL、manifest允许且独立proof逐项一致时可settle。接口没有让手机提交origin的字段，也没有默认成功verifier。相同终态内容的重试幂等；不同内容冲突；自动、取消、超时作业不能被外部PASS覆盖。

所有终态先确认实际backend cleanup。取消、超时与结果幂等；未知/淘汰job为NotFound，最多保留最近8个终态及一个活动job。job号不回绕复用。超时在 `poll/result/cancel/external_result` 的可信now上检查；持续调用poll仍是集成方责任。授权撤销终止工作并使结果缓存不可访问，恢复授权不能复活旧会话。Debug省略样本、metric值和evidence bytes。

## 72/73命名持久作业边界

`PersistentEngine::new(profile, Some(&mut actual_backend), &product_schema, &authority)` 要求当前可信工站身份、实际原子claim、持久身份/终态、已完成真实当前启动恢复，以及至少32个未确认终态与去重墓碑的保留保证。缺少任一保证、实际执行器或该操作恢复策略时返回Unavailable。Commercial始终拒绝。没有默认内存后端、物理执行器或允许所有参数的schema。

`submit(NamedOperation)` 按V0.6提案检查72.0001..000C、73.0001..0005的固定schema1形状、非零16B operation_id、参数长度/结构与强制危险scope，再由真实产品schema验证选择器、范围和判据。73序列号只接收1..32字节ASCII十进制文本，不截断，不接受NUL。提案中命名操作、参数布局、独立作业状态和此收紧均标记 `new-design-choice`，不代表旧C已经实现。

实际 `StorageBackend::claim_atomic` 必须在同一个串行原子提交边界内再次验证当前授权和产品schema，比较operation_id、module、operation及全部参数字节，持久分配job后才返回Accepted。不能仅比较有碰撞风险的摘要。相同身份/参数返回原job，改变绑定返回Conflict；终态不重跑。满存储拒绝新身份，不能淘汰未确认去重记录。手机的ID只用于幂等，不能授予身份或声称成功。

`submit` 不执行物理操作；真实外部执行器只能在成功响应被接纳后启动。其执行、持久终态更新、工站确认清理及Flash实现不在本crate内。真实重启必须把旧未决作业持久标记Interrupted，不能自动重放或PASS；普通/工厂复位都保留独立台账。这里的 `Guarantee::Implemented` 是可信集成方的实现断言，本库无法替真实Flash证明掉电原子性。

`query` 只读取实际记录；作业状态为1Accepted、2Running、3Succeeded、4Failed、5Interrupted，与71的0..7分开。非终态result=0只是保留值；Succeeded要求实际OK，失败/中断必须携带既定非零错误。`output` 仅在终态读取真实持久输出，并用原policy revision的产品输出schema再验证；未定义输出schema默认Unavailable，失败时清空调用者缓冲。失败/中断的诊断输出不会改变失败状态。授权撤销永久关闭此engine，恢复权限不能复活它。

## 验证范围

测试后端只在tests内提供合成夹具，用于离线状态机验收。持久作业测试中的Memory和模拟reboot只检查接口与状态行为，不证明真实持久恢复。host测试、feature矩阵、strict clippy、格式和Cortex-M0编译不代表真实采样频率、固件体积、队列吞吐、传感器测量有效性、人工/仪器证据真实性或硬件自检通过。真实backend及共享owner适配、冻结wire编解码/授权路由、72/73执行/持久化/恢复仍须单独实现与验收。
