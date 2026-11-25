//! Host-only CSV reconstruction through valid public model actions.
use crate::support::{choose, page};
use std::error::Error;
use wattch5_retro_ui_demo::{
    Appearance, DisplayData, Event, Face, Model, Notice, Screen, StopwatchState, Theme, TimeFormat,
    TimerState, Weekday,
};
fn send(m: &mut Model, event: Event) -> Result<(), Box<dyn Error>> {
    m.handle(event).map_err(|e| format!("{e:?}"))?;
    Ok(())
}
pub fn replay(line: &str) -> Result<(String, Model), Box<dyn Error>> {
    let p: Vec<_> = line.split(',').collect();
    if p.len() != 21 {
        return Err("invalid scenario column count".into());
    }
    let appearance = Appearance {
        brightness: p[3].parse()?,
        face: choose(
            p[4],
            &[("0", Face::Stack), ("1", Face::Line), ("2", Face::Segment)],
        )?,
        theme: choose(
            p[5],
            &[
                ("0", Theme::Phosphor),
                ("1", Theme::Amber),
                ("2", Theme::Ice),
            ],
        )?,
        format: choose(
            p[6],
            &[("0", TimeFormat::Hour24), ("1", TimeFormat::Hour12)],
        )?,
    };
    let data = parse_data(&p)?;
    let mut m = Model::new(data, appearance).map_err(|e| format!("{e:?}"))?;
    let screen = page(p[1])?;
    m.preview(screen).map_err(|e| format!("{e:?}"))?;
    if matches!(
        screen,
        Screen::Tools | Screen::Quick | Screen::Settings | Screen::Display | Screen::System
    ) {
        for _ in 0..p[2].parse::<u8>()? {
            send(&mut m, Event::Up)?;
        }
    }
    if matches!(
        screen,
        Screen::Brightness | Screen::Face | Screen::Theme | Screen::Format
    ) {
        let draft = p[7].parse::<u8>()?;
        let event = if m.draft() < draft {
            Event::Up
        } else {
            Event::Down
        };
        let distance = m.draft().abs_diff(draft);
        if distance > 4 {
            return Err("invalid draft".into());
        }
        for _ in 0..distance {
            send(&mut m, event)?;
        }
        if m.draft() != draft {
            return Err("unreachable draft".into());
        }
    }
    if matches!(screen, Screen::Confirm | Screen::Request) {
        let request = if screen == Screen::Confirm {
            p[10]
        } else {
            p[8]
        };
        if matches!(request, "Restart" | "PowerOff" | "ResetSettings") {
            m.preview(Screen::System).map_err(|e| format!("{e:?}"))?;
            let n = match request {
                "Restart" => 0,
                "PowerOff" => 1,
                _ => 2,
            };
            for _ in 0..n {
                send(&mut m, Event::Up)?;
            }
            send(&mut m, Event::Select)?;
            if screen == Screen::Request {
                send(&mut m, Event::Tap { x: 90, y: 110 })?;
            }
        } else if request.starts_with("Timer") || request.starts_with("Stopwatch") {
            m.preview(if request.starts_with("Timer") {
                Screen::Timer
            } else {
                Screen::Stopwatch
            })
            .map_err(|e| format!("{e:?}"))?;
            send(&mut m, Event::Select)?;
        } else if request == "DisplayToggle" {
            m.preview(Screen::Dial).map_err(|e| format!("{e:?}"))?;
            send(&mut m, Event::KeyRelease)?;
        } else if request == "DisplaySleep" {
            m.preview(Screen::Quick).map_err(|e| format!("{e:?}"))?;
            send(&mut m, Event::Tap { x: 10, y: 90 })?;
        } else {
            return Err("unsupported request replay".into());
        }
        if p[9] == "unavailable" {
            m.set_notice(Notice::Unavailable);
        }
    }
    Ok((p[0].to_owned(), m))
}
fn parse_data(p: &[&str]) -> Result<DisplayData, Box<dyn Error>> {
    let data = DisplayData {
        hour: p[11].parse()?,
        minute: p[12].parse()?,
        month: p[13].parse()?,
        day: p[14].parse()?,
        weekday: choose(
            p[15],
            &[
                ("MON", Weekday::Mon),
                ("TUE", Weekday::Tue),
                ("WED", Weekday::Wed),
                ("THU", Weekday::Thu),
                ("FRI", Weekday::Fri),
                ("SAT", Weekday::Sat),
                ("SUN", Weekday::Sun),
            ],
        )?,
        battery: p[16].parse()?,
        timer_seconds: p[17].parse()?,
        timer_state: choose(
            p[18],
            &[
                ("IDLE", TimerState::Idle),
                ("RUNNING", TimerState::Running),
                ("PAUSED", TimerState::Paused),
                ("DONE", TimerState::Done),
                ("UNAVAILABLE", TimerState::Unavailable),
            ],
        )?,
        stopwatch_centis: p[19].parse()?,
        stopwatch_state: choose(
            p[20],
            &[
                ("IDLE", StopwatchState::Idle),
                ("RUNNING", StopwatchState::Running),
                ("PAUSED", StopwatchState::Paused),
                ("UNAVAILABLE", StopwatchState::Unavailable),
            ],
        )?,
    };
    Ok(data)
}
