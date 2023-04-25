#![no_std]
#![feature(error_in_core)]

extern crate alloc;

use alloc::{borrow::ToOwned, collections::BTreeMap};

use error_stack::{Context, Result};
use time::OffsetDateTime;
use type_system::url::BaseUrl;

use crate::entity::Entity;

pub mod entity;
pub mod types;

#[derive(Debug, Copy, Clone)]
pub struct BaseUrlRef<'a>(&'a str);

impl<'a> BaseUrlRef<'a> {
    #[doc(hidden)] // use the macro instead
    #[must_use]
    pub const fn new_unchecked(url: &'a str) -> Self {
        Self(url)
    }

    // cannot implement ToOwned because this is fallible
    // TODO: compile time fail! ~> const fn validator?
    #[must_use]
    pub fn into_owned(self) -> BaseUrl {
        BaseUrl::new(self.0.to_owned()).expect("invalid Base URL")
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

pub trait Type: Sized {
    type Ref<'a>: TypeRef<Owned = Self>
    where
        Self: 'a;

    const ID: VersionedUrlRef<'static>;

    fn as_ref(&self) -> Self::Ref<'_>;
}

pub trait DataTypeRef<'a>: TypeRef {
    type Error: Context;

    fn try_from_value(value: &'a serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait DataType: Type
where
    for<'a> Self::Ref<'a>: DataTypeRef<'a>,
{
    type Error: Context;

    fn try_from_value(value: serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait PropertyType: Type
where
    for<'a> Self::Ref<'a>: PropertyTypeRef<'a>,
{
    type Error: Context;

    fn try_from_value(value: serde_json::Value) -> Result<Self, Self::Error>;
}

pub trait PropertyTypeRef<'a>: TypeRef {
    type Error: Context;

    fn try_from_value(value: &'a serde_json::Value) -> Result<Self, Self::Error>;
}

// TODO: versions! fn latest(), should be from somewhere else (vertices? -> should be unified)
pub trait EntityType: Type
where
    for<'a> Self::Ref<'a>: EntityTypeRef<'a>,
{
    type Error: Context;

    fn try_from_entity(value: Entity) -> Result<Self, Self::Error>;
}

pub trait EntityTypeRef<'a>: TypeRef {
    type Error: Context;

    fn try_from_entity(value: &'a Entity) -> Result<Self, Self::Error>;
}
