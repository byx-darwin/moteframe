# 通用对象服务

独立workspace；`no_std`、无堆、禁止unsafe，依赖现有device-protocol/device-storage/device-services。仅实现冻结V0.5通用C20–C28的同步业务服务，没有真实Flash、硬件操作、产品模块注册或默认成功后端。

```rust,ignore
let transactions = ObjectTransactions::<4096>::new(); // 每个新物理连接只创建一次
let handler = ObjectHandler::new(
    actual_module_entry, &mut actual_repository, &actual_complete_schema,
    &trusted_access, actual_supported_kinds, &transactions,
)?;
```

同一连接所有对象模块必须共用同一个`ObjectTransactions<N>`及相同的实时授权视图；每模块各造owner会违反合同，集成方不得这样注册。共享owner持有唯一ObjectEngine、全连接递增事务号和关闭标志；另一个模块BEGIN返回Busy，不能用另一模块事务号WRITE/COMMIT/ABORT。关闭后不能重开或为同连接替换owner。`RefCell`仅支持同步串行调用，后端/schema回调不得重入同一owner。构造器要求真实可用ModuleEntry、128..512控制容量、1..4096暂存容量以及排序唯一非零kind清单。Session接口要求整个ModuleEntry逐字段匹配，包含版本、容量和feature，不靠模块号假称兼容。

`ObjectAccess::allowed(module, operation, None)`为模块级检查，`Some(id)`为具体ID范围。READ/INFO/ID_STATE用Read，BEGIN/WRITE/COMMIT/ABORT/单条DELETE用Write，ENUM用Enumerate，FFFFFFFF清空用Clear，且额外调用`dangerous_allowed`验证真实D与产品策略。这些字段不由请求授予；构建profile和HELLO均不能替代授权。`BoundAuthorization`把BEGIN的module与具体ID绑定给ObjectEngine，在实际reserve/commit持久原子边界重新检查。

| 命令 | 请求/成功响应（均不含10B帧头，响应包含4B状态） |
|---|---|
|20 BEGIN|31B；返回10B：status、tx:u32、chunk_max:u16|
|21 WRITE|tx:u32、offset:u32、length:u16、bytes；返回8B：status、accepted_offset:u32|
|22 COMMIT|8B；返回12B：status、object_id:u32、revision:u32|
|23 READ|14B；返回18+N：status、revision:u32、total:u32、offset:u32、length:u16、bytes|
|24 DELETE|8B；仅4B状态；FFFFFFFF使用集合revision CAS|
|25 ENUM|start:u32、max:u8；返回13+10×count，每项id:u32、kind:u16、total:u32|
|26 ABORT|4B tx；仅4B状态|
|27 INFO|4B ID；返回18B：status、kind:u16、revision:u32、total:u32、CRC:u32|
|28 ID_STATE|4B合法ID；返回39B：status、state:u8、high_water:u32、birth_kind:u16、birth_total:u32、birth_crc:u32、birth_revision:u32、birth_token:16B|

READ一次最多返回请求max和Session实际协商输出容量允许的片段；BEGIN声明的chunk_max也受实际控制容量约束并绑定本次事务，不能因模块目录声明512就忽略HELLO协商128。后端必须从同一个expected_revision快照返回完整该范围，不能混版本或把不完整属性冒充完整值。目录同样按实际容量分页，从0 inclusive，稀疏ID不重排，next必须是首个未返回真实ID或FFFFFFFF；每个返回项及next本身都检查具体资源权限。空页只允许EOF，排序/重复/非法kind/畸形元数据会返回InternalFailure，不向外传递内部数据。初次枚举缺少某项scope返回ACCESS_DENIED，不声称已经发生过授权撤销；已缓存页的原项/next权限撤销则终止连接。后端仍须保证不遗漏真实项，适配层不能凭一页独立证明数据库的完整性。

创建必须先原子持久推进high_water并保留唯一token；跳过的ID、ABORT、删除和清空都不能回收或降低水位。`ObjectRepository::id_state`必须直接读取真实持久记录；state0未消耗、state1存在、state2已消耗无对象。非state1的全部birth字段必须为0，适配器拒绝带私有残留的后端答复。state1使用首次提交的kind/total/CRC/revision1/token，不能从当前编辑后内容重建。当前INFO与不可变birth明确分开。

ObjectStore的reserve/commit和ObjectRepository的delete_atomic必须在真正串行持久提交边界复核当前具体ID授权及CAS；创建/编辑/删除同时维护非零、不回绕的对象与集合revision。ObjectSchema在完整长度和WOBJ CRC通过后验证完整产品正文；静态kind清单不能代替它。没有可靠持久身份台账、schema或原子提交实现，就不能注册此服务。真实Flash掉电原子性是后端验收义务，本crate没有生产memory实现。

成功容量在任何写副作用前检查。COMMIT失败（含格式、CRC、schema、CAS或未知后端结果）结束所属模块暂存，旧对象不应被覆盖；未知实际提交结果必须用原ID的ID_STATE/INFO恢复，不能自动换ID新建。持久预留不会撤销。WRITE错误不会伪造接纳偏移。业务错误只输出精确4B：参数1、状态4、权限5/FF03、D缺失5/FF04、Busy6、容量7、NotFound8、StorageFailure9、Conflict11、InternalFailure17、CRC错误18/FF01；存储内容完整性错误18/FF02。Incomplete/顺序/schema错误按参数1处理；后端应使用Schema报告正文问题、Integrity报告完整性问题，不能返回猜测的成功。

授权在执行后和发送/缓存释放前重查。撤权清暂存和全部待返回输出，永久关闭共享owner；实现`ServiceHandler::must_end_session`，所以已有暂存Write权限撤销后连核心PING也不能继续旧Session。已缓存COMMIT的具体ID、ENUM原返回项/next和成功CLEAR的D权限都会重查。初始无权限仍返回正常0005，不谎称发生过已授权执行。Debug只暴露容量/关闭状态等元信息，省略正文、token、CRC和创建证据。

服务事务是同步完成的。直接实现ServiceHandler生命周期：`response_ready`使用同步true默认值，`response_admitted`对已经完成的同步操作使用Ok(())默认值，不执行第二次提交；`connection_closed`显式调用disconnect并永久关闭共享owner，重复调用幂等。此noop只表示没有延迟作业待启动，不代表后端成功。无需依赖device-connection或另建包装。实际队列接纳仍须在同一授权边界复核；Session负责唯一最后请求/响应缓存和请求号规则，此crate不另存响应或执行重放。

测试仅使用tests内非持久Memory夹具核验接口、固定字节、授权、ID消耗、创建证据和CAS；模拟状态与ARM编译不能证明实际Flash、复位恢复、固件集成或硬件可用性。当前wattch5 core0候选未因本crate新增任何业务能力。
