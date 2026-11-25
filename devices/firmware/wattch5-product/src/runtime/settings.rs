//! Actual settings codecs and shared synchronous/asynchronous property owners.
use core::task::Poll;
use device_configuration::{ProductCapabilities, Value, decode_property, encode_property};
use device_property_store::{AsyncPropertyStore, CodecSchemas, MutationToken, PropertyStore};
use device_runtime::{PortError, Work};
use device_services::{PropertyAccess, PropertyBackend, PropertyFailure, PropertyOperation};
use device_storage::{incremental::AsyncMedium, persistence::SnapshotStore};
use wattch5_parity_ui_demo::{Appearance, Field, Locale, TimeFormat};
/// Actual product dictionary; indices match the existing V2 appearance editor.
#[derive(Debug, Clone, Copy)]
pub struct Dictionary<'a> {
    /// Five actual normal backlight levels, in increasing order.
    pub normal_levels: [u16; 5],
    /// Actual selected Chinese and English language identifiers.
    pub languages: [&'a str; 2],
}
impl Dictionary<'_> {
    fn capabilities(&self) -> ProductCapabilities<'_> {
        ProductCapabilities {
            languages: Some(&self.languages),
            normal_levels: Some(&self.normal_levels),
            ..ProductCapabilities::NONE
        }
    }
    fn value(&self, field: Field, value: u8) -> Result<(u16, Value<'_>), PortError> {
        match field {
            Field::Format if value <= 1 => Ok((3, Value::HourFormat(value))),
            Field::Language => self
                .languages
                .get(usize::from(value))
                .map(|s| (7, Value::Text(s)))
                .ok_or(PortError::Unavailable),
            Field::Brightness => self
                .normal_levels
                .get(usize::from(value))
                .map(|n| (10, Value::Brightness(*n)))
                .ok_or(PortError::InvalidInput),
            _ => Err(PortError::Unavailable),
        }
    }
    fn encode(&self, field: Field, value: u8, out: &mut [u8]) -> Result<(u16, usize), PortError> {
        let (key, value) = self.value(field, value)?;
        let n = encode_property(4, key, value, out, &self.capabilities())
            .map_err(|_| PortError::InvalidInput)?;
        Ok((key, n))
    }
    fn validate(&self, adapter: &dyn PropertyBackend) -> Result<(), PortError> {
        if self.normal_levels.iter().any(|n| *n > 1000)
            || !self.normal_levels.windows(2).all(|w| w[0] < w[1])
            || self.languages[0] == self.languages[1]
        {
            return Err(PortError::InvalidInput);
        }
        for (field, count) in [
            (Field::Format, 2),
            (Field::Language, 2),
            (Field::Brightness, 5),
        ] {
            for value in 0..count {
                let mut bytes = [0; 512];
                let (key, n) = self.encode(field, value, &mut bytes)?;
                adapter
                    .validate_value(key, &bytes[..n])
                    .map_err(property_error)?;
            }
        }
        Ok(())
    }
    fn read<'b>(
        &self,
        adapter: &dyn PropertyBackend,
        key: u16,
        out: &'b mut [u8],
        access: &dyn PropertyAccess,
    ) -> Result<Value<'b>, PortError> {
        check_access(access, PropertyOperation::Get, key)?;
        let value = adapter.read(key, out).map_err(property_error)?;
        check_access(access, PropertyOperation::Get, key)?;
        decode_property(4, key, &out[..value.length], &self.capabilities())
            .map_err(|_| PortError::Fault)
    }
    fn load<const B: usize>(
        &self,
        adapter: &dyn PropertyBackend,
        access: &dyn PropertyAccess,
        mut ram: Appearance,
    ) -> Result<Appearance, PortError> {
        let mut bytes = [0; B];
        ram.format = match self.read(adapter, 3, &mut bytes, access)? {
            Value::HourFormat(0) => TimeFormat::Hour24,
            Value::HourFormat(1) => TimeFormat::Hour12,
            _ => return Err(PortError::Fault),
        };
        ram.locale = match self.read(adapter, 7, &mut bytes, access)? {
            Value::Text(s) if s == self.languages[0] => Locale::Zh,
            Value::Text(s) if s == self.languages[1] => Locale::En,
            _ => return Err(PortError::Unavailable),
        };
        ram.brightness = match self.read(adapter, 10, &mut bytes, access)? {
            Value::Brightness(n) => u8::try_from(
                self.normal_levels
                    .iter()
                    .position(|v| *v == n)
                    .ok_or(PortError::Unavailable)?,
            )
            .map_err(|_| PortError::InvalidInput)?,
            _ => return Err(PortError::Fault),
        };
        Ok(ram)
    }
}
/// Appearance bridge borrowing the same durable owner as CONTROL property handling.
#[derive(Debug)]
pub struct Settings<'a, 'c, S: SnapshotStore, const N: usize> {
    pub(crate) owner: &'a PropertyStore<'c, S, CodecSchemas<'c>, N>,
    dictionary: Dictionary<'c>,
}
impl<'a, 'c, S: SnapshotStore, const N: usize> Settings<'a, 'c, S, N> {
    /// Bind a ready owner and validate dictionary values against its actual schemas.
    /// # Errors
    /// Invalid dictionaries, mismatched selected facts and unready owners fail.
    pub fn new(
        owner: &'a PropertyStore<'c, S, CodecSchemas<'c>, N>,
        dictionary: Dictionary<'c>,
    ) -> Result<Self, PortError> {
        dictionary.validate(&owner.adapter(4).map_err(|_| PortError::Unavailable)?)?;
        Ok(Self { owner, dictionary })
    }
    /// Read confirmed fields, retaining explicit caller RAM-only appearance fields.
    /// # Errors
    /// Authority, storage, unsupported readback and recovery failures are explicit.
    pub fn load(
        &self,
        access: &dyn PropertyAccess,
        ram: Appearance,
    ) -> Result<Appearance, PortError> {
        let adapter = self.owner.adapter(4).map_err(|_| PortError::Unavailable)?;
        // Every validated value is inside this owner's N-byte snapshot image.
        // Larger owners retain the protocol's existing 512-byte read ceiling.
        if N < 512 {
            self.dictionary.load::<N>(&adapter, access, ram)
        } else {
            self.dictionary.load::<512>(&adapter, access, ram)
        }
    }
    /// Commit an actual schema field before publishing the staged UI model.
    /// # Errors
    /// Unsupported fields, authority and failed/uncertain durability are explicit.
    pub fn commit(
        &self,
        field: Field,
        value: u8,
        access: &dyn PropertyAccess,
    ) -> Result<(), PortError> {
        let mut bytes = [0; 512];
        let (key, n) = self.dictionary.encode(field, value, &mut bytes)?;
        check_access(access, PropertyOperation::Set, key)?;
        self.owner
            .adapter(4)
            .map_err(|_| PortError::Unavailable)?
            .commit_atomic(4, key, 0, &bytes[..n], access)
            .map_err(property_error)?;
        check_access(access, PropertyOperation::Set, key)
    }
}
#[derive(Debug)]
struct Save {
    token: Option<MutationToken>,
    key: u16,
}
/// Local async settings controller borrowing the same boot owner as BLE.
/// Drop detaches delivery and never cancels an accepted durable operation.
#[derive(Debug)]
pub struct AsyncSettings<'a, 'c, M: AsyncMedium, const N: usize> {
    owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>,
    dictionary: Dictionary<'c>,
    save: Option<Save>,
}
impl<'a, 'c, M: AsyncMedium, const N: usize> AsyncSettings<'a, 'c, M, N> {
    /// Validate a ready owner and actual selected dictionary.
    /// # Errors
    /// Unready storage and incompatible dictionary/schema are rejected.
    pub fn new(
        owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>,
        dictionary: Dictionary<'c>,
    ) -> Result<Self, PortError> {
        dictionary.validate(&owner.adapter(4).map_err(|_| PortError::Unavailable)?)?;
        Ok(Self {
            owner,
            dictionary,
            save: None,
        })
    }
    /// Read the confirmed image while ordinary asynchronous work remains pending.
    /// # Errors
    /// Missing authority, poisoned storage and invalid readback are explicit.
    pub fn load(
        &self,
        access: &dyn PropertyAccess,
        ram: Appearance,
    ) -> Result<Appearance, PortError> {
        let adapter = self.owner.adapter(4).map_err(|_| PortError::Unavailable)?;
        // Every validated value is inside this owner's N-byte snapshot image.
        // Larger owners retain the protocol's existing 512-byte read ceiling.
        if N < 512 {
            self.dictionary.load::<N>(&adapter, access, ram)
        } else {
            self.dictionary.load::<512>(&adapter, access, ram)
        }
    }
    /// Accept one settings save without medium I/O; this is not a success receipt.
    /// # Errors
    /// Busy maps to Fault because `PortError` has no Busy variant. Wire statuses are unchanged.
    /// Invalid values, denied access and actual storage failures propagate.
    pub fn begin(
        &mut self,
        field: Field,
        value: u8,
        access: &dyn PropertyAccess,
    ) -> Result<(), PortError> {
        if self.save.is_some() {
            return Err(PortError::Fault);
        }
        let mut bytes = [0; 512];
        let (key, n) = self.dictionary.encode(field, value, &mut bytes)?;
        check_access(access, PropertyOperation::Set, key)?;
        let token = self
            .owner
            .begin_set(4, key, 0, &bytes[..n], access)
            .map_err(property_error)?;
        self.save = Some(Save {
            token: Some(token),
            key,
        });
        Ok(())
    }
    /// Inspect durability without progressing the medium; successful delivery remains pending
    /// until `finish` after the UI has validated and published its confirmed readback.
    /// # Errors
    /// Scope revocation detaches first. Token or storage failures never become success.
    pub fn poll(&mut self, access: &dyn PropertyAccess) -> Result<bool, PortError> {
        let Some(save) = self.save.as_ref() else {
            return Ok(true);
        };
        if let Err(error) = check_access(access, PropertyOperation::Set, save.key) {
            self.detach();
            return Err(error);
        }
        let Some(token) = save.token else {
            return Ok(true);
        };
        match self.owner.result(token).map_err(|_| PortError::Fault)? {
            Poll::Pending => Ok(false),
            Poll::Ready(_) => {
                let result = self
                    .owner
                    .take_result(token)
                    .map_err(|_| PortError::Fault)?;
                if let Some(save) = self.save.as_mut() {
                    save.token = None;
                }
                match result {
                    Ok(receipt) if receipt.change.is_some() => Ok(true),
                    Ok(_) => {
                        self.save = None;
                        Err(PortError::Fault)
                    }
                    Err(error) => {
                        self.save = None;
                        Err(property_error(error))
                    }
                }
            }
        }
    }
    /// Whether this controller still owns staged save/publication state.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.save.is_some()
    }
    /// Release successful local delivery after confirmed appearance publication.
    /// # Errors
    /// Active storage cannot be acknowledged early; callers must first observe poll success.
    pub fn finish(&mut self) -> Result<(), PortError> {
        if self.save.as_ref().is_some_and(|s| s.token.is_some()) {
            return Err(PortError::Fault);
        }
        self.save = None;
        Ok(())
    }
    /// Suppress local publication; accepted owner work continues independently.
    pub fn detach(&mut self) {
        if let Some(token) = self.save.take().and_then(|s| s.token) {
            let _ = self.owner.detach(token);
        }
    }
}
impl<M: AsyncMedium, const N: usize> Drop for AsyncSettings<'_, '_, M, N> {
    fn drop(&mut self) {
        self.detach();
    }
}
/// One independent bounded background step over the boot-owned settings worker.
#[derive(Debug)]
pub struct SettingsWork<'a, 'c, M: AsyncMedium, const N: usize> {
    owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>,
}
impl<'a, 'c, M: AsyncMedium, const N: usize> SettingsWork<'a, 'c, M, N> {
    /// Borrow the same owner used by UI and retained BLE handlers.
    pub const fn new(owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>) -> Self {
        Self { owner }
    }
}
impl<M: AsyncMedium, const N: usize> Work for SettingsWork<'_, '_, M, N> {
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        self.owner.step().map_err(|error| match error {
            device_property_store::Error::Storage(_) | device_property_store::Error::Integrity => {
                PortError::Uncertain
            }
            _ => PortError::Fault,
        })
    }
}
fn check_access(
    access: &dyn PropertyAccess,
    operation: PropertyOperation,
    key: u16,
) -> Result<(), PortError> {
    if access.allowed(4, operation, None) && access.allowed(4, operation, Some(key)) {
        Ok(())
    } else {
        Err(PortError::Unavailable)
    }
}
pub(crate) fn property_error(error: PropertyFailure) -> PortError {
    match error {
        PropertyFailure::AccessDenied | PropertyFailure::NotFound => PortError::Unavailable,
        PropertyFailure::InvalidValue | PropertyFailure::LimitExceeded => PortError::InvalidInput,
        PropertyFailure::StorageFailure => PortError::Uncertain,
        _ => PortError::Fault,
    }
}
