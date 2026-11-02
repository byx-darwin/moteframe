# UI 配置 r0.2 实现计划

依据：docs/ui-configuration.md r0.2，用户已批准开发。只改通用框架，不接硬件、不加业务、不提交或上传。

## Task 1: 配置和资源
- 测试先行：私有设置、验证资源视图、稳定上下文、原子更新和布局缺省值。
- 实现 Palette、ResourceKey/ResourceView、UiSettings/UiState、UpdateImpact。
- 验证：配置测试通过，既有默认绘图测试保持通过。

## Task 2: 刷新会话
- 测试先行：工作区容量、绑定/代次、错误保留、帧后失效合并和重绑定。
- 实现 RefreshSettings、RefreshSession、DisplayBinding、DeferredInvalidation。
- 验证：故障注入、跨分块画面一致、同步写屏后 service。

## Task 3: 裁剪和调度
- 测试先行：无默认功能消费者、RUN 容量、时间回退/失败/启停序列。
- 接入 Cargo features，保留默认低层接口，添加可选帧间隔。
- 验证：所有 feature 子集、真实消费者合并、所有权/颜色/私有字段编译失败。

## Task 4: 内存和交付
- 实测各组合结构尺寸和 ARM 链接段；区分工作区、候选值和栈帧限制。
- 更新文档、白名单、回归入口；运行 make check/build/hooks/supply-chain/secrets。
- 新上下文独立审查整轮实现，修复重要问题并复验。
- 输出本地实现及验证报告；硬件行为、整机峰值栈均不冒充已验证。
