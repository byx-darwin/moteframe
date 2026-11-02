# moteframe-input

独立、无运行时依赖的 `no_std` 输入基础组件；无堆、禁止 unsafe。

- `EventQueue<T,N>`：实际 N 个 FIFO 槽，支持 N=0；满时拒绝，不覆盖旧事件。
- `Button`：调用方指定去抖和可选长按时间，输出 Pressed/Released/Held。
- `Touch`：调用方指定逻辑尺寸、tap slop、swipe 距离和 hold 时间；支持单触点
  Down/Move/Up/Cancel、点击、四方向滑动、一次长按和取消。
- 所有 now_us 都是同一启动代次内单调微秒；回退报错，新实例表示新代次。
- 队满、非法坐标／顺序或时钟回退均保留状态与已有队列；保留失败样本，先重试再推进输入流。

```rust
use moteframe_input::{Button, ButtonConfig, ButtonEvent, EventQueue};
let mut button = Button::new(ButtonConfig::new(30_000, Some(800_000)).unwrap());
let mut events = EventQueue::<ButtonEvent, 4>::new();
button.sample(true, 0, &mut events).unwrap();
button.sample(true, 30_000, &mut events).unwrap();
assert_eq!(events.pop(), Some(ButtonEvent::Pressed));
```

上面的阈值只是调用方示例，不是库的产品默认值。硬件读取、按键有效电平、触摸芯片
手势码、旋转／镜像／坐标缩放、ISR 同步、页面跳转都在外部适配器。
触摸需要真实 Down/Move/Up 样本，不能由缺失原始事件的厂商手势码凭空补出。
Up 每次最多产生一个手势；已经产生 Held 的接触不会再产生 Tap/Swipe。
按键需要在按住期间继续调用 sample 驱动长按；同一次调用若确认释放，释放优先于
刚到期的长按，不会在释放后补发 Held。库没有后台定时器。
移动超出 slop 后再回到起点不会触发点击／长按；滑动以释放时主轴位移判定。
多点触摸、自动丢弃策略、业务动作和后台定时器不在本版范围。

Apache-2.0；分发时保留 LICENSE 和 NOTICE。
