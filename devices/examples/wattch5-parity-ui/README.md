# wattch 5 bilingual parity V2

安全`no_std`、无堆的独立产品皮肤。按原厂37个注册窗体、10类通知及15个子页/扩展实现62个场景；保留V1包和像素基线，不替换原厂C代码。

- `Model`：固定6层导航、原菜单分页、RAM外观及显式请求。`handle(event,&snapshot)`原子拒绝非法输入；`inject`整理快照变化对应的导航。
- `Snapshot`：调用方借用的消息、计次与二维码；数值、状态和时间来自调用方。六类测量独立，心率历史可注入；UI不产生真实健康数据。
- `NoticeSnapshot`：10类通知的有限触发类型，恢复来源页；原Alarm/Reset/Disconnect空处理仍明确标注。
- `Scene`：一次验证后可重复绘制到行目标，256字节RGB565行缓冲，无MCU整屏缓冲。准备场景会在模型副本中修正消息/计次索引，不修改调用方模型或数据。
- `UiRequest`：测量、计时、运动、消息、音乐、来电、拍照、清数据、重启等均为请求。演示动作没有设备成功确认；仅实验背光预览由板级执行。

默认简体中文，可切英文。其他7种原语言可选但只发`LanguageRequested`。242字中文12×12子集与47字英数字形，核心图形源数据6730B；中文授权/出处在[FONT-NOTICE](../../designs/wattch5-retro-parity/FONT-NOTICE.md)，完整随附许可在`licenses/`。不是任意CJK支持：未知字符返回`UnsupportedGlyph`，超过当前布局的消息返回`TextOverflow`，均在场景写屏前拒绝。消息最大20条、每种语言每条最多120 UTF-8字节；单场景布局可能更严格，调用方需分页/重新供应合适内容。

```sh
cargo run -p wattch5-parity-ui-demo --example preview --release --locked
make parity-ui-check
```

实际Rust预览：`http://127.0.0.1:25340/`。浏览器只转换输入和显示Rust位图，可注入明确的演示快照；不连接真实服务。原厂25331、V1设计25337、V2设计25338、V1 Rust25339保留。

`tools/generate.mjs --check`核对62场景、固定双语文案及字库；`tools/reference.mjs`捕获独立设计参考，Rust通过公开事件重建并逐行导出1757个状态。另有动态文字、不可用数据、导航循环和原子性回归，不能把设计JS的通过结论当Rust结果。

板级候选：`firmware/ble-host-probe --features parity-ui`，沿用原正常触摸/侧键读取和BLE协同泵，未更改中断/射频实现。当前只编译链接、静态审计，V2未刷写设备，未做V2实机触摸/蓝牙验证。

设备构建方式及检测范围见 [设备工程说明](../../README.md)。
