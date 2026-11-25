//! Host-only command adapter. Snapshots and requests remain separate from services.
#[cfg(test)]
mod tests {
    use super::*;
    use wattch5_parity_ui_demo::UiRequest;
    #[test]
    fn test_should_commit_host_commands_atomically() {
        let mut s = State::default();
        let before = s;
        assert!(s.command("time 24 0").is_err());
        assert_eq!(s, before);
        assert!(s.command("timer RUNNING 360000").is_err());
        assert_eq!(s, before);
        s.command("view heart").unwrap_or_else(|e| panic!("{e}"));
        s.command("measure heart result 72 150 55")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.data.heart_high, Some(150));
        assert_eq!(s.data.measurements[0].reading, Reading::Number(72));
    }
    #[test]
    fn test_should_require_v2_origin() {
        let mut input=&b"POST /event HTTP/1.1\r\nOrigin: http://127.0.0.1:25339\r\nContent-Length: 2\r\n\r\nup"[..];
        assert!(crate::http::read(&mut input, "http://127.0.0.1:25340").is_err());
    }
    #[test]
    fn test_should_leave_measurements_idle_on_user_start() {
        let mut s = State::default();
        s.command("view heart").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            s.command("select"),
            Ok(Effect::Request(UiRequest::MeasureStart(MeasureKind::Heart)))
        );
        assert_eq!(s.data.measurements[0].state, MeasureState::Idle);
    }
}
use std::{
    error::Error,
    io::Write,
    net::{TcpListener, TcpStream},
    time::Duration,
};
use wattch5_parity_ui_demo::{
    Effect, Event, Locale, MeasureKind, MeasureState, Model, Reading, Screen, Snapshot, SportPhase,
    StopwatchState, Theme, TimerState, WeatherCondition, WeatherSnapshot,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    model: Model,
    data: Snapshot<'static>,
}
type HostError = Box<dyn Error>;
fn measurement(s: &str) -> Result<MeasureKind, HostError> {
    Ok(match s {
        "heart" => MeasureKind::Heart,
        "pressure" => MeasureKind::Pressure,
        "oxygen" => MeasureKind::Oxygen,
        "temperature" => MeasureKind::Temperature,
        "glucose" => MeasureKind::Glucose,
        "hba1c" => MeasureKind::Hba1c,
        _ => return Err("invalid measurement kind".into()),
    })
}
fn measure_state(s: &str) -> Result<MeasureState, HostError> {
    Ok(match s {
        "idle" => MeasureState::Idle,
        "pending" => MeasureState::Pending,
        "measuring" => MeasureState::Measuring,
        "result" => MeasureState::Result,
        "error" => MeasureState::Error,
        "unavailable" => MeasureState::Unavailable,
        _ => return Err("invalid measurement state".into()),
    })
}
fn reading(s: &str) -> Result<Reading, HostError> {
    if s == "--" || s == "--/--" {
        Ok(Reading::Missing)
    } else if let Some((a, b)) = s.split_once('/') {
        Ok(Reading::Pressure {
            upper: a.parse()?,
            lower: b.parse()?,
        })
    } else if let Some((a, b)) = s.split_once('.') {
        if b.len() != 1 {
            return Err("one decimal place required".into());
        }
        Ok(Reading::Tenth(
            a.parse::<u16>()?
                .checked_mul(10)
                .and_then(|n| n.checked_add(b.parse().ok()?))
                .ok_or("reading overflow")?,
        ))
    } else {
        Ok(Reading::Number(s.parse()?))
    }
}
fn optional(s: &str) -> Result<Option<u16>, HostError> {
    if s == "--" {
        Ok(None)
    } else {
        Ok(Some(s.parse()?))
    }
}
fn timer(s: &str) -> Result<TimerState, HostError> {
    Ok(match s {
        "IDLE" => TimerState::Idle,
        "RUNNING" => TimerState::Running,
        "PAUSED" => TimerState::Paused,
        "DONE" => TimerState::Done,
        "UNAVAILABLE" => TimerState::Unavailable,
        _ => return Err("invalid timer state".into()),
    })
}
fn stopwatch(s: &str) -> Result<StopwatchState, HostError> {
    Ok(match s {
        "IDLE" => StopwatchState::Idle,
        "RUNNING" => StopwatchState::Running,
        "PAUSED" => StopwatchState::Paused,
        "UNAVAILABLE" => StopwatchState::Unavailable,
        _ => return Err("invalid stopwatch state".into()),
    })
}
fn sport_phase(s: &str) -> Result<SportPhase, HostError> {
    Ok(match s {
        "COUNTDOWN" => SportPhase::Countdown,
        "RUNNING" => SportPhase::Running,
        "PAUSED" => SportPhase::Paused,
        "DONE" => SportPhase::Done,
        _ => return Err("invalid sport phase".into()),
    })
}
const LAPS: [u32; 20] = [
    100, 205, 310, 415, 520, 625, 730, 835, 940, 1045, 1150, 1255, 1360, 1465, 1570, 1675, 1780,
    1885, 1990, 2095,
];
impl State {
    fn command(&mut self, text: &str) -> Result<Effect, String> {
        let mut n = *self;
        let effect = n.apply(text).map_err(|e| e.to_string())?;
        n.model.inject(&n.data).map_err(|e| format!("{e:?}"))?;
        n.model.scene(&n.data).map_err(|e| format!("{e:?}"))?;
        *self = n;
        Ok(effect)
    }
    fn apply(&mut self, text: &str) -> Result<Effect, HostError> {
        let parts: Vec<_> = text.split_whitespace().collect();
        let p = parts.as_slice();
        match p {
            ["view", name] => self
                .model
                .preview(
                    Screen::from_id(name).map_err(|e| format!("{e:?}"))?,
                    &self.data,
                )
                .map_err(|e| format!("{e:?}"))?,
            ["locale", name] => {
                let locale = match *name {
                    "zh" => Locale::Zh,
                    "en" => Locale::En,
                    _ => return Err("invalid locale".into()),
                };
                self.model
                    .set_appearance(wattch5_parity_ui_demo::Appearance {
                        locale,
                        ..self.model.appearance()
                    })
                    .map_err(|e| format!("{e:?}"))?;
            }
            ["theme", name] => {
                let theme = match *name {
                    "0" => Theme::Phosphor,
                    "1" => Theme::Amber,
                    "2" => Theme::Ice,
                    _ => return Err("invalid theme".into()),
                };
                self.model
                    .set_appearance(wattch5_parity_ui_demo::Appearance {
                        theme,
                        ..self.model.appearance()
                    })
                    .map_err(|e| format!("{e:?}"))?;
            }
            ["time", h, m] => {
                self.data.hour = h.parse()?;
                self.data.minute = m.parse()?;
            }
            _ => return self.data_command(p),
        }
        Ok(Effect::None)
    }
    fn data_command(&mut self, p: &[&str]) -> Result<Effect, HostError> {
        match p {
            ["measure", kind, state, value, high, low] => {
                let kind = measurement(kind)?;
                self.data.measurements[kind as usize] =
                    wattch5_parity_ui_demo::MeasurementSnapshot {
                        state: measure_state(state)?,
                        reading: reading(value)?,
                    };
                self.data.heart_high = optional(high)?;
                self.data.heart_low = optional(low)?;
            }
            ["timer", state, seconds] => {
                self.data.timer.state = timer(state)?;
                self.data.timer.seconds = seconds.parse()?;
            }
            ["watch", state, centis, laps] => {
                self.data.stopwatch.state = stopwatch(state)?;
                self.data.stopwatch.centis = centis.parse()?;
                self.data.stopwatch.laps = match *laps {
                    "empty" => &[],
                    "demo" => &[1040, 1284],
                    "twenty" => &LAPS,
                    _ => return Err("invalid lap preset".into()),
                };
            }
            ["messages", mode] => {
                self.data.messages = match *mode {
                    "empty" => &[],
                    "demo" => Snapshot::default().messages,
                    _ => return Err("invalid messages preset".into()),
                }
            }
            ["weather", "none"] => self.data.weather = None,
            ["weather", low, high, condition] => {
                let conditions = [
                    WeatherCondition::Fine,
                    WeatherCondition::Cloudy,
                    WeatherCondition::Rain,
                    WeatherCondition::Snow,
                    WeatherCondition::Smog,
                    WeatherCondition::Sand,
                    WeatherCondition::Unknown,
                ];
                self.data.weather = Some(WeatherSnapshot {
                    low: low.parse()?,
                    high: high.parse()?,
                    condition: *conditions
                        .get(condition.parse::<usize>()?)
                        .ok_or("invalid condition")?,
                });
            }
            [
                "sport",
                phase,
                countdown,
                elapsed,
                distance,
                calories,
                heart,
                locked,
            ] => {
                self.data.sport = wattch5_parity_ui_demo::SportSnapshot {
                    phase: sport_phase(phase)?,
                    countdown: countdown.parse()?,
                    elapsed_seconds: elapsed.parse()?,
                    distance: distance.parse()?,
                    calories: calories.parse()?,
                    heart: optional(heart)?,
                    locked: match *locked {
                        "0" => false,
                        "1" => true,
                        _ => return Err("invalid lock".into()),
                    },
                };
            }
            _ => {
                let event = match p {
                    ["up"] => Event::Up,
                    ["down"] => Event::Down,
                    ["left"] => Event::Left,
                    ["right"] => Event::Right,
                    ["key"] => Event::KeyRelease,
                    ["keyhold"] => Event::KeyLongPress,
                    ["hold"] => Event::LongPress,
                    ["select"] => Event::Select,
                    ["tap", x, y] => Event::Tap {
                        x: x.parse()?,
                        y: y.parse()?,
                    },
                    _ => return Err("unknown command".into()),
                };
                return self
                    .model
                    .handle(event, &self.data)
                    .map_err(|e| format!("{e:?}").into());
            }
        }
        Ok(Effect::None)
    }
}
fn bitmap(pixels: &[u16]) -> Result<Vec<u8>, HostError> {
    if pixels.len() != 16384 {
        return Err("unexpected frame size".into());
    }
    let mut bytes = vec![0; 54];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&49206_u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&128_i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&(-128_i32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    for pixel in pixels {
        bytes.extend_from_slice(&[
            u8::try_from((pixel & 31) * 255 / 31)?,
            u8::try_from(((pixel >> 5) & 63) * 255 / 63)?,
            u8::try_from((pixel >> 11) * 255 / 31)?,
        ]);
    }
    Ok(bytes)
}
fn connection(stream: &mut TcpStream, state: &mut State) -> Result<(), HostError> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let Ok(request) = crate::http::read(stream, "http://127.0.0.1:25340") else {
        stream.write_all(
            b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        )?;
        return Ok(());
    };
    let (status, mime, body) = if request.line.starts_with("GET /frame.bmp") {
        (
            "200 OK",
            "image/bmp",
            bitmap(&crate::support::render(&state.model, &state.data)?)?,
        )
    } else if request.line.starts_with("GET / ") {
        (
            "200 OK",
            "text/html; charset=utf-8",
            include_bytes!("preview.html").to_vec(),
        )
    } else if request.line.starts_with("POST /event ") {
        let result = state.command(&request.body);
        ("200 OK","text/plain; charset=utf-8",format!("{result:?}\nscreen={}; nav={}; draft={}; appearance={:?}\nmeasurement={:?}; timer={:?}; stopwatch={:?}",state.model.screen().id(),state.model.nav_page(),state.model.draft(),state.model.appearance(),state.data.measurements,state.data.timer,state.data.stopwatch).into_bytes())
    } else {
        ("404 Not Found", "text/plain", b"Unknown route".to_vec())
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    Ok(())
}
pub fn serve() -> Result<(), HostError> {
    let listener = TcpListener::bind("127.0.0.1:25340")?;
    let mut state = State::default();
    println!("wattch5 V2 actual Rust preview:http://127.0.0.1:25340/");
    for incoming in listener.incoming() {
        let mut stream = incoming?;
        if let Err(error) = connection(&mut stream, &mut state) {
            eprintln!("Preview connection:{error}");
        }
    }
    Ok(())
}
