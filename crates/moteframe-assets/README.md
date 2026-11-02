# moteframe-assets

`no_std`、无堆、禁止 unsafe 的素材读取基础组件。依赖现有 `ReadAt` 和
`embedded-graphics-core`，可直接使用 `moteframe-storage::ReadWindow`。

- `Raster<F>` 仅保存几何和字节范围，不持有 reader、缓存或整幅图片。
- 原生输出 RGB565 BE/LE、RGB888、Gray8 和 0/1 单字节单色；支持自定义 `PixelFormat`。
- `Glyph` 组合 raster 与 bearing/advance；字符映射、字体文件和内容由调用方提供。
- `read_span` 使用调用方的像素输出和字节 scratch，按整像素分块；无固定 256 B 临时数组。
- 完整范围在 I/O 前校验；读取／解码首错停止，无隐藏重试。错误时丢弃整个输出。

```rust
use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
use moteframe_assets::{Raster, Rgb565Be};
use moteframe_storage::SliceSource;
let mut source = SliceSource::new(&[0xf8, 0x00, 0x00, 0x1f]).unwrap();
let image = Raster::<Rgb565Be>::new(0, 2, 1).unwrap();
let mut pixels = [Rgb565::BLACK; 2];
image.read_span(&mut source, 0, 0, &mut pixels, &mut [0; 2]).unwrap();
assert_eq!(pixels, [Rgb565::RED, Rgb565::BLUE]);
```

这是存储素材编码，与屏幕线上的 RGB565 编码职责不同。未提供压缩、位打包字形、
字库、目录格式、资源更新或厂商原图。继续使用 UI 的 ResourceKey/Metadata/View，
将不可变 descriptor 和可变 reader 分开传入 scene；tests/ui.rs 展示真实组合和失败重绘。
几何／偏移限定为 u16／u32；不支持大于该地址空间的数据源。

Apache-2.0；分发时保留 LICENSE 和 NOTICE。
