#![no_std]
#![feature(error_in_core)]
#![feature(never_type)]

extern crate alloc;

use alloc::borrow::ToOwned;

use error_stack::{Context, Result};
use serde::Serialize;
pub use type_system::url::{BaseUrl, VersionedUrl};

use crate::entity::{Entity, LinkData};

pub mod entity;
mod error;
mod polyfill;
mod serialize;
pub mod types;

pub use error::{GenericEntityError, GenericPropertyError};
pub use polyfill::{fold_iter_reports, fold_tuple_reports};

#[derive(Debug, Copy, Clone)]
pub struct BaseUrlRef<'a>(&'a str);

impl<'a> BaseUrlRef<'a> {
    #[doc(hidden)] // use the macro instead
    #[must_use]
    pub const fn new_unchecked(url: &'a str) -> Self {
        Self(url)
    }

    // cannot implement ToOwned because `AsRef` is not implemented
    // TODO: compile time fail! ~> const fn validator?
    #[must_use]
    pub fn into_owned(self) -> BaseUrl {
        BaseUrl::new(self.0.to_owned()).expect("invalid Base URL")
    }

    #[must_use]
    pub const fn as_str(&self) -> &str {
        self.0
    }
}

pub struct VersionedUrlRef<'a> {
    base: BaseUrlRef<'a>,
    version: u32,
}

impl<'a> VersionedUrlRef<'a> {
    #[doc(hidden)] // use the macro instead
    #[must_use]
    pub const fn new_unchecked(base: BaseUrlRef<'a>, version: u32) -> Self {
        Self { base, version }
    }

    #[must_use]
    pub const fn base(&self) -> BaseUrlRef<'a> {
        self.base
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub fn into_owned(self) -> VersionedUrl {
        VersionedUrl {
            base_url: self.base.into_owned(),
            version: self.version,
        }
    }
}

impl PartialEq<VersionedUrl> for VersionedUrlRef<'_> {
    fn eq(&self, other: &VersionedUrl) -> bool {
        self.version == other.version && self.base.as_str() == other.base_url.as_str()
    }
}

#[macro_export]
macro_rules! url {
    ($base:literal / v / $version:literal) => {
        $crate::VersionedUrlRef::new_unchecked($crate::BaseUrlRef::new_unchecked($base), $version)
    };
}

pub trait TypeRef: Sized {
    type Owned;

    // called into_owned instead of to_owned to prevent confusion
    fn into_owned(self) -> Self::Owned;
}

pub trait TypeMut: Sized {
    type Owned;

    fn into_owned(self) -> Self::Owned;
}

pub trait Type: Sized {
    type Mut<'a>: TypeMut<Owned = Self>
    where
        Self: 'a;

    type Ref<'a>: TypeRef<Owned = Self>
    where
        Self: 'a;

    const ID: VersionedUrlRef<'static>;

    fn as_mut(&mut self) -> Self::Mut<'_>;

    fn as_ref(&self) -> Self::Ref<'_>;
}

pub trait DataTypeRef<'a>: Serialize + TypeRef {
    type Error: Context;

    fn try_from_value(value: &'a serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait DataTypeMut<'a>: Serialize + TypeMut {
    type Error: Context;

    fn try_from_value(value: &'a mut serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait DataType: Serialize + Type
where
    for<'a> Self::Ref<'a>: DataTypeRef<'a>,
    for<'a> Self::Mut<'a>: DataTypeMut<'a>,
{
    type Error: Context;

    fn try_from_value(value: serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait PropertyTypeRef<'a>: Serialize + TypeRef {
    type Error: Context;

    fn try_from_value(value: &'a serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait PropertyTypeMut<'a>: Serialize + TypeMut {
    type Error: Context;

    fn try_from_value(value: &'a mut serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait PropertyType: Serialize + Type
where
    for<'a> Self::Ref<'a>: PropertyTypeRef<'a>,
    for<'a> Self::Mut<'a>: PropertyTypeMut<'a>,
{
    type Error: Context;

    fn try_from_value(value: serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait EntityTypeRef<'a>: Serialize + TypeRef {
    type Error: Context;

    fn try_from_entity(value: &'a Entity) -> Option<Result<Self, Self::Error>>;
}

pub trait EntityTypeMut<'a>: Serialize + TypeMut {
    type Error: Context;

    fn try_from_entity(value: &'a mut Entity) -> Option<Result<Self, Self::Error>>;
}

pub trait EntityType: Serialize + Type
where
    for<'a> Self::Ref<'a>: EntityTypeRef<'a>,
    for<'a> Self::Mut<'a>: EntityTypeMut<'a>,
{
    type Error: Context;

    fn try_from_entity(value: Entity) -> Option<Result<Self, Self::Error>>;
}

// These might be removed, but act as a marker trait.
//
// These are especially important for functions that only care about properties or links, not the
// type of entity e.g. EntityType, EntityTypeRef and EntityTypeMut are all handled the same.
pub trait EntityProperties {
    type Properties<'a>: Serialize
    where
        Self: 'a;

    fn properties(&self) -> &Self::Properties<'_>;
}

pub trait EntityLink {
    fn link_data(&self) -> &LinkData;
}

// TODO: rename
pub trait OptionalEntityLink {
    fn link_data_opt(&self) -> Option<&LinkData>;
}

impl<T> OptionalEntityLink for T
where
    T: EntityLink,
{
    fn link_data_opt(&self) -> Option<&LinkData> {
        Some(<T as EntityLink>::link_data(self))
    }
}
