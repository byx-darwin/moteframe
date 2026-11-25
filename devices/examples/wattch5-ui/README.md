# wattch 5 reference UI

产品示例，复刻原版外观和交互；通用屏幕、绘图、刷新接口仍在 `crates/device-ui`。
本批包含 style2 数字表盘、功能菜单、设置列表及亮度页。时间由调用方注入，
亮度只保存到示例内存，其他目的地仅发出 `Action::Open`，不执行产品功能。

- `src/`：安全 `no_std` 无堆的素材验证、页面状态、绘制代码，不依赖 SYD/BLE。
- `examples/preview.rs`：主机预览，页面状态由同一个 Rust Model 处理。
- `tools/`：从只读原 C/BMP 生成忽略目录下的素材包及独立参考图。
- `firmware/ble-host-probe` 的 `wattch-ui` feature：独立板级验证入口。
  触摸正常寄存器读取、按键、背光在适配器里，禁止调用原厂触控升级入口。

在仓库根目录运行：

```sh
python3 -m pip install -r examples/wattch5-ui/tools/requirements.txt
python3 examples/wattch5-ui/tools/prepare.py --source ../V2.2.4 --output target/wattch5-ui/assets
cargo run -p wattch5-ui-demo --example preview --locked -- target/wattch5-ui/assets/bundle.bin target/wattch5-ui/assets
python3 examples/wattch5-ui/tools/prepare.py --output target/wattch5-ui/assets --verify
cargo run -p wattch5-ui-demo --example preview --locked -- target/wattch5-ui/assets/bundle.bin --serve
```

预览地址 `http://127.0.0.1:25331`。上滑两次进入第二页功能菜单，点击设置，
点击设置第一页第三行进入亮度，上下滑切换，确认或右滑返回。侧键按钮返回表盘。
原素材语言帧0中亮度标签为英文 Dimming，保留源图，不自行翻译或替换。
浏览器重放/逐像素一致不能代替实机触摸及照片验收。

`make wattch-ui-check` 使用合成素材，包含主机状态、边界、HTTP、工具、
按键和 ARM 适配检查，无需原厂素材。原图、打包素材和设备镜像均不纳入提交。
