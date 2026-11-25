//! Loopback host preview of the real Rust UI, without device APIs.
#[path = "../../wattch5-ui/examples/support/http.rs"]
mod http;
mod support;
use std::{
    error::Error,
    io::Write,
    net::{TcpListener, TcpStream},
    time::Duration,
};
use wattch5_retro_ui_demo::{Effect, Event, Model, StopwatchState, TimerState};
fn dispatch(model: &mut Model, text: &str) -> Result<Effect, Box<dyn Error>> {
    let p: Vec<_> = text.split_whitespace().collect();
    if let ["view", page] = p.as_slice() {
        model
            .preview(support::page(page)?)
            .map_err(|e| format!("{e:?}"))?;
        return Ok(Effect::None);
    }
    if let ["time", hour, minute] = p.as_slice() {
        let mut data = model.data();
        data.hour = hour.parse()?;
        data.minute = minute.parse()?;
        model.set_data(data).map_err(|e| format!("{e:?}"))?;
        return Ok(Effect::None);
    }
    if let ["timer", state] = p.as_slice() {
        let mut data = model.data();
        data.timer_state = support::choose(
            state,
            &[
                ("IDLE", TimerState::Idle),
                ("RUNNING", TimerState::Running),
                ("PAUSED", TimerState::Paused),
                ("DONE", TimerState::Done),
                ("UNAVAILABLE", TimerState::Unavailable),
            ],
        )?;
        model.set_data(data).map_err(|e| format!("{e:?}"))?;
        return Ok(Effect::None);
    }
    if let ["stopwatch", state] = p.as_slice() {
        let mut data = model.data();
        data.stopwatch_state = support::choose(
            state,
            &[
                ("IDLE", StopwatchState::Idle),
                ("RUNNING", StopwatchState::Running),
                ("PAUSED", StopwatchState::Paused),
                ("UNAVAILABLE", StopwatchState::Unavailable),
            ],
        )?;
        model.set_data(data).map_err(|e| format!("{e:?}"))?;
        return Ok(Effect::None);
    }
    let event = match p.as_slice() {
        ["up"] => Event::Up,
        ["down"] => Event::Down,
        ["left"] => Event::Left,
        ["right"] => Event::Right,
        ["select"] => Event::Select,
        ["key"] => Event::KeyRelease,
        ["keyhold"] => Event::KeyLongPress,
        ["hold"] => Event::LongPress,
        ["tap", x, y] => Event::Tap {
            x: x.parse()?,
            y: y.parse()?,
        },
        _ => return Err("invalid event".into()),
    };
    model.handle(event).map_err(|e| format!("{e:?}").into())
}
fn bitmap(pixels: &[u16]) -> Vec<u8> {
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
            u8::try_from((pixel & 31) * 255 / 31).unwrap_or_default(),
            u8::try_from(((pixel >> 5) & 63) * 255 / 63).unwrap_or_default(),
            u8::try_from((pixel >> 11) * 255 / 31).unwrap_or_default(),
        ]);
    }
    bytes
}
fn connection(stream: &mut TcpStream, model: &mut Model) -> Result<(), Box<dyn Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let Ok(request) = http::read(stream, "http://127.0.0.1:25339") else {
        stream.write_all(
            b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        )?;
        return Ok(());
    };
    let (status, mime, body) = if request.line.starts_with("GET /frame.bmp") {
        ("200 OK", "image/bmp", bitmap(&support::render(*model)?))
    } else if request.line.starts_with("GET / ") {
        (
            "200 OK",
            "text/html; charset=utf-8",
            include_bytes!("support/preview.html").to_vec(),
        )
    } else if request.line.starts_with("POST /event ") {
        let result = dispatch(model, &request.body);
        (
            "200 OK",
            "text/plain; charset=utf-8",
            format!(
                "{result:?}\nscreen={:?}; focus={}; appearance={:?}; draft={}",
                model.screen(),
                model.focus(),
                model.appearance(),
                model.draft()
            )
            .into_bytes(),
        )
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
fn main() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:25339")?;
    let mut model = Model::default();
    println!("wattch 5 real Rust preview:http://127.0.0.1:25339");
    for incoming in listener.incoming() {
        let mut stream = incoming?;
        if let Err(error) = connection(&mut stream, &mut model) {
            eprintln!("Preview connection:{error}");
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_should_reject_bad_snapshot_without_partial_mutation() {
        let mut m = Model::default();
        let before = m;
        assert!(dispatch(&mut m, "time 24 0").is_err());
        assert_eq!(m, before);
        assert!(dispatch(&mut m, "tap -1 20").is_err());
        assert_eq!(m, before);
    }
}
