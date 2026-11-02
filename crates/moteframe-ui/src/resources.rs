// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::ConfigError;
use core::num::NonZeroU32;
use moteframe_core::Dimensions;

/// Stable identity of one immutable resource collection and its revision.
/// Change this key whenever content, metrics or indexing rules change. Never
/// reuse a key for different contents during a UI state's lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceKey {
    collection: u32,
    revision: u32,
}
impl ResourceKey {
    /// Reserved identity for an empty resource collection.
    pub const EMPTY: Self = Self {
        collection: 0,
        revision: 0,
    };
    /// Identifies a nonempty collection. Revision zero is a valid first revision.
    #[must_use]
    pub const fn new(collection: NonZeroU32, revision: u32) -> Self {
        Self {
            collection: collection.get(),
            revision,
        }
    }
    /// Collection identifier (zero only for the empty collection).
    #[must_use]
    pub const fn collection(self) -> u32 {
        self.collection
    }
    /// Collection revision.
    #[must_use]
    pub const fn revision(self) -> u32 {
        self.revision
    }
}

/// Validated directory summary, independent of resource format and storage.
/// Entries have nonzero, representable maximum extents. This is a metadata
/// check, not a promise that future reads/decodes succeed. Providers validate
/// actual indexes, formats and color compatibility before creating the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceMetadata {
    maximum: Dimensions,
    entries: u32,
}
impl ResourceMetadata {
    /// Summary of an empty directory.
    pub const EMPTY: Self = Self {
        maximum: Dimensions::new(0, 0),
        entries: 0,
    };
    /// Checks the maximum entry dimensions; an empty directory uses all zeros.
    /// # Errors
    /// Returns `InvalidResources` for inconsistent or unrepresentable geometry.
    pub fn new(width: u32, height: u32, entries: u32) -> Result<Self, ConfigError> {
        let width = u16::try_from(width).map_err(|_| ConfigError::InvalidResources)?;
        let height = u16::try_from(height).map_err(|_| ConfigError::InvalidResources)?;
        if (entries == 0 && (width != 0 || height != 0))
            || (entries != 0 && (width == 0 || height == 0))
        {
            return Err(ConfigError::InvalidResources);
        }
        Ok(Self {
            maximum: Dimensions::new(width, height),
            entries,
        })
    }
    /// Maximum entry geometry, or zero for an empty directory.
    #[must_use]
    pub const fn maximum(self) -> Dimensions {
        self.maximum
    }
    /// Number of entries declared by the provider.
    #[must_use]
    pub const fn entries(self) -> u32 {
        self.entries
    }
}

/// An immutable borrowed directory/provider with validated identity metadata.
///
/// The provider guarantees that this key names this content, including when
/// storage is external. Borrowing does not preload assets into RAM. A reader's
/// temporary window must not outlive its own borrow; report I/O through scene
/// errors. Interior-mutability must not change current-frame contents.
#[derive(Debug)]
pub struct ResourceView<'a, R: ?Sized> {
    key: ResourceKey,
    metadata: ResourceMetadata,
    provider: &'a R,
}
impl<R: ?Sized> Copy for ResourceView<'_, R> {}
impl<R: ?Sized> Clone for ResourceView<'_, R> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<'a, R: ?Sized> ResourceView<'a, R> {
    /// Creates a view after checking empty/nonempty identity consistency.
    /// # Errors
    /// Returns `InvalidResources` when the reserved empty key and metadata disagree.
    pub fn new(
        key: ResourceKey,
        metadata: ResourceMetadata,
        provider: &'a R,
    ) -> Result<Self, ConfigError> {
        if (key == ResourceKey::EMPTY) != (metadata.entries == 0) {
            return Err(ConfigError::InvalidResources);
        }
        Ok(Self {
            key,
            metadata,
            provider,
        })
    }
    /// An empty view; the provider contains no resources consumed by the UI.
    #[must_use]
    pub const fn empty(provider: &'a R) -> Self {
        Self {
            key: ResourceKey::EMPTY,
            metadata: ResourceMetadata::EMPTY,
            provider,
        }
    }
    /// Validated collection key.
    #[must_use]
    pub const fn key(self) -> ResourceKey {
        self.key
    }
    /// Immediately available directory summary.
    #[must_use]
    pub const fn metadata(self) -> ResourceMetadata {
        self.metadata
    }
    /// Borrows the provider for the view lifetime.
    #[must_use]
    pub const fn provider(self) -> &'a R {
        self.provider
    }
}
