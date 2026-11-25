use crate::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Property,
    Object,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub module: Option<u8>,
    pub key: u16,
    pub kind: EntryKind,
    pub owner: &'static str,
    pub feature: &'static str,
}
/// All62 V0.5 properties and15 objects. New candidate endpoints are listed separately in README.
/// A route records ownership; query the compiled descriptor to establish a validator exists.
pub fn catalog() -> impl Iterator<Item = CatalogEntry> {
    [
        CatalogEntry {
            id: "01.P0001",
            module: Some(0x01),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "time",
        },
        CatalogEntry {
            id: "03.P0001",
            module: Some(0x03),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-models",
            feature: "profile",
        },
        CatalogEntry {
            id: "03.P0002",
            module: Some(0x03),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-models",
            feature: "profile",
        },
        CatalogEntry {
            id: "20.P0001",
            module: Some(0x20),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "20.P0002",
            module: Some(0x20),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "20.P0003",
            module: Some(0x20),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "21.P0001",
            module: Some(0x21),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-motion/device-models",
            feature: "sleep",
        },
        CatalogEntry {
            id: "21.P0002",
            module: Some(0x21),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-motion/device-models",
            feature: "sleep",
        },
        CatalogEntry {
            id: "22.P0001",
            module: Some(0x22),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-motion/device-configuration",
            feature: "sport",
        },
        CatalogEntry {
            id: "22.P0002",
            module: Some(0x22),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-motion/device-configuration",
            feature: "sport",
        },
        CatalogEntry {
            id: "23.P0001",
            module: Some(0x23),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-motion/device-configuration",
            feature: "activity",
        },
        CatalogEntry {
            id: "25.P0002",
            module: Some(0x25),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "25.P0003",
            module: Some(0x25),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "26.P0001",
            module: Some(0x26),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "26.P0002",
            module: Some(0x26),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "27.P0001",
            module: Some(0x27),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "28.P0001",
            module: Some(0x28),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "28.P0002",
            module: Some(0x28),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "28.P0003",
            module: Some(0x28),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "2A.P0001",
            module: Some(0x2A),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "2A.P0002",
            module: Some(0x2A),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "2A.P0003",
            module: Some(0x2A),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "health-policy",
        },
        CatalogEntry {
            id: "2D.P0001",
            module: Some(0x2D),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-wellness",
            feature: "womens-health",
        },
        CatalogEntry {
            id: "2E.P0001",
            module: Some(0x2E),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-wellness",
            feature: "uv",
        },
        CatalogEntry {
            id: "通用.PF000",
            module: None,
            key: 0xf000,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "limits",
        },
        CatalogEntry {
            id: "02.P0001",
            module: Some(0x02),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "02.P0002",
            module: Some(0x02),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "02.P0003",
            module: Some(0x02),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "02.P0004",
            module: Some(0x02),
            key: 0x0004,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "02.P0005",
            module: Some(0x02),
            key: 0x0005,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "02.P0006",
            module: Some(0x02),
            key: 0x0006,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "02.P0007",
            module: Some(0x02),
            key: 0x0007,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "device",
        },
        CatalogEntry {
            id: "04.P0001",
            module: Some(0x04),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0002",
            module: Some(0x04),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0003",
            module: Some(0x04),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0004",
            module: Some(0x04),
            key: 0x0004,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0005",
            module: Some(0x04),
            key: 0x0005,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0006",
            module: Some(0x04),
            key: 0x0006,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0007",
            module: Some(0x04),
            key: 0x0007,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0008",
            module: Some(0x04),
            key: 0x0008,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P0009",
            module: Some(0x04),
            key: 0x0009,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.P000A",
            module: Some(0x04),
            key: 0x000a,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "04.O0001",
            module: Some(0x04),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "device-configuration",
            feature: "settings",
        },
        CatalogEntry {
            id: "05.P0001",
            module: Some(0x05),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "05.O0001",
            module: Some(0x05),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "05.O0002",
            module: Some(0x05),
            key: 0x0002,
            kind: EntryKind::Object,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "05.O0003",
            module: Some(0x05),
            key: 0x0003,
            kind: EntryKind::Object,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "05.O0004",
            module: Some(0x05),
            key: 0x0004,
            kind: EntryKind::Object,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "05.O0005",
            module: Some(0x05),
            key: 0x0005,
            kind: EntryKind::Object,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "05.O0006",
            module: Some(0x05),
            key: 0x0006,
            kind: EntryKind::Object,
            owner: "device-models",
            feature: "reminders",
        },
        CatalogEntry {
            id: "06.P0001",
            module: Some(0x06),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "find",
        },
        CatalogEntry {
            id: "07.P0001",
            module: Some(0x07),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "07.P0002",
            module: Some(0x07),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "07.P0003",
            module: Some(0x07),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "07.P0004",
            module: Some(0x07),
            key: 0x0004,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "07.P0005",
            module: Some(0x07),
            key: 0x0005,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "08.P0001",
            module: Some(0x08),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "09.P0001",
            module: Some(0x09),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "telemetry",
        },
        CatalogEntry {
            id: "11.P0001",
            module: Some(0x11),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "extension",
        },
        CatalogEntry {
            id: "30.O0001",
            module: Some(0x30),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "30.P0001",
            module: Some(0x30),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "30.P0002",
            module: Some(0x30),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "30.P0003",
            module: Some(0x30),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "31.P0001",
            module: Some(0x31),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "31.O0001",
            module: Some(0x31),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "32.O0001",
            module: Some(0x32),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "34.O0001",
            module: Some(0x34),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "35.O0001",
            module: Some(0x35),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "36.O0001",
            module: Some(0x36),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "external SchemaProvider / device-content",
            feature: "external-content",
        },
        CatalogEntry {
            id: "37.O0001",
            module: Some(0x37),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "device-configuration",
            feature: "hid",
        },
        CatalogEntry {
            id: "38.P0001",
            module: Some(0x38),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "antplus",
        },
        CatalogEntry {
            id: "39.P0001",
            module: Some(0x39),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "radiation",
        },
        CatalogEntry {
            id: "72.P0001",
            module: Some(0x72),
            key: 0x0001,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "engineering",
        },
        CatalogEntry {
            id: "72.P0002",
            module: Some(0x72),
            key: 0x0002,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "engineering",
        },
        CatalogEntry {
            id: "72.P0003",
            module: Some(0x72),
            key: 0x0003,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "engineering",
        },
        CatalogEntry {
            id: "72.P0004",
            module: Some(0x72),
            key: 0x0004,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "engineering",
        },
        CatalogEntry {
            id: "72.O待冻结",
            module: Some(0x72),
            key: 0x0001,
            kind: EntryKind::Object,
            owner: "device-configuration",
            feature: "engineering",
        },
    ]
    .into_iter()
}
/// Additional candidate routes, separate from the frozen62 properties and15 objects.
/// Presence declares ownership only; an actual compiled provider must validate each value.
pub fn candidate_catalog() -> impl Iterator<Item = CatalogEntry> {
    [
        CatalogEntry {
            id: "25.P00F0",
            module: Some(0x25),
            key: 0x00f0,
            kind: EntryKind::Property,
            owner: "device-configuration",
            feature: "blood-pressure",
        },
        CatalogEntry {
            id: "30.P0004",
            module: Some(0x30),
            key: 0x0004,
            kind: EntryKind::Property,
            owner: "device-content",
            feature: "external-content",
        },
    ]
    .into_iter()
}
pub trait SchemaProvider {
    fn property_descriptor(&self, _: u8, _: u16) -> Option<SchemaDescriptor> {
        None
    }
    fn object_descriptor(&self, _: u8, _: u16) -> Option<SchemaDescriptor> {
        None
    }
    fn validate_property_value(
        &self,
        _: u8,
        _: u16,
        _: &[u8],
        _: &ProductCapabilities<'_>,
    ) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    fn validate_object_value(
        &self,
        _: u8,
        _: u16,
        _: &[u8],
        _: &ProductCapabilities<'_>,
    ) -> Result<(), Error> {
        Err(Error::Unsupported)
    }
    /// Validate a new object write, which may be stricter than preserving future readback values.
    /// Default delegates to the existing strict validator; missing providers still reject.
    fn validate_object_write(
        &self,
        module: u8,
        key: u16,
        bytes: &[u8],
        caps: &ProductCapabilities<'_>,
    ) -> Result<(), Error> {
        self.validate_object_value(module, key, bytes, caps)
    }
}
fn provider_route(module: u8, key: u16, kind: EntryKind) -> bool {
    cfg!(feature = "external-content")
        && catalog().chain(candidate_catalog()).any(|e| {
            e.module == Some(module)
                && e.key == key
                && e.kind == kind
                && e.feature == "external-content"
        })
}
fn checked_descriptor(
    desc: SchemaDescriptor,
    module: u8,
    key: u16,
) -> Result<SchemaDescriptor, Error> {
    if desc.module != module || desc.key != key || desc.revision == 0 {
        Err(Error::Invalid)
    } else {
        Ok(desc)
    }
}
/// Explicit provider fallback only for selected external-content routes; built-in failures never fall through.
pub fn validate_property_with_provider(
    module: u8,
    key: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
    provider: &dyn SchemaProvider,
) -> Result<SchemaDescriptor, Error> {
    match property_descriptor(module, key) {
        Ok(_) => return validate_property(module, key, bytes, caps),
        Err(Error::Unknown) => {}
        Err(e) => return Err(e),
    }
    if !provider_route(module, key, EntryKind::Property) {
        return Err(Error::Unknown);
    }
    let d = checked_descriptor(
        provider
            .property_descriptor(module, key)
            .ok_or(Error::Unsupported)?,
        module,
        key,
    )?;
    if d.access == Access::ReadOnly {
        return Err(Error::Readonly);
    }
    if bytes.len() as u64 > d.max_length {
        return Err(Error::Invalid);
    }
    provider.validate_property_value(module, key, bytes, caps)?;
    Ok(d)
}
/// Readback/provider validation, including read-only content DTOs.
pub fn validate_property_value_with_provider(
    module: u8,
    key: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
    provider: &dyn SchemaProvider,
) -> Result<SchemaDescriptor, Error> {
    match property_descriptor(module, key) {
        Ok(_) => return validate_property_value(module, key, bytes, caps),
        Err(Error::Unknown) => {}
        Err(e) => return Err(e),
    }
    if !provider_route(module, key, EntryKind::Property) {
        return Err(Error::Unknown);
    }
    let d = checked_descriptor(
        provider
            .property_descriptor(module, key)
            .ok_or(Error::Unsupported)?,
        module,
        key,
    )?;
    if bytes.len() as u64 > d.max_length {
        return Err(Error::Invalid);
    }
    provider.validate_property_value(module, key, bytes, caps)?;
    Ok(d)
}
pub fn validate_object_with_provider(
    module: u8,
    key: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
    provider: &dyn SchemaProvider,
) -> Result<SchemaDescriptor, Error> {
    match object_descriptor(module, key) {
        Ok(_) => return validate_object(module, key, bytes, caps),
        Err(Error::Unknown) => {}
        Err(e) => return Err(e),
    }
    if !provider_route(module, key, EntryKind::Object) {
        return Err(Error::Unknown);
    }
    let d = checked_descriptor(
        provider
            .object_descriptor(module, key)
            .ok_or(Error::Unsupported)?,
        module,
        key,
    )?;
    if d.access == Access::ReadOnly {
        return Err(Error::Readonly);
    }
    if bytes.len() as u64 > d.max_length {
        return Err(Error::Invalid);
    }
    if let Some(limits) = caps.limits {
        if bytes.len() as u64 > u64::from(limits.max_object_bytes) {
            return Err(Error::Invalid);
        }
        if limits.max_objects == 0 {
            return Err(Error::Unsupported);
        }
    }
    provider.validate_object_write(module, key, bytes, caps)?;
    Ok(d)
}
pub fn validate_object_value_with_provider(
    module: u8,
    key: u16,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
    provider: &dyn SchemaProvider,
) -> Result<SchemaDescriptor, Error> {
    match object_descriptor(module, key) {
        Ok(_) => return validate_object_value(module, key, bytes, caps),
        Err(Error::Unknown) => {}
        Err(e) => return Err(e),
    }
    if !provider_route(module, key, EntryKind::Object) {
        return Err(Error::Unknown);
    }
    let d = checked_descriptor(
        provider
            .object_descriptor(module, key)
            .ok_or(Error::Unsupported)?,
        module,
        key,
    )?;
    if bytes.len() as u64 > d.max_length {
        return Err(Error::Invalid);
    }
    if let Some(limits) = caps.limits {
        if bytes.len() as u64 > u64::from(limits.max_object_bytes) {
            return Err(Error::Invalid);
        }
        if limits.max_objects == 0 {
            return Err(Error::Unsupported);
        }
    }
    provider.validate_object_value(module, key, bytes, caps)?;
    Ok(d)
}
