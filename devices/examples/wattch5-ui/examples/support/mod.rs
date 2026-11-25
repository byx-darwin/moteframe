//! Host raster/export/control transport, not linked into device runtime.
mod http;
use device_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use device_ui::{DirtyRegion, graphics::pixelcolor::Rgb565, repaint};
use std::{error::Error, io::Write, net::TcpListener, time::Duration};
use wattch5_ui_demo::{Bundle, Event, Model};
#[derive(Debug)]
struct Raster(Vec<Packed>);
impl Display for Raster {
    type Error = &'static str;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(128, 128)
    }
    fn write(&mut self, area: Rect, pixels: &[Packed]) -> Result<(), Self::Error> {
        if !area.fits(self.dimensions()) || usize::try_from(area.area()).ok() != Some(pixels.len())
        {
            return Err("invalid frame write");
        }
        for (row, colors) in pixels.chunks(usize::from(area.width)).enumerate() {
            let start = (usize::from(area.y) + row) * 128 + usize::from(area.x);
            self.0[start..start + colors.len()].copy_from_slice(colors);
        }
        Ok(())
    }
}
pub fn render(model: Model, assets: &Bundle<'_>) -> Result<Vec<u16>, Box<dyn Error>> {
    let mut raster = Raster(vec![Packed(0); 16384]);
    let mut dirty = DirtyRegion::new(raster.dimensions());
    dirty.invalidate_all();
    repaint(
        &mut raster,
        &mut dirty,
        &mut [Packed(0); 128],
        Rgb565::new(0, 0, 0),
        |tile| model.draw(assets, tile),
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(raster.0.into_iter().map(|p| p.0).collect())
}
fn bitmap(pixels: &[u16]) -> Vec<u8> {
    let mut bytes = vec![0_u8; 54];
    bytes[..2].copy_from_slice(b"BM");
    bytes[2..6].copy_from_slice(&49206_u32.to_le_bytes());
    bytes[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bytes[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&128_i32.to_le_bytes());
    bytes[22..26].copy_from_slice(&(-128_i32).to_le_bytes());
    bytes[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&24_u16.to_le_bytes());
    for p in pixels {
        bytes.extend_from_slice(&[
            u8::try_from((p & 31) * 255 / 31).unwrap_or_default(),
            u8::try_from(((p >> 5) & 63) * 255 / 63).unwrap_or_default(),
            u8::try_from((p >> 11) * 255 / 31).unwrap_or_default(),
        ]);
    }
    bytes
}
fn event(text: &str) -> Result<Event, Box<dyn Error>> {
    let words: Vec<_> = text.split_whitespace().collect();
    match words.as_slice() {
        ["up"] => Ok(Event::Up),
        ["down"] => Ok(Event::Down),
        ["left"] => Ok(Event::Left),
        ["right"] => Ok(Event::Right),
        ["press"] => Ok(Event::LongPress),
        ["key"] => Ok(Event::KeyRelease),
        ["hold"] => Ok(Event::KeyLongPress),
        ["tap", x, y] => Ok(Event::Tap {
            x: x.parse()?,
            y: y.parse()?,
        }),
        _ => Err("invalid event".into()),
    }
}
/// Local interactive preview uses the same Rust model as the firmware adapter.
pub fn serve(assets: &Bundle<'_>) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:25331")?;
    let mut model = Model::new(10, 50, 3).map_err(|e| format!("{e:?}"))?;
    println!("wattch 5 visual demo: http://127.0.0.1:25331 (sample data; no hardware actions)");
    for incoming in listener.incoming() {
        let mut stream = incoming?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let request = match http::read(&mut stream, "http://127.0.0.1:25331") {
            Ok(request) => request,
            Err(error) => {
                write!(
                    stream,
                    "HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
                )?;
                eprintln!("Rejected preview request: {error}");
                continue;
            }
        };
        let line = request.line.as_str();
        let (mime, body) = if line.starts_with("GET /frame.bmp") {
            ("image/bmp", bitmap(&render(model, assets)?))
        } else if line.starts_with("POST /event ") {
            let body = request.body.as_str();
            let result = event(body)
                .and_then(|input| model.handle(input).map_err(|e| format!("{e:?}").into()));
            (
                "text/plain; charset=utf-8",
                format!(
                    "{result:?}; page={:?}; level={}",
                    model.page(),
                    model.level()
                )
                .into_bytes(),
            )
        } else if line.starts_with("GET / ") {
            (
                "text/html; charset=utf-8",
                include_bytes!("preview.html").to_vec(),
            )
        } else {
            ("text/plain", b"Unknown route".to_vec())
        };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)?;
    }
    Ok(())
}
