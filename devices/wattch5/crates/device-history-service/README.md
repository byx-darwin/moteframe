# 历史服务与DATA调度

独立workspace，`no_std`、无堆、禁止unsafe。只有device-protocol/device-storage/device-services路径依赖。实现模块10的C01..08真实服务分派、RECORD数据调度、DONE以及取消排空。没有真实Flash/传感器/手机数据库实现，没有自动注册模块10；当前wattch5 core0候选不因本crate获得历史能力。

```rust,ignore
let lane = SharedDataLane::new(actual_unique_connection_identity);
let mut history = HistoryHandler::<_, 8, 90>::new(
    actual_module_entry, &mut actual_record_store, &trusted_history_access,
    &lane, actual_negotiated_data_max, actual_ordered_data_queue_guarantee,
)?;
```

`D`为有界目录容量1..255，`P`为有界完整记录正文缓冲1..474。data_max要求实际协商的128..512，包含RECORD固定38B及全部正文；例如128时正文最多90B。不能截断较大记录冒充完整记录。若无法保证同一DATA FIFO的整条逻辑消息顺序准入，构造失败。所有历史/原始采集adapter必须借同一个`&SharedDataLane`；此crate没有DATA/stream分配器。每连接只注册一个模块10 handler，不得在同一连接重建actor/owner绕过关闭或重用ID。

ModuleEntry全字段必须与编译配置相同，包含版本、availability、feature和控制容量。ENUM按Session实际传入的控制容量分页，排序、inclusive起点、next首个未回domain与空页EOF均通过冻结codec。目录来自真实RecordStore的连接静态视图，dataset_id/epoch/目录变化会结束旧会话。这里没有BLE MAC导出的假dataset、示例历史或默认成功RecordStore。

## 后端与授权

`RecordStore`必须提供真实directory、open保留租约、plan实际ID、完整read、真实正文schema校验、record_admitted证明、原子ack、count、renew与close。全部方法没有默认成功实现。OPEN只有实际能够在整个租期保证固定上界和重读时才成功；没有容量保证返回Busy。后端实际原子性、掉电恢复、epoch/ID持久性和过滤正确性都须独立验收，不能由本状态机推定。

`HistoryAccess`来自可信身份、profile与dataset/domain scope。模块门禁先于报文解析；具体domain再检查Read/Enumerate/Acknowledge，dataset查询单独Identity权限。手机报文不能自授身份。OPEN把真实filter绑定进lease；ACK不能通过省略filter字段绕过。任何filter_flags位非零均拒绝ACK，返回0004/FF05，包含刚好选中所有记录的情况。未知或过期snapshot先返回CURSOR_EXPIRED；真实epoch冲突返回CONFLICT。

每次请求、DATA尝试和排空等待期间调用`advance(trusted_mono_ms)`。READ/ACK成功续租，COUNT不创建snapshot或ACK证明。ACK续租预检在不可逆持久ACK之前完成，防止续租失败时先推进水位再返回失败。OPEN异常回复或计时溢出会释放已得到的真实handle；清理失败保留handle并结束连接，使close hook仍能重试。已过期闲置lease可在新OPEN前真实释放，snapshot号仍单调不复用。

## 数据与确认

READ响应17B（含公共4B状态），只接纳实际plan的1..16或空EOF记录批次。只有`ServiceHandler::response_admitted`收到实际完整READ响应，并逐项核对原请求、snapshot、stream、cursor、eof、count之后，才开启数据。Caller不能用只有OK前缀的假响应放行DATA。

核心资源适配方法：

- `grant(DataKind::History, stream_id, cumulative)`接受累计whole-message额度；重复幂等、递减冲突、其他kind/stream拒绝。
- `dataset()`读取真实连接目录身份并再次检查Identity scope。
- `grant_authorized()`与`dataset_authorized()`只查同源真实权限，不能因为没有stream或额度下降把资源状态误报成权限拒绝。
- `validate_grant(kind, sid, cumulative)`为无副作用的上下文/额度检查；`grant_replay_valid(sid, accepted_cumulative)`只认当前仍持有的stream generation，已释放旧流一律false。DONE待准入时流仍当前；runtime必须先排完GRANT控制响应，再准入DONE并释放流。
- 若核心资源provider和业务route同时访问同一个actor，外层可用checked RefCell/短借route包装共享它，不得复制actor或另造lane。

`next_data(out)`读取完整真实记录，依次核对domain/epoch/ID、实际commit、WREC C_store及产品正文schema，然后构造绑定当前snapshot/stream的WAP1 RECORD并计算新的C_event。禁止直接复用存储CRC，坏存储记录不会被跳过、输出0测量或形成ACK证明。只有完整事件可放入输出时，才一次预留credit与外部DATA message ID。调用重试返回完全相同的固定bytes/MID；SharedDataLane暂时Busy不会消耗额度或制造DONE错误。

`finish_data(mid)`只允许在该消息真实末片已进入同一有序DATA队列后调用，成功同时通知共享owner与RecordStore的临时sent证据。错MID保留inflight。该回调不证明手机收到、CRC正确或持久保存，不会自动推进ACK。普通批次完成用`done(out)`取固定16B控制事件，再在真实DONE队列准入后调用`done_admitted(stream)`释放正常流；DONE不耗DATA credit/ID，不是保存证明。实际控制事件message ID、GATT分片和通知队列仍由连接runtime负责。

ACK由真实后端在同一个持久原子边界内复核scope、epoch、filter、当前水位、实际记录索引及本lease的完整准入证据。必须验证无遗漏真实前缀；数值跳号允许，遗漏实际记录不允许。不能只相信highest_sent或手机声称以前发过。新OPEN/新连接的证明为空，不能导入以前的发送主张。手机先校验C_event并提交数据库，再发ACK；本库无法验证手机文件系统。持久确认不等于立即物理擦除。

## 取消与关闭

`ServiceHandler::response_ready`处理CANCEL_READ/CLOSE：停止新逻辑消息，丢弃未开始计划，保留已经开始消息的固定bytes和已预留credit。只要有inflight就返回false；Session对相同REQ返回本地`SessionError::Deferred`，不编码wire Pending。实际末片准入并`finish_data`后才生成6B成功响应，barrier为本流真实最后完整准入MID；从未开始则0。重复pending检查不重置十秒期限。

CANCEL保留snapshot与持久ACK；CLOSE排空后真实释放lease。取消路径删除未准入DONE，不额外伪造DONE成功。共享owner的cancel_barrier确认后才release，后续流与DATA ID继续不复用。十秒未排空、时钟回退、身份/epoch变化或控制/DATA通知撤销均结束连接，不能重开旧actor。生命周期`connection_closed`清payload/event缓存、关闭实际共享lane并释放后端lease，清理失败显式返回错误。runtime必须保证真实通知关闭/发送失败也走此hook；只drop actor不构成清理。

## 规范裁决与验证边界

构造时拒绝零dataset，与核心DatasetIdentity规则一致；目录身份无法验证时结束旧会话，不能从缓存制造一个可继续使用的身份。`new-design-choice`明确两项冻结合同未完全规定的边界：实际后端暴露未commit记录时用STORAGE_FAILURE9/None，避免把CRC正确但未提交记录说成CRC mismatch；空批DONE没有“本批最后记录”，last_id取0，不把after游标伪装成已发送记录。坏C_store仍严格18/FF02；所有非OK业务响应只4B。没有新增wire状态/原因数字。

tests内Memory为非持久合成夹具，检验冻结3A161ABD例包、坏CRC、未commit、实际前缀缺口/数值跳号、全部过滤ACK禁令、累计credit、跨流ID耗尽、取消半消息/零屏障/十秒超时、epoch/租期、真实Session Deferred和回调绑定。夹具不证明真实Flash保留、过滤索引、原子ACK、手机持久化、BLE FIFO、吞吐或硬件测量。上述真实后端与runtime适配未接入前，不能宣称产品历史服务已可用。
