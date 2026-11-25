# wattch 5 Retro Digital

独立的安全 `no_std`、无堆表现层示例。完整实现设计包中的15个页面：
表盘、工具、快捷设置、设置、显示、亮度、表盘选择、配色、时间制式、关于、
倒计时快照、秒表快照、系统请求、确认、请求反馈。

- `src/types.rs`、`model.rs`：输入、快照、演示外观、固定六层导航和请求。
- `src/font.rs`、`render.rs`：47个5×7字形、四索引颜色、整数矩形与七段数字。
- `examples/preview.rs`：真实Rust模型/渲染器的主机预览。
- `tools/reference.mjs`：从批准的JS设计生成独立像素参考，供Rust逐像素核对。
- `firmware/ble-host-probe` 的 `retro-ui`：单独的SYD实验适配入口。

库不引用SYD寄存器、BLE、RTC或外部素材包。核心图形源数据为438字节，
不含代码、字符串、对齐；固件没有整屏位图，也不分配整屏缓冲。

`Model::handle(Event)` 最多返回一个 `Effect::Request`；计时、屏幕休眠、重启、
关机、重置均只发请求，反馈显示 `NO ACTION RUN`。外观确认只更新RAM示例状态，
不写设置Flash。`DisplayData` 由调用方注入；UI不自行推进时间。
背光预览由板级实验适配器处理。

在仓库根目录运行：

```sh
cargo run -p wattch5-retro-ui-demo --example preview --locked
```

打开 `http://127.0.0.1:25339/`。鼠标拖动/点击或方向键模拟输入，Enter选择、
空格侧键、Esc右滑。演示定位与数据快照控件不属于手表产品界面。
主机预览保留最近12条请求。原厂皮肤与25331入口继续保留。

```sh
make retro-ui-check
```

验证561个设计状态、9191424个RGB565像素，覆盖15页、三表盘、三配色、
12/24小时、边界时间与计时数据状态。CI需要Node22生成设计参考；Rust运行时不依赖Node。
设计参考在 `designs/wattch5-retro-digital/`，不是实机画面证明。

设备构建方式及检测范围见 [设备工程说明](../../README.md)。
