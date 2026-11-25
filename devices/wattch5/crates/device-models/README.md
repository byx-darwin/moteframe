# device-models

Optional, no_std application value codecs with zero allocation. Default features are empty:

| Feature | Encoded values |
| --- | --- |
| profile | User profile, activity goals |
| device | Battery/charging value with validity mask |
| settings | Display units, hour format, local weekday/minute window |
| reminders | Six reminder object kinds; depends on settings for shared windows |
| notifications | Phone notification object schema2 |
| weather | Strict device-side category/code dictionary, unique ordered forecast dates |

These are business-body schemas, not concrete sensors, persistence, timers or displayed UI. Public construction is validated again on encode. Borrowed text requires UTF-8/no NUL and caller-provided product byte limits; Debug omits contents. Weather read fallback in a future App must preserve unknown future codes; the device COMMIT decoder intentionally rejects unsupported writes.

The V0.6 candidate definitions are in specs/protocol/v06-contract-proposal.json. Reminder codecs validate the configuration only. Executing reminders still requires a real UTC/RTC policy, timezone and alert backend; suppressed-event/DST behavior has not been implemented by this crate.

```sh
cargo test -p device-models --all-features --locked
cargo clippy -p device-models --all-features --all-targets --locked -- -D warnings
cargo check -p device-models --no-default-features --features weather --target thumbv6m-none-eabi --locked
```
