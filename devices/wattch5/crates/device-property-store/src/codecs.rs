//! Actual optional lower-level validators; no application or platform dependency.
use crate::{Error, PropertySchema, excluded};
use device_configuration::{Access, ProductCapabilities};
use device_services::PropertyDescriptor;
/// Immutable actual capability facts for one selected module.
#[derive(Debug, Clone, Copy)]
pub struct ModuleCapabilities<'a> {
    /// Selected module number.
    pub module: u8,
    /// Actual per-module limits, ranges and supported dictionaries.
    pub capabilities: ProductCapabilities<'a>,
}
/// Direct bridge to actual feature-selected configuration and content codecs.
#[derive(Debug, Clone, Copy)]
pub struct CodecSchemas<'a> {
    modules: &'a [ModuleCapabilities<'a>],
    #[cfg(any(
        feature = "find",
        feature = "phone",
        feature = "companion-radio",
        feature = "notifications",
        feature = "watchface"
    ))]
    content: Option<device_content::Manifest<'a>>,
}
impl<'a> CodecSchemas<'a> {
    /// Bind explicitly selected per-module facts. No capability is inferred.
    #[must_use]
    pub const fn new(modules: &'a [ModuleCapabilities<'a>]) -> Self {
        Self {
            modules,
            #[cfg(any(
                feature = "find",
                feature = "phone",
                feature = "companion-radio",
                feature = "notifications",
                feature = "watchface"
            ))]
            content: None,
        }
    }
    /// Bind actual content limits, find delay and packaged watchface dictionaries.
    #[cfg(any(
        feature = "find",
        feature = "phone",
        feature = "companion-radio",
        feature = "notifications",
        feature = "watchface"
    ))]
    #[must_use]
    pub const fn with_content(mut self, manifest: device_content::Manifest<'a>) -> Self {
        self.content = Some(manifest);
        self
    }
    fn capabilities(&self, module: u8) -> Result<&ProductCapabilities<'a>, Error> {
        if !self.modules.windows(2).all(|m| m[0].module < m[1].module) {
            return Err(Error::InvalidCatalogue);
        }
        self.modules
            .iter()
            .find(|m| m.module == module)
            .map(|m| &m.capabilities)
            .ok_or(Error::Unavailable)
    }
    #[cfg(any(
        feature = "find",
        feature = "phone",
        feature = "companion-radio",
        feature = "notifications",
        feature = "watchface"
    ))]
    fn manifest(
        &self,
        caps: &ProductCapabilities<'_>,
    ) -> Result<device_content::Manifest<'a>, Error> {
        let mut manifest = self.content.ok_or(Error::Unavailable)?;
        if let Some(limits) = caps.limits {
            manifest.string_max = manifest
                .string_max
                .map(|max| max.min(limits.max_string_bytes));
            manifest.object_max = manifest
                .object_max
                .map(|max| max.min(limits.max_object_bytes));
        }
        Ok(manifest)
    }
    #[cfg(any(
        feature = "phone",
        feature = "companion-radio",
        feature = "notifications",
        feature = "watchface"
    ))]
    fn content_descriptor(
        &self,
        module: u8,
        key: u16,
        caps: &ProductCapabilities<'_>,
    ) -> Result<PropertyDescriptor, Error> {
        let actual = device_content::descriptor(module, key, false).map_err(content_error)?;
        let manifest = self.manifest(caps)?;
        let value_max = match (module, key) {
            (0x30, 2) => 1,
            (0x31, 1) => 4,
            (0x30, 4) => match manifest.property_max {
                Some(n @ 1..=512) => n,
                _ => return Err(Error::Unavailable),
            },
            _ => 512,
        };
        Ok(PropertyDescriptor {
            key,
            access: 11,
            schema: actual.revision,
            value_max,
        })
    }
}
impl PropertySchema for CodecSchemas<'_> {
    fn descriptor(&self, module: u8, key: u16) -> Result<PropertyDescriptor, Error> {
        if excluded(module, key) {
            return Err(Error::Unavailable);
        }
        let caps = self.capabilities(module)?;
        match device_configuration::property_descriptor(module, key) {
            Ok(d) => {
                if d.access != Access::ReadWrite {
                    return Err(Error::Unavailable);
                }
                #[cfg(feature = "find")]
                if (module, key) == (6, 1) {
                    self.manifest(caps)?
                        .find_delay_s
                        .ok_or(Error::Unavailable)?;
                }
                let _ = caps;
                Ok(PropertyDescriptor {
                    key,
                    access: 11,
                    schema: d.revision,
                    value_max: u16::try_from(d.max_length.min(512))
                        .map_err(|_| Error::InvalidCatalogue)?,
                })
            }
            Err(device_configuration::Error::Unknown) => {
                #[cfg(any(
                    feature = "phone",
                    feature = "companion-radio",
                    feature = "notifications",
                    feature = "watchface"
                ))]
                {
                    self.content_descriptor(module, key, caps)
                }
                #[cfg(not(any(
                    feature = "phone",
                    feature = "companion-radio",
                    feature = "notifications",
                    feature = "watchface"
                )))]
                {
                    Err(Error::Unavailable)
                }
            }
            Err(error) => Err(configuration_error(error)),
        }
    }
    fn validate(&self, module: u8, key: u16, value: &[u8]) -> Result<(), Error> {
        let descriptor = self.descriptor(module, key)?;
        if value.len() > usize::from(descriptor.value_max) {
            return Err(Error::InvalidValue);
        }
        let caps = self.capabilities(module)?;
        match device_configuration::property_descriptor(module, key) {
            Ok(_) => {
                device_configuration::validate_property(module, key, value, caps)
                    .map_err(configuration_error)?;
                #[cfg(feature = "find")]
                if (module, key) == (6, 1) {
                    device_content::decode_property(module, key, value, &self.manifest(caps)?)
                        .map_err(content_error)?;
                }
                Ok(())
            }
            Err(device_configuration::Error::Unknown) => {
                #[cfg(any(
                    feature = "phone",
                    feature = "companion-radio",
                    feature = "notifications",
                    feature = "watchface"
                ))]
                {
                    device_content::decode_property(module, key, value, &self.manifest(caps)?)
                        .map(|_| ())
                        .map_err(content_error)
                }
                #[cfg(not(any(
                    feature = "phone",
                    feature = "companion-radio",
                    feature = "notifications",
                    feature = "watchface"
                )))]
                {
                    Err(Error::Unavailable)
                }
            }
            Err(error) => Err(configuration_error(error)),
        }
    }
}
fn configuration_error(error: device_configuration::Error) -> Error {
    match error {
        device_configuration::Error::Unknown
        | device_configuration::Error::Unavailable
        | device_configuration::Error::Readonly => Error::Unavailable,
        _ => Error::InvalidValue,
    }
}
#[cfg(any(
    feature = "find",
    feature = "phone",
    feature = "companion-radio",
    feature = "notifications",
    feature = "watchface"
))]
fn content_error(error: device_content::Error) -> Error {
    match error {
        device_content::Error::Unknown | device_content::Error::Unavailable => Error::Unavailable,
        _ => Error::InvalidValue,
    }
}
