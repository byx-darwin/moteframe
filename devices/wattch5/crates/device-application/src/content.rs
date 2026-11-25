//! Typed fallback for the original sixteen content routes and separate candidate additions.
use device_configuration::{
    Access, Error, ProductCapabilities, SchemaDescriptor, SchemaProvider, SchemaType,
};
/// Trusted concrete content manifest used by the shared configuration/object services.
#[derive(Debug, Clone, Copy)]
pub struct ContentSchemas<'a> {
    manifest: device_content::Manifest<'a>,
}
impl<'a> ContentSchemas<'a> {
    /// Bind actual text/object limits and supported resource dictionaries.
    #[must_use]
    pub const fn new(manifest: device_content::Manifest<'a>) -> Self {
        Self { manifest }
    }
    fn effective(&self, caps: &ProductCapabilities<'_>) -> device_content::Manifest<'a> {
        let mut m = self.manifest;
        if let Some(limits) = caps.limits {
            m.string_max = m.string_max.map(|max| max.min(limits.max_string_bytes));
            m.object_max = m.object_max.map(|max| max.min(limits.max_object_bytes));
        }
        m
    }
    fn descriptor(&self, module: u8, key: u16, object: bool) -> Option<SchemaDescriptor> {
        let d = device_content::descriptor(module, key, object).ok()?;
        Some(SchemaDescriptor {
            module,
            key,
            revision: d.revision,
            schema_type: if object {
                SchemaType::Object
            } else {
                SchemaType::Struct
            },
            access: if module == 8 {
                Access::ReadOnly
            } else {
                Access::ReadWrite
            },
            max_length: if object {
                u64::from(self.manifest.object_max?)
            } else if (module, key) == (0x30, 4) {
                let capacity = self.manifest.property_max?;
                if !(1..=512).contains(&capacity) {
                    return None;
                }
                u64::from(capacity)
            } else {
                512
            },
        })
    }
}
impl SchemaProvider for ContentSchemas<'_> {
    fn property_descriptor(&self, module: u8, key: u16) -> Option<SchemaDescriptor> {
        self.descriptor(module, key, false)
    }
    fn object_descriptor(&self, module: u8, key: u16) -> Option<SchemaDescriptor> {
        self.descriptor(module, key, true)
    }
    #[allow(unused_variables)]
    fn validate_property_value(
        &self,
        module: u8,
        key: u16,
        bytes: &[u8],
        caps: &ProductCapabilities<'_>,
    ) -> Result<(), Error> {
        #[cfg(any(
            feature = "phone",
            feature = "companion-radio",
            feature = "find",
            feature = "notifications",
            feature = "watchface"
        ))]
        {
            device_content::decode_property(module, key, bytes, &self.effective(caps))
                .map(|_| ())
                .map_err(failure)
        }
        #[cfg(not(any(
            feature = "phone",
            feature = "companion-radio",
            feature = "find",
            feature = "notifications",
            feature = "watchface"
        )))]
        {
            Err(Error::Unknown)
        }
    }
    #[allow(unused_variables)]
    fn validate_object_write(
        &self,
        module: u8,
        key: u16,
        bytes: &[u8],
        caps: &ProductCapabilities<'_>,
    ) -> Result<(), Error> {
        #[cfg(any(
            feature = "notifications",
            feature = "watchface",
            feature = "weather",
            feature = "contacts",
            feature = "wallet",
            feature = "memo"
        ))]
        {
            device_content::decode_object(module, key, bytes, &self.effective(caps))
                .map(|_| ())
                .map_err(failure)
        }
        #[cfg(not(any(
            feature = "notifications",
            feature = "watchface",
            feature = "weather",
            feature = "contacts",
            feature = "wallet",
            feature = "memo"
        )))]
        {
            Err(Error::Unknown)
        }
    }
    #[allow(unused_variables)]
    fn validate_object_value(
        &self,
        module: u8,
        key: u16,
        bytes: &[u8],
        caps: &ProductCapabilities<'_>,
    ) -> Result<(), Error> {
        #[cfg(any(
            feature = "notifications",
            feature = "watchface",
            feature = "weather",
            feature = "contacts",
            feature = "wallet",
            feature = "memo"
        ))]
        {
            device_content::decode_object_read(module, key, bytes, &self.effective(caps))
                .map(|_| ())
                .map_err(failure)
        }
        #[cfg(not(any(
            feature = "notifications",
            feature = "watchface",
            feature = "weather",
            feature = "contacts",
            feature = "wallet",
            feature = "memo"
        )))]
        {
            Err(Error::Unknown)
        }
    }
}
fn failure(error: device_content::Error) -> Error {
    match error {
        device_content::Error::Unknown => Error::Unknown,
        device_content::Error::Unavailable => Error::Unavailable,
        device_content::Error::Capacity => Error::Capacity,
        _ => Error::Invalid,
    }
}
