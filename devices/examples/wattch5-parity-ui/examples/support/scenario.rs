//! Host-only case reconstruction through validated public model actions.
use std::error::Error;
use wattch5_parity_ui_demo::{
    Appearance, Event, Face, Locale, MeasureState, Model, Reading, Screen, Snapshot, SportPhase,
    StopwatchState, Theme, TimeFormat, TimerState, WeatherCondition, WeatherSnapshot, Weekday,
};
fn pick<T: Copy>(s: &str, items: &[T]) -> Result<T, Box<dyn Error>> {
    items
        .get(s.parse::<usize>()?)
        .copied()
        .ok_or_else(|| "invalid index".into())
}
fn optional(s: &str) -> Result<Option<u16>, Box<dyn Error>> {
    if s == "null" {
        Ok(None)
    } else {
        Ok(Some(s.parse()?))
    }
}
fn distance(s: &str) -> Result<u32, Box<dyn Error>> {
    let (a, b) = s.split_once('.').ok_or("invalid fixed point")?;
    if b.len() != 2 {
        return Err("invalid fraction".into());
    }
    Ok(a.parse::<u32>()? * 100 + b.parse::<u32>()?)
}
fn reading(s: &str) -> Result<Reading, Box<dyn Error>> {
    if s == "--" || s == "--/--" {
        Ok(Reading::Missing)
    } else if let Some((a, b)) = s.split_once('/') {
        Ok(Reading::Pressure {
            upper: a.parse()?,
            lower: b.parse()?,
        })
    } else if let Some((a, b)) = s.split_once('.') {
        if b.len() != 1 {
            return Err("invalid reading fraction".into());
        }
        Ok(Reading::Tenth(a.parse::<u16>()? * 10 + b.parse::<u16>()?))
    } else {
        Ok(Reading::Number(s.parse()?))
    }
}
fn sleep(s: &str) -> Result<Option<u16>, Box<dyn Error>> {
    if s == "--" {
        Ok(None)
    } else {
        let (a, b) = s.split_once(':').ok_or("invalid sleep time")?;
        Ok(Some(a.parse::<u16>()? * 60 + b.parse::<u16>()?))
    }
}
fn timer(s: &str) -> Result<TimerState, Box<dyn Error>> {
    Ok(match s {
        "IDLE" => TimerState::Idle,
        "RUNNING" => TimerState::Running,
        "PAUSED" => TimerState::Paused,
        "DONE" => TimerState::Done,
        "UNAVAILABLE" => TimerState::Unavailable,
        _ => return Err("invalid timer state".into()),
    })
}
fn stopwatch(s: &str) -> Result<StopwatchState, Box<dyn Error>> {
    Ok(match s {
        "IDLE" => StopwatchState::Idle,
        "RUNNING" => StopwatchState::Running,
        "PAUSED" => StopwatchState::Paused,
        "UNAVAILABLE" => StopwatchState::Unavailable,
        _ => return Err("invalid stopwatch state".into()),
    })
}
fn state(s: &str) -> Result<MeasureState, Box<dyn Error>> {
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
fn phase(s: &str) -> Result<SportPhase, Box<dyn Error>> {
    Ok(match s {
        "COUNTDOWN" => SportPhase::Countdown,
        "RUNNING" => SportPhase::Running,
        "PAUSED" => SportPhase::Paused,
        "DONE" => SportPhase::Done,
        _ => return Err("invalid workout phase".into()),
    })
}
fn send(m: &mut Model, e: Event, d: &Snapshot<'_>) -> Result<(), Box<dyn Error>> {
    m.handle(e, d).map_err(|e| format!("{e:?}"))?;
    Ok(())
}
pub fn replay(line: &str) -> Result<(String, Model, Snapshot<'static>), Box<dyn Error>> {
    let p: Vec<_> = line.split(',').collect();
    if p.len() != 58 {
        return Err("invalid column count".into());
    }
    let d = parse_data(&p)?;
    let mut m = Model::default();
    m.set_appearance(Appearance {
        brightness: p[5].parse()?,
        face: pick(p[6], &[Face::Stack, Face::Line, Face::Segment])?,
        theme: pick(p[7], &[Theme::Phosphor, Theme::Amber, Theme::Ice])?,
        format: pick(p[8], &[TimeFormat::Hour24, TimeFormat::Hour12])?,
        textsize: p[9].parse()?,
        locale: match p[2] {
            "zh" => Locale::Zh,
            "en" => Locale::En,
            _ => return Err("locale".into()),
        },
    })
    .map_err(|e| format!("{e:?}"))?;
    m.preview(Screen::from_id(p[10]).map_err(|e| format!("{e:?}"))?, &d)
        .map_err(|e| format!("{e:?}"))?;
    let page = Screen::from_id(p[1]).map_err(|e| format!("{e:?}"))?;
    m.preview(page, &d).map_err(|e| format!("{e:?}"))?;
    for _ in 0..p[3].parse::<u8>()? {
        send(&mut m, Event::Up, &d)?;
    }
    if matches!(
        page,
        Screen::Face
            | Screen::Brightness
            | Screen::Theme
            | Screen::Format
            | Screen::Textsize
            | Screen::Language
    ) {
        let want = p[4].parse::<u8>()?;
        if want > 8 {
            return Err("invalid draft".into());
        }
        let event = if page == Screen::Face {
            if m.draft() < want {
                Event::Left
            } else {
                Event::Right
            }
        } else if m.draft() < want {
            Event::Up
        } else {
            Event::Down
        };
        for _ in 0..m.draft().abs_diff(want) {
            send(&mut m, event, &d)?;
        }
        if m.draft() != want {
            return Err("unreachable draft".into());
        }
    }
    for _ in 0..p[22].parse::<usize>()? {
        send(&mut m, Event::Up, &d)?;
    }
    if page == Screen::MessageDetail || page == Screen::Messages || page == Screen::NoticeApp {
        for _ in 0..p[23].parse::<usize>()? {
            send(&mut m, Event::Up, &d)?;
        }
    }
    if !p[24].is_empty() {
        return Err("request fixture requires explicit action path".into());
    }
    if [p[18], p[19], p[20]] != ["0", "5", "0"] || p[21] != "0" {
        return Err("timer editor fixture requires explicit edit path".into());
    }
    Ok((p[0].to_owned(), m, d))
}

fn parse_data(p: &[&str]) -> Result<Snapshot<'static>, Box<dyn Error>> {
    let mut d = Snapshot {
        hour: p[25].parse()?,
        minute: p[26].parse()?,
        month: p[27].parse()?,
        day: p[28].parse()?,
        weekday: match p[29] {
            "MON" => Weekday::Mon,
            "TUE" => Weekday::Tue,
            "WED" => Weekday::Wed,
            "THU" => Weekday::Thu,
            "FRI" => Weekday::Fri,
            "SAT" => Weekday::Sat,
            "SUN" => Weekday::Sun,
            _ => return Err("weekday".into()),
        },
        battery: p[30].parse()?,
        ..Snapshot::default()
    };
    d.timer.seconds = p[31].parse()?;
    d.timer.state = timer(p[32])?;
    d.stopwatch.centis = p[33].parse()?;
    d.stopwatch.state = stopwatch(p[34])?;
    d.steps = p[35].parse()?;
    d.distance = distance(p[36])?;
    d.calories = p[37].parse()?;
    d.sleep_total = sleep(p[38])?;
    d.sleep_deep = sleep(p[39])?;
    d.sleep_light = sleep(p[40])?;
    if p[41] == "0" {
        d.messages = &[];
    } else if p[41] != "2" {
        return Err("unsupported fixture message count".into());
    }
    d.charging = p[42].parse()?;
    d.progress = p[43].parse()?;
    d.applet_available = p[44] == "1";
    d.activity_heart = optional(p[45])?;
    d.heart_high = optional(p[46])?;
    d.heart_low = optional(p[47])?;
    d.sport.phase = phase(p[48])?;
    d.sport.countdown = p[49].parse()?;
    d.sport.elapsed_seconds = p[50].parse()?;
    d.sport.distance = distance(p[51])?;
    d.sport.calories = p[52].parse()?;
    d.sport.heart = optional(p[53])?;
    d.sport.locked = p[54] == "1";
    if p[55] != "null" {
        d.weather = Some(WeatherSnapshot {
            low: p[55].parse()?,
            high: p[56].parse()?,
            condition: pick(
                p[57],
                &[
                    WeatherCondition::Fine,
                    WeatherCondition::Cloudy,
                    WeatherCondition::Rain,
                    WeatherCondition::Snow,
                    WeatherCondition::Smog,
                    WeatherCondition::Sand,
                    WeatherCondition::Unknown,
                ],
            )?,
        });
    }
    for (i, v) in d.measurements.iter_mut().enumerate() {
        v.state = state(p[11])?;
        v.reading = reading(p[12 + i])?;
    }
    d.validate().map_err(|e| format!("snapshot:{e:?}"))?;
    Ok(d)
}
