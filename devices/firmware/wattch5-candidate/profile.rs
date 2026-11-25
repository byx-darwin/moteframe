//! Strict caller-selected test profile parser shared by build and host tests.
#![allow(
    missing_docs,
    reason = "Build-only schema helper, shared by integration tests"
)]
use std::collections::BTreeMap;
#[derive(Debug, PartialEq, Eq)]
pub struct Profile {
    pub address: [u8; 6],
    pub service: u16,
    pub rx: u16,
    pub tx: u16,
    pub name: String,
    pub product_id: u32,
    pub build_id: u32,
}
pub fn parse(text: &str) -> Result<Profile, &'static str> {
    let mut keys = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or("profile needs key=value")?;
        if keys.insert(k, v).is_some() {
            return Err("duplicate key");
        }
    }
    if keys.len() != 8 {
        return Err("exact eight profile fields required");
    }
    let mut take = |k| keys.remove(k).ok_or("missing profile field");
    if take("kind")? != "wattch5-bringup-test-v1" {
        return Err("test profile kind required");
    }
    let raw = take("address")?;
    if raw.len() != 12 || !raw.is_ascii() {
        return Err("address must be six hexadecimal bytes");
    }
    let mut address = [0; 6];
    for (i, b) in address.iter_mut().enumerate() {
        *b = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).map_err(|_| "address hex")?;
    }
    if address[5] & 0xc0 != 0xc0 || address == [255; 6] || address == [0, 0, 0, 0, 0, 0xc0] {
        return Err("static random address required");
    }
    fn uuid(s: &str) -> Result<u16, &'static str> {
        if s.len() != 4 {
            return Err("16-bit UUID hex required");
        }
        let u = u16::from_str_radix(s, 16).map_err(|_| "UUID hex")?;
        if u < 0x8000 {
            return Err("fixture UUID must have high bit set; no allocation claimed");
        }
        Ok(u)
    }
    let service = uuid(take("service_uuid")?)?;
    let rx = uuid(take("rx_uuid")?)?;
    let tx = uuid(take("tx_uuid")?)?;
    if service == rx || service == tx || rx == tx {
        return Err("distinct UUIDs required");
    }
    let name = take("name")?.to_owned();
    if name.is_empty()
        || name.len() > 20
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("name must be1..20 ASCII letters/digits/-/_");
    }
    let product_id = take("product_id")?
        .parse()
        .map_err(|_| "product_id decimal")?;
    if product_id != 0 {
        return Err("bringup requires unassigned test product0");
    }
    let build_id = take("build_id")?.parse().map_err(|_| "build_id decimal")?;
    if build_id == 0 {
        return Err("nonzero fixture build_id required");
    }
    if !keys.is_empty() {
        return Err("unknown profile key");
    }
    Ok(Profile {
        address,
        service,
        rx,
        tx,
        name,
        product_id,
        build_id,
    })
}
