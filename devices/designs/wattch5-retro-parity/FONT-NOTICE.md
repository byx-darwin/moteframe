# 中文点阵子集来源与授权

本项目的派生子集名称为 **Wattch Retro CN Subset**。它不是上游完整字体，也不使用上游保留字体名命名派生字体。

源：TakWolf / Fusion Pixel Font，12px monospaced zh_hans，固定发布 **2026.10.25**。
[上游发布](https://github.com/TakWolf/fusion-pixel-font/releases/tag/2026.10.25)，[上游字体授权说明](https://github.com/TakWolf/fusion-pixel-font#许可证)。

Copyright (c) 2022, TakWolf (https://takwolf.com), with Reserved Font Name “Fusion Pixel”.
依SIL Open Font License 1.1授权。保留完整许可与上游附带声明在`licenses/`。
本子集更改了表示形式和名称，仅提取本次固定中文UI文案所需字符；不分发完整字体，不将该子集单独出售。

来源压缩包、原BDF的SHA-256及子集大小在`font-provenance.json`。
`prepare_sources.py`从固定发布压缩包中的BDF按字框/基线提取，生成`font-zh.js`，未用浏览器字体或系统闭源字体栅格化。

编码：12×12格，每行u16的低12位为像素，bit11最左，bit0最右；row0在上。Unicode索引为u16。
完整压缩包仅位于忽略的target目录；运行时仅需要子集行掩码、索引与许可声明。
授权文本与出处是开发/分发文档，无需作为屏内图片或运行时字形资产载入。
