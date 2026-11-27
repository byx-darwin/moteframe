# 多厂商硬件适配架构

MoteFrame 的目标是让同一套采集、配置保存和 BLE 应用可以适配不同厂商的主控。产品固件选择运行方式和功能，开发板描述实际接线，芯片后端提供硬件操作。下图表示目标装配关系，当前 FR8029D 后端仍在固件工程中。

```mermaid
flowchart TD
    App["产品固件：采集、配置、BLE、升级、可选 UI"]
    API["通用框架 crates/：I²C、存储、输入、显示等"]
    RT["裸机事件循环 / FreeRTOS 任务"]
    Layout["产品分区与镜像预算"]
    Board["具体开发板：器件、供电、接线和引脚复用"]
    FR["Freqchip 具体料号与封装"]
    Other["其他厂商的具体芯片"]
    App --> API
    App --> RT
    App --> Layout
    App --> Board
    Board -->|FR8029D EVB| FR
    Board -->|其他厂商开发板| Other
    FR --> FRDrv["已核对可共用的系列驱动及型号专用后端"]
    Other --> Drv["对应厂商的外设、启动和无线后端"]
```

| 职责 | 目录 | 换硬件时需要核对 |
|---|---|---|
| 通用接口与算法 | `crates/` | I²C 事务、配置双槽、输入和显示等接口能否由新后端实现 |
| 设备间复用组件 | `devices/crates/` | 选择可复用的 Host 或控制器；提供 HCI 的芯片可对接 Host，直接驱动射频的芯片需适配控制器后端 |
| 芯片支持 | `devices/chips/<厂商>/` | 具体料号、封装、外设实例、中断、ROM、射频校准和 Flash 控制 |
| 开发板 | `devices/boards/<板型>/` | 实装器件、引脚、供电、跳线和互斥资源；同芯片的不同 PCB 可分别登记 |
| 产品固件 | `devices/firmware/<固件>/` | 组合板级驱动、运行方式和服务；按 Flash 容量、擦除粒度及 ROM 约束核对应用、配置、升级暂存和搬运范围 |

裸机和 FreeRTOS 可共用板级驱动及 GATT 服务，各自接入唤醒与超时等待。FR8029D 的 `0x40000`、`0x41000` 配置槽属于当前 1 MiB 产品布局。现有实现见 [FR8029D 固件和 BLE 架构](../../devices/firmware/fr8029d-bringup/README.md#fr8029d-的-ble-架构)。

## 首个参考实现

移植先查[富芮坤型号能力表](../../devices/chips/freqchip/README.md)，再查[FR8029D 板载功能、实测状态和验收计划](../../devices/boards/fr8029d-evb/README.md)。接入其他厂商时，在对应厂商目录登记料号能力，按实际板型登记资源与验证结果，再由产品固件选用。
