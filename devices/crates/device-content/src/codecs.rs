use crate::Error;
#[cfg(any(
    feature = "phone",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "weather",
    feature = "contacts",
    feature = "memo"
))]
use crate::wire::ensure;
#[cfg(any(
    feature = "phone",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "companion-radio"
))]
use crate::wire::{Reader, Writer};
/// Inclusive product-selected unsigned range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// Minimum.
    pub min: u32,
    /// Maximum.
    pub max: u32,
}
#[cfg(any(feature = "phone", feature = "find"))]
impl Range {
    fn check(self, v: u32) -> Result<(), Error> {
        ensure(self.min <= self.max && v >= self.min && v <= self.max)
    }
}
/// Inclusive UTC milliseconds accepted by a product calendar backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    /// Earliest representable UTC milliseconds.
    pub min: i64,
    /// Latest representable UTC milliseconds.
    pub max: i64,
}
/// Product-selected codec limits. Missing required values fail closed.
#[derive(Clone, Copy, Default)]
pub struct Manifest<'a> {
    /// UTF-8 byte bound, including every entry in text arrays.
    pub string_max: Option<u16>,
    /// Total object size bound, independent of property storage capacity.
    pub object_max: Option<u32>,
    /// Actual whole-property capacity1..512 bytes, required only by candidate30.P0004.
    /// None leaves P4 unavailable without affecting existing property codecs.
    pub property_max: Option<u16>,
    /// Maximum array count.
    pub count_max: Option<u16>,
    /// Supported heartbeat timeout seconds.
    pub heartbeat_timeout_s: Option<Range>,
    /// Supported heartbeat grace seconds.
    pub heartbeat_grace_s: Option<Range>,
    /// Supported find delay seconds.
    pub find_delay_s: Option<Range>,
    /// Explicit actual independent Classic/call-Bluetooth backend selection; unrelated to BLE.
    pub independent_call_bluetooth: bool,
    /// Actual calendar backend's supported UTC range, required by memo objects.
    pub utc_ms: Option<TimeRange>,
    /// Versioned watchface manifest; required for watchface metadata.
    pub watchface: Option<WatchfaceManifest<'a>>,
}
/// Versioned supported watchface values and actual logical canvas.
#[derive(Clone, Copy)]
pub struct WatchfaceManifest<'a> {
    /// Nonzero manifest revision.
    pub revision: u16,
    /// Actual logical width.
    pub width: u16,
    /// Actual logical height.
    pub height: u16,
    /// Supported dial IDs.
    pub dials: &'a [u32],
    /// Supported groups.
    pub groups: &'a [u16],
    /// Supported element IDs.
    pub elements: &'a [u16],
    /// Supported style IDs.
    pub styles: &'a [u16],
    /// True only when resource verification/installation is available.
    pub verified_resources: bool,
}
#[cfg(any(
    feature = "phone",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
fn text_max(m: &Manifest<'_>) -> Result<u16, Error> {
    m.string_max.ok_or(Error::Unavailable)
}
#[cfg(any(feature = "weather", feature = "notifications", feature = "watchface"))]
fn count_max(m: &Manifest<'_>) -> Result<u16, Error> {
    m.count_max.ok_or(Error::Unavailable)
}
#[cfg(any(feature = "phone", feature = "find"))]
fn range(v: u32, r: Option<Range>) -> Result<(), Error> {
    r.ok_or(Error::Unavailable)?.check(v)
}
macro_rules! packet_debug {
    ($t:ty) => {
        impl core::fmt::Debug for $t {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct(stringify!($t))
                    .field("length", &core::mem::size_of_val(self))
                    .finish_non_exhaustive()
            }
        }
    };
}
/// Phone platform and reported screen characteristics, 07.P0001.
#[cfg(feature = "phone")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PhonePlatform {
    /// 1 iOS, 2 Android, 3 other (candidate allocation).
    pub platform: u8,
    /// Zero means unknown, never inferred.
    pub ppi: u16,
    /// Actual reported width, zero unknown.
    pub width: u16,
    /// Actual reported height, zero unknown.
    pub height: u16,
}
#[cfg(feature = "phone")]
packet_debug!(PhonePlatform);
/// OS-reported phone status, 07.P0002.
#[cfg(feature = "phone")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PhoneStatus {
    /// 0 unknown, 1 off, 2 on.
    pub screen: u8,
    /// 0 other, 1 video, 2 reading, 3 system camera. Unknown clears validity bit1 and uses0.
    pub app_class: u8,
    /// OS reports an unlocked screen.
    pub unlocked: bool,
    /// Bits 0 screen, 1 app class, 2 unlocked.
    pub valid_mask: u8,
}
#[cfg(feature = "phone")]
packet_debug!(PhoneStatus);
/// Phone location, 07.P0003. Invalid requires canonical zero coordinates.
#[cfg(feature = "phone")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PhoneLocation {
    /// Latitude degrees times 10^7.
    pub lat_e7: i32,
    /// Longitude degrees times 10^7.
    pub lon_e7: i32,
    /// Whether the coordinates are available.
    pub valid: bool,
}
#[cfg(feature = "phone")]
packet_debug!(PhoneLocation);
/// Phone heartbeat policy, 07.P0005; ranges come from the product manifest.
#[cfg(feature = "phone")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatPolicy {
    /// Enable monitoring.
    pub enabled: bool,
    /// Timeout seconds.
    pub timeout_s: u16,
    /// Grace seconds.
    pub grace_s: u16,
}
#[cfg(feature = "phone")]
packet_debug!(HeartbeatPolicy);
/// Find disconnect policy, 06.P0001.
#[cfg(feature = "find")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FindPolicy {
    /// Enable policy.
    pub enabled: bool,
    /// Product-supported delay seconds.
    pub delay_s: u16,
}
#[cfg(feature = "find")]
packet_debug!(FindPolicy);
/// Validated categories, 30.P0001. Omitted categories deny notifications.
#[cfg(feature = "notifications")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CategorySwitches<'a> {
    entries: &'a [u8],
}
#[cfg(feature = "notifications")]
impl CategorySwitches<'_> {
    /// Return whether a category is explicitly allowed.
    #[must_use]
    pub fn allows(self, category: u16) -> bool {
        self.entries
            .chunks_exact(3)
            .any(|e| u16::from_le_bytes([e[0], e[1]]) == category && e[2] == 1)
    }
    /// Number of categories.
    #[must_use]
    pub fn len(self) -> usize {
        self.entries.len() / 3
    }
    /// Whether the policy denies all categories.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.entries.is_empty()
    }
}
#[cfg(feature = "notifications")]
packet_debug!(CategorySwitches<'_>);
/// Validated unique nonempty iOS bundle identifiers, 30.P0003.
#[cfg(feature = "notifications")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AppIdentifiers<'a> {
    entries: &'a [u8],
    count: u16,
}
#[cfg(feature = "notifications")]
impl<'a> AppIdentifiers<'a> {
    /// Number of identifiers.
    #[must_use]
    pub const fn len(self) -> u16 {
        self.count
    }
    /// Whether every application is denied.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.count == 0
    }
    /// Check exact case-sensitive membership.
    #[must_use]
    pub fn contains(self, id: &str) -> bool {
        let mut r = Reader::new(self.entries);
        for _ in 0..self.count {
            if let Ok(s) = r.text(u16::MAX)
                && s == id
            {
                return true;
            }
        }
        false
    }
    /// Visit validated identifiers without allocation.
    #[must_use]
    pub fn iter(self) -> IdentifierIter<'a> {
        IdentifierIter {
            reader: Reader::new(self.entries),
            remaining: self.count,
        }
    }
}
/// Iterator over validated identifiers.
#[cfg(feature = "notifications")]
pub struct IdentifierIter<'a> {
    reader: Reader<'a>,
    remaining: u16,
}
#[cfg(feature = "notifications")]
impl<'a> Iterator for IdentifierIter<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        self.reader.text(u16::MAX).ok()
    }
}
#[cfg(feature = "notifications")]
packet_debug!(AppIdentifiers<'_>);
/// Validated cross-platform source-app rules, candidate30.P0004/schema1.
/// Keys are nonempty UTF-8 strings in strictly ascending byte order.
#[cfg(feature = "notifications")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AppSwitches<'a> {
    entries: &'a [u8],
    count: u8,
}
#[cfg(feature = "notifications")]
impl AppSwitches<'_> {
    /// Return the exact source-app override; absence falls back to the category policy.
    #[must_use]
    pub fn rule_for(self, source_app: &str) -> Option<bool> {
        let mut r = Reader::new(self.entries);
        for _ in 0..self.count {
            let key = r.text(u16::MAX).ok()?;
            let enabled = r.boolean().ok()?;
            match key.as_bytes().cmp(source_app.as_bytes()) {
                core::cmp::Ordering::Equal => return Some(enabled),
                core::cmp::Ordering::Greater => return None,
                core::cmp::Ordering::Less => {}
            }
        }
        None
    }
}
#[cfg(feature = "notifications")]
packet_debug!(AppSwitches<'_>);
/// Evaluate notification preferences without performing OS or device actions.
/// Global off denies everything. An exact app override wins over category policy;
/// a missing app falls back to the category. Pass `Some` for the independently selected
/// iOS identifier policy and `None` for non-iOS sources. This does not prove OS authorization.
#[cfg(feature = "notifications")]
#[must_use]
pub fn notification_allowed(
    global_enabled: bool,
    categories: CategorySwitches<'_>,
    apps: AppSwitches<'_>,
    source_app: &str,
    category: u16,
    ios_identifiers: Option<AppIdentifiers<'_>>,
) -> bool {
    global_enabled
        && category <= 7
        && apps
            .rule_for(source_app)
            .unwrap_or_else(|| categories.allows(category))
        && ios_identifiers.is_none_or(|ids| ids.contains(source_app))
}
/// Typed property values. Every variant belongs to an explicit feature.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface"
))]
pub enum Property<'a> {
    /// 06.P0001.
    #[cfg(feature = "find")]
    FindPolicy(FindPolicy),
    /// 08.P0001 independent Classic state.
    #[cfg(feature = "companion-radio")]
    ClassicBluetooth(crate::ClassicBluetooth),
    /// 07.P0001.
    #[cfg(feature = "phone")]
    PhonePlatform(PhonePlatform),
    /// 07.P0002.
    #[cfg(feature = "phone")]
    PhoneStatus(PhoneStatus),
    /// 07.P0003.
    #[cfg(feature = "phone")]
    PhoneLocation(PhoneLocation),
    /// 07.P0004.
    #[cfg(feature = "phone")]
    Address(&'a str),
    /// 07.P0005.
    #[cfg(feature = "phone")]
    HeartbeatPolicy(HeartbeatPolicy),
    /// 30.P0001.
    #[cfg(feature = "notifications")]
    CategorySwitches(CategorySwitches<'a>),
    /// 30.P0002.
    #[cfg(feature = "notifications")]
    NotificationsEnabled(bool),
    /// 30.P0003.
    #[cfg(feature = "notifications")]
    AppIdentifiers(AppIdentifiers<'a>),
    /// Additional candidate30.P0004/schema1; separate from the original catalogue.
    #[cfg(feature = "notifications")]
    AppSwitches(AppSwitches<'a>),
    /// 31.P0001.
    #[cfg(feature = "watchface")]
    CurrentWatchface(u32),
    /// Lifetime marker for builds without borrowed properties; never an endpoint.
    Hidden(core::marker::PhantomData<&'a ()>),
}
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface"
))]
packet_debug!(Property<'_>);
/// Stable schema metadata, without claiming a product backend exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    /// Module.
    pub module: u8,
    /// Property key or object kind.
    pub key: u16,
    /// Optional module schema revision.
    pub revision: u16,
    /// True for object values.
    pub object: bool,
}
/// Return a compiled schema's metadata.
/// # Errors
/// Returns `Unknown` for disabled or unsupported endpoints.
pub fn descriptor(module: u8, key: u16, object: bool) -> Result<Descriptor, Error> {
    let revision = match (module, key, object) {
        #[cfg(feature = "find")]
        (6, 1, false) => 1,
        #[cfg(feature = "phone")]
        (7, 1..=5, false) => 2,
        #[cfg(feature = "companion-radio")]
        (8, 1, false) => 2,
        #[cfg(feature = "weather")]
        (0x32, 1, true) => 2,
        #[cfg(feature = "contacts")]
        (0x34, 1, true) => 1,
        #[cfg(feature = "memo")]
        (0x36, 1, true) => 1,
        #[cfg(feature = "notifications")]
        (0x30, 1..=3, false) | (0x30, 1, true) => 2,
        #[cfg(feature = "notifications")]
        (0x30, 4, false) => 1,
        #[cfg(feature = "watchface")]
        (0x31, 1, _) => 2,
        #[cfg(feature = "wallet")]
        (0x35, 1, true) => 1,
        _ => return Err(Error::Unknown),
    };
    Ok(Descriptor {
        module,
        key,
        revision,
        object,
    })
}
/// Decode and validate an exact property value using explicit product limits.
/// # Errors
/// Returns `Unknown`, `Invalid`, or `Unavailable`; performs no state change.
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface"
))]
pub fn decode_property<'a>(
    module: u8,
    key: u16,
    bytes: &'a [u8],
    m: &Manifest<'_>,
) -> Result<Property<'a>, Error> {
    descriptor(module, key, false)?;
    let mut r = Reader::new(bytes);
    let v = match (module, key) {
        #[cfg(feature = "phone")]
        (7, 1..=5) => decode_phone(key, &mut r, m)?,
        #[cfg(feature = "companion-radio")]
        (8, 1) => Property::ClassicBluetooth(crate::ClassicBluetooth::decode(&mut r, m)?),
        #[cfg(feature = "find")]
        (6, 1) => {
            let v = FindPolicy {
                enabled: r.boolean()?,
                delay_s: r.u16()?,
            };
            range(u32::from(v.delay_s), m.find_delay_s)?;
            Property::FindPolicy(v)
        }
        #[cfg(feature = "notifications")]
        (0x30, 1) => {
            let n = r.u8()?;
            ensure(u16::from(n) <= count_max(m)?)?;
            let entries = r.take(usize::from(n) * 3)?;
            let mut mask = 0u16;
            for e in entries.chunks_exact(3) {
                let c = u16::from_le_bytes([e[0], e[1]]);
                ensure(c <= 7 && e[2] <= 1 && mask & (1 << c) == 0)?;
                mask |= 1 << c;
            }
            Property::CategorySwitches(CategorySwitches { entries })
        }
        #[cfg(feature = "notifications")]
        (0x30, 2) => Property::NotificationsEnabled(r.boolean()?),
        #[cfg(feature = "notifications")]
        (0x30, 3) => {
            let count = r.u16()?;
            ensure(count <= count_max(m)?)?;
            let entries = r.rest();
            for i in 0..count {
                let s = r.text(text_max(m)?)?;
                ensure(!s.is_empty())?;
                let mut prev = Reader::new(entries);
                for _ in 0..i {
                    ensure(prev.text(text_max(m)?)? != s)?;
                }
            }
            Property::AppIdentifiers(AppIdentifiers { entries, count })
        }
        #[cfg(feature = "notifications")]
        (0x30, 4) => {
            let limit = m.property_max.ok_or(Error::Unavailable)?;
            if limit == 0 {
                return Err(Error::Unavailable);
            }
            ensure((1..=512).contains(&limit))?;
            ensure(u64::try_from(bytes.len()).map_err(|_| Error::Capacity)? <= u64::from(limit))?;
            let max_text = text_max(m)?;
            let count = r.u8()?;
            ensure(u16::from(count) <= count_max(m)?)?;
            let entries = r.rest();
            let mut previous: Option<&str> = None;
            for _ in 0..count {
                let key = r.text(max_text)?;
                ensure(!key.is_empty() && previous.is_none_or(|p| p.as_bytes() < key.as_bytes()))?;
                r.boolean()?;
                previous = Some(key);
            }
            Property::AppSwitches(AppSwitches { entries, count })
        }
        #[cfg(feature = "watchface")]
        (0x31, 1) => {
            let id = r.u32()?;
            let w = watch_manifest(m)?;
            ensure(w.dials.contains(&id))?;
            Property::CurrentWatchface(id)
        }
        _ => return Err(Error::Unknown),
    };
    r.end()?;
    Ok(v)
}
#[cfg(feature = "phone")]
fn decode_phone<'a>(key: u16, r: &mut Reader<'a>, m: &Manifest<'_>) -> Result<Property<'a>, Error> {
    Ok(match key {
        #[cfg(feature = "phone")]
        1 => {
            let v = PhonePlatform {
                platform: r.u8()?,
                ppi: r.u16()?,
                width: r.u16()?,
                height: r.u16()?,
            };
            ensure((1..=3).contains(&v.platform))?;
            Property::PhonePlatform(v)
        }
        #[cfg(feature = "phone")]
        2 => {
            let v = PhoneStatus {
                screen: r.u8()?,
                app_class: r.u8()?,
                unlocked: r.boolean()?,
                valid_mask: r.u8()?,
            };
            ensure(
                v.screen <= 2
                    && v.app_class <= 3
                    && v.valid_mask & !7 == 0
                    && (v.valid_mask & 1 != 0 || v.screen == 0)
                    && (v.valid_mask & 2 != 0 || v.app_class == 0)
                    && (v.valid_mask & 4 != 0 || !v.unlocked),
            )?;
            Property::PhoneStatus(v)
        }
        #[cfg(feature = "phone")]
        3 => {
            let v = PhoneLocation {
                lat_e7: r.i32()?,
                lon_e7: r.i32()?,
                valid: r.boolean()?,
            };
            ensure(
                (-900_000_000..=900_000_000).contains(&v.lat_e7)
                    && (-1_800_000_000..=1_800_000_000).contains(&v.lon_e7)
                    && (v.valid || (v.lat_e7 == 0 && v.lon_e7 == 0)),
            )?;
            Property::PhoneLocation(v)
        }
        #[cfg(feature = "phone")]
        4 => Property::Address(r.text(text_max(m)?)?),
        #[cfg(feature = "phone")]
        5 => {
            let v = HeartbeatPolicy {
                enabled: r.boolean()?,
                timeout_s: r.u16()?,
                grace_s: r.u16()?,
            };
            range(u32::from(v.timeout_s), m.heartbeat_timeout_s)?;
            range(u32::from(v.grace_s), m.heartbeat_grace_s)?;
            ensure(!v.enabled || v.timeout_s > 0)?;
            Property::HeartbeatPolicy(v)
        }
        _ => return Err(Error::Unknown),
    })
}
/// Encode a typed property and revalidate its exact wire representation.
/// # Errors
/// Returns invalid fields, insufficient capacity, missing limits, or an unsupported variant.
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface"
))]
pub fn encode_property(v: Property<'_>, out: &mut [u8], m: &Manifest<'_>) -> Result<usize, Error> {
    let mut w = Writer::new(out);
    let (module, key) = match v {
        #[cfg(feature = "companion-radio")]
        Property::ClassicBluetooth(value) => {
            value.encode(&mut w, m)?;
            (8, 1)
        }

        #[cfg(feature = "phone")]
        Property::PhonePlatform(v) => {
            w.u8(v.platform)?;
            w.u16(v.ppi)?;
            w.u16(v.width)?;
            w.u16(v.height)?;
            (7, 1)
        }
        #[cfg(feature = "phone")]
        Property::PhoneStatus(v) => {
            w.u8(v.screen)?;
            w.u8(v.app_class)?;
            w.u8(u8::from(v.unlocked))?;
            w.u8(v.valid_mask)?;
            (7, 2)
        }
        #[cfg(feature = "phone")]
        Property::PhoneLocation(v) => {
            w.i32(v.lat_e7)?;
            w.i32(v.lon_e7)?;
            w.u8(u8::from(v.valid))?;
            (7, 3)
        }
        #[cfg(feature = "phone")]
        Property::Address(v) => {
            w.text(v, text_max(m)?)?;
            (7, 4)
        }
        #[cfg(feature = "phone")]
        Property::HeartbeatPolicy(v) => {
            w.u8(u8::from(v.enabled))?;
            w.u16(v.timeout_s)?;
            w.u16(v.grace_s)?;
            (7, 5)
        }
        #[cfg(feature = "find")]
        Property::FindPolicy(v) => {
            w.u8(u8::from(v.enabled))?;
            w.u16(v.delay_s)?;
            (6, 1)
        }
        #[cfg(feature = "notifications")]
        Property::CategorySwitches(v) => {
            w.u8(u8::try_from(v.len()).map_err(|_| Error::Capacity)?)?;
            w.bytes(v.entries)?;
            (0x30, 1)
        }
        #[cfg(feature = "notifications")]
        Property::NotificationsEnabled(v) => {
            w.u8(u8::from(v))?;
            (0x30, 2)
        }
        #[cfg(feature = "notifications")]
        Property::AppIdentifiers(v) => {
            w.u16(v.count)?;
            w.bytes(v.entries)?;
            (0x30, 3)
        }
        #[cfg(feature = "notifications")]
        Property::AppSwitches(v) => {
            w.u8(v.count)?;
            w.bytes(v.entries)?;
            (0x30, 4)
        }
        #[cfg(feature = "watchface")]
        Property::CurrentWatchface(v) => {
            w.u32(v)?;
            (0x31, 1)
        }
        Property::Hidden(_) => return Err(Error::Unknown),
    };
    let n = w.len();
    decode_property(module, key, &out[..n], m)?;
    Ok(n)
}
/// Compatibility validation entry for phone fields that require no manifest.
/// # Errors
/// Returns invalid data, unknown keys, or missing product limits for P4/P5.
#[cfg(feature = "phone")]
pub fn validate_phone_property(key: u16, bytes: &[u8]) -> Result<(), Error> {
    decode_property(7, key, bytes, &Manifest::default()).map(|_| ())
}
/// Display-only namecard/wallet metadata, never payment authority.
#[cfg(feature = "wallet")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Wallet<'a> {
    /// 1 namecard, 2 wallet display.
    pub category: u8,
    /// Candidate0..8 retained;9 Twitter,10 Instagram,11 Gmail,12 LinkedIn,13 Snapchat,
    ///14 KakaoTalk,15 Viber,16 KiK,17 `UnionPay`. Future numbers are read-only.
    pub provider: u16,
    /// Display title.
    pub title: &'a str,
    /// Display text; never automatically executed.
    pub content: &'a str,
}
#[cfg(feature = "wallet")]
packet_debug!(Wallet<'_>);
/// Typed watchface element metadata.
#[cfg(feature = "watchface")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WatchfaceElement {
    /// Product element ID.
    pub element: u16,
    /// Visibility.
    pub visible: bool,
    /// Logical canvas x coordinate.
    pub x: u16,
    /// Logical canvas y coordinate.
    pub y: u16,
    /// RGB565 color.
    pub color_rgb565: u16,
    /// Product style ID.
    pub style: u16,
}
#[cfg(feature = "watchface")]
packet_debug!(WatchfaceElement);
/// Validated watchface metadata backed by borrowed wire elements.
#[cfg(feature = "watchface")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Watchface<'a> {
    /// Dial identifier, distinct from object ID.
    pub dial_id: u32,
    /// Product group ID.
    pub group: u16,
    /// 0 not installed, 1 metadata only, 2 verified ready, 3 failed.
    pub download_state: u8,
    /// Resource version label.
    pub resource_version: &'a str,
    elements: &'a [u8],
}
#[cfg(feature = "watchface")]
impl Watchface<'_> {
    /// Return validated element count.
    #[must_use]
    pub fn len(self) -> usize {
        self.elements.len() / 11
    }
    /// Whether there are no elements.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.elements.is_empty()
    }
    /// Read one typed element.
    /// # Errors
    /// Returns `NotFound` for an invalid index.
    pub fn element(self, index: usize) -> Result<WatchfaceElement, Error> {
        let start = index.checked_mul(11).ok_or(Error::NotFound)?;
        let end = start.checked_add(11).ok_or(Error::NotFound)?;
        let mut r = Reader::new(self.elements.get(start..end).ok_or(Error::NotFound)?);
        read_element(&mut r)
    }
}
#[cfg(feature = "watchface")]
packet_debug!(Watchface<'_>);
#[cfg(feature = "watchface")]
fn read_element(r: &mut Reader<'_>) -> Result<WatchfaceElement, Error> {
    Ok(WatchfaceElement {
        element: r.u16()?,
        visible: r.boolean()?,
        x: r.u16()?,
        y: r.u16()?,
        color_rgb565: r.u16()?,
        style: r.u16()?,
    })
}
#[cfg(feature = "watchface")]
fn watch_manifest<'a>(m: &Manifest<'a>) -> Result<WatchfaceManifest<'a>, Error> {
    let w = m.watchface.ok_or(Error::Unavailable)?;
    ensure(w.revision > 0 && w.width > 0 && w.height > 0)?;
    Ok(w)
}
/// Typed content object. Borrowed contents never imply source authority.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(any(
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
pub enum Object<'a> {
    /// 32.O0001 shared strict forecast, without a freshness claim.
    #[cfg(feature = "weather")]
    Weather(crate::Weather<'a>),
    /// 34.O0001 frozen contact.
    #[cfg(feature = "contacts")]
    Contact(crate::Contact<'a>),
    /// 36.O0001 frozen memo.
    #[cfg(feature = "memo")]
    Memo(crate::Memo<'a>),
    /// Existing shared schema2 notification.
    #[cfg(feature = "notifications")]
    Notification(device_models::Notification<'a>),
    /// 31.O0001 metadata.
    #[cfg(feature = "watchface")]
    Watchface(Watchface<'a>),
    /// 35.O0001 display content.
    #[cfg(feature = "wallet")]
    Wallet(Wallet<'a>),
    /// Lifetime marker, never an endpoint.
    Hidden(core::marker::PhantomData<&'a ()>),
}
#[cfg(any(
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
packet_debug!(Object<'_>);
/// Decode content objects for a write. Unknown wallet providers are rejected.
/// # Errors
/// Returns unknown endpoint, malformed data, missing manifest, or unavailable resource verification.
#[cfg(any(
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
pub fn decode_object<'a>(
    module: u8,
    kind: u16,
    bytes: &'a [u8],
    m: &Manifest<'_>,
) -> Result<Object<'a>, Error> {
    decode_object_mode(module, kind, bytes, m, true)
}
/// Decode readback; preserves future wallet provider numbers.
/// # Errors
/// Returns malformed data, unknown endpoint or absent manifest limits.
#[cfg(any(
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
pub fn decode_object_read<'a>(
    module: u8,
    kind: u16,
    bytes: &'a [u8],
    m: &Manifest<'_>,
) -> Result<Object<'a>, Error> {
    decode_object_mode(module, kind, bytes, m, false)
}
#[cfg(any(
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
fn decode_object_mode<'a>(
    module: u8,
    kind: u16,
    bytes: &'a [u8],
    manifest: &Manifest<'_>,
    write: bool,
) -> Result<Object<'a>, Error> {
    descriptor(module, kind, true)?;
    ensure(
        u64::try_from(bytes.len()).map_err(|_| Error::Capacity)?
            <= u64::from(manifest.object_max.ok_or(Error::Unavailable)?),
    )?;
    match (module, kind) {
        #[cfg(feature = "weather")]
        (0x32, 1) => {
            return Ok(Object::Weather(
                crate::Weather::decode(bytes, weather_max(manifest)?)
                    .map_err(|_| Error::Invalid)?,
            ));
        }
        #[cfg(feature = "contacts")]
        (0x34, 1) => return Ok(Object::Contact(crate::Contact::decode(bytes, manifest)?)),
        #[cfg(feature = "memo")]
        (0x36, 1) => return Ok(Object::Memo(crate::Memo::decode(bytes, manifest)?)),
        _ => {}
    }
    #[cfg(feature = "notifications")]
    if (module, kind) == (0x30, 1) {
        let value = device_models::Notification::decode(bytes, usize::from(text_max(manifest)?))
            .map_err(|_| Error::Invalid)?;
        return Ok(Object::Notification(value));
    }
    #[cfg(not(feature = "wallet"))]
    let _ = write;
    #[cfg(any(feature = "wallet", feature = "watchface"))]
    {
        let mut reader = Reader::new(bytes);
        let value = match (module, kind) {
            #[cfg(feature = "wallet")]
            (0x35, 1) => {
                let value = Wallet {
                    category: reader.u8()?,
                    provider: reader.u16()?,
                    title: reader.text(text_max(manifest)?)?,
                    content: reader.text(text_max(manifest)?)?,
                };
                ensure((1..=2).contains(&value.category) && (!write || value.provider <= 17))?;
                Object::Wallet(value)
            }
            #[cfg(feature = "watchface")]
            (0x31, 1) => {
                let watch = watch_manifest(manifest)?;
                let dial_id = reader.u32()?;
                let group = reader.u16()?;
                let download_state = reader.u8()?;
                ensure(
                    watch.dials.contains(&dial_id)
                        && watch.groups.contains(&group)
                        && download_state <= 3,
                )?;
                if download_state == 2 && !watch.verified_resources {
                    return Err(Error::Unavailable);
                }
                let resource_version = reader.text(text_max(manifest)?)?;
                let count = reader.u8()?;
                ensure(u16::from(count) <= count_max(manifest)?)?;
                let elements = reader.take(usize::from(count) * 11)?;
                let mut element_reader = Reader::new(elements);
                for _ in 0..count {
                    let value = read_element(&mut element_reader)?;
                    ensure(
                        watch.elements.contains(&value.element)
                            && watch.styles.contains(&value.style)
                            && value.x < watch.width
                            && value.y < watch.height,
                    )?;
                }
                Object::Watchface(Watchface {
                    dial_id,
                    group,
                    download_state,
                    resource_version,
                    elements,
                })
            }
            _ => return Err(Error::Unknown),
        };
        reader.end()?;
        Ok(value)
    }
    #[cfg(not(any(feature = "wallet", feature = "watchface")))]
    Err(Error::Unknown)
}
/// Encode a validated content object, preserving future provider IDs for readback.
/// # Errors
/// Returns invalid data, missing manifest, unavailable resources or insufficient buffer.
#[cfg(any(
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
pub fn encode_object(v: Object<'_>, out: &mut [u8], m: &Manifest<'_>) -> Result<usize, Error> {
    #[cfg(feature = "weather")]
    if let Object::Weather(value) = v {
        let n = value
            .encode(out, weather_max(m)?)
            .map_err(|_| Error::Invalid)?;
        decode_object_read(0x32, 1, &out[..n], m)?;
        return Ok(n);
    }
    #[cfg(feature = "contacts")]
    if let Object::Contact(value) = v {
        let n = value.encode(out, m)?;
        decode_object_read(0x34, 1, &out[..n], m)?;
        return Ok(n);
    }
    #[cfg(feature = "memo")]
    if let Object::Memo(value) = v {
        let n = value.encode(out, m)?;
        decode_object_read(0x36, 1, &out[..n], m)?;
        return Ok(n);
    }
    #[cfg(feature = "notifications")]
    if let Object::Notification(value) = v {
        let n = value
            .encode(out, usize::from(text_max(m)?))
            .map_err(|_| Error::Invalid)?;
        decode_object_read(0x30, 1, &out[..n], m)?;
        return Ok(n);
    }
    #[cfg(any(feature = "wallet", feature = "watchface"))]
    {
        let mut w = Writer::new(out);
        let (module, kind) = match v {
            #[cfg(feature = "wallet")]
            Object::Wallet(v) => {
                w.u8(v.category)?;
                w.u16(v.provider)?;
                w.text(v.title, text_max(m)?)?;
                w.text(v.content, text_max(m)?)?;
                (0x35, 1)
            }
            #[cfg(feature = "watchface")]
            Object::Watchface(v) => {
                w.u32(v.dial_id)?;
                w.u16(v.group)?;
                w.u8(v.download_state)?;
                w.text(v.resource_version, text_max(m)?)?;
                w.u8(u8::try_from(v.len()).map_err(|_| Error::Capacity)?)?;
                w.bytes(v.elements)?;
                (0x31, 1)
            }
            #[cfg(feature = "notifications")]
            Object::Notification(_) => return Err(Error::State),
            #[cfg(feature = "weather")]
            Object::Weather(_) => return Err(Error::State),
            #[cfg(feature = "contacts")]
            Object::Contact(_) => return Err(Error::State),
            #[cfg(feature = "memo")]
            Object::Memo(_) => return Err(Error::State),
            Object::Hidden(_) => return Err(Error::Unknown),
        };
        let n = w.len();
        decode_object_read(module, kind, &out[..n], m)?;
        Ok(n)
    }
    #[cfg(not(any(feature = "wallet", feature = "watchface")))]
    Err(Error::Unknown)
}

#[cfg(feature = "weather")]
fn weather_max(manifest: &Manifest<'_>) -> Result<u8, Error> {
    u8::try_from(count_max(manifest)?.min(u16::from(u8::MAX))).map_err(|_| Error::Invalid)
}

impl core::fmt::Debug for Manifest<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Manifest")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for WatchfaceManifest<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WatchfaceManifest")
            .field("dial_length", &self.dials.len())
            .field("element_length", &self.elements.len())
            .field("style_length", &self.styles.len())
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "notifications")]
impl core::fmt::Debug for IdentifierIter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IdentifierIter")
            .field("length", &self.remaining)
            .finish_non_exhaustive()
    }
}
