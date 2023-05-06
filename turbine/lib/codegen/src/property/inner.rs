use std::collections::HashMap;

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};
use type_system::{url::VersionedUrl, PropertyValues};

use crate::{
    name::{Location, Name, NameResolver},
    property::{
        type_::{Type, TypeGenerator},
        PathSegment, Stack, State,
    },
    shared::Variant,
};

type Path = Box<[PathSegment]>;

struct NameVariants {
    index: usize,
    owned: Ident,
    ref_: Ident,
    mut_: Ident,
}

impl NameVariants {
    fn new(prefix: &str, n: usize) -> Self {
        Self {
            index: n,
            owned: format_ident!("{}{}", prefix, n),
            ref_: format_ident!("{}{}", prefix, n + 1),
            mut_: format_ident!("{}{}", prefix, n + 2),
        }
    }

    fn to_variant(&self, variant: Variant) -> (Ident, usize) {
        match variant {
            Variant::Owned => (self.owned.clone(), self.index),
            Variant::Ref => (self.ref_.clone(), self.index + 1),
            Variant::Mut => (self.mut_.clone(), self.index + 2),
        }
    }
}

pub(super) struct InnerTypes {
    lookup: HashMap<Path, NameVariants>,
    prefix: String,
}

impl InnerTypes {
    pub(super) fn new(locations: &HashMap<&VersionedUrl, Location>) -> Self {
        let mut prefix = "Inner".to_owned();

        for location in locations.values() {
            let name = location
                .alias
                .value
                .as_ref()
                .unwrap_or(&location.name.value);
            let name_ref = location
                .alias
                .value_ref
                .as_ref()
                .unwrap_or(&location.name_ref.value);
            let name_mut = location
                .alias
                .value_mut
                .as_ref()
                .unwrap_or(&location.name_mut.value);

            // ensures that we test if the new identifier is also a collision
            loop {
                if name.starts_with(prefix.as_str())
                    || name_ref.starts_with(prefix.as_str())
                    || name_mut.starts_with(prefix.as_str())
                {
                    prefix = format!("_{prefix}");
                } else {
                    break;
                }
            }
        }

        Self {
            lookup: HashMap::new(),
            prefix,
        }
    }

    fn find(&self, path: &[PathSegment]) -> Option<&NameVariants> {
        self.lookup.get(path)
    }

    fn len(&self) -> usize {
        // every `Names` has 3 items, therefore `* 3`
        self.lookup.len() * 3
    }

    fn get_or_insert(&mut self, path: &[PathSegment]) -> &NameVariants {
        let n = self.len();

        self.lookup
            .entry(path.into())
            .or_insert_with(|| NameVariants::new(&self.prefix, n))
    }
}

pub(super) struct Inner {
    name: Ident,
    stream: TokenStream,
}

impl ToTokens for Inner {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.stream.clone());
    }
}

pub(super) struct InnerGenerator<'a> {
    pub(super) id: &'a VersionedUrl,
    pub(super) variant: Variant,

    pub(super) values: &'a [PropertyValues],

    pub(super) resolver: &'a NameResolver<'a>,
    pub(super) locations: &'a HashMap<&'a VersionedUrl, Location<'a>>,

    pub(super) state: &'a mut State,
}

impl<'a> InnerGenerator<'a> {
    pub(super) fn finish(self) -> Ident {
        let names = self.state.inner.get_or_insert(&self.state.stack);

        let (name, index) = names.to_variant(self.variant);

        self.state.stack.push(PathSegment::Inner { index });

        let Type {
            def,
            impl_ty,
            impl_try_from_value,
            impl_conversion,
        } = TypeGenerator {
            id: self.id,
            name: &name,
            variant: self.variant,
            values: self.values,
            resolver: self.resolver,
            locations: self.locations,
            state: self.state,
        }
        .finish();

        self.state.stack.pop();

        let value_ref = match self.variant {
            Variant::Owned => None,
            Variant::Ref => Some(quote!(&'a)),
            Variant::Mut => Some(quote!(&'a mut)),
        };

        self.state.extra.push(quote!(
            #def

            impl #impl_ty {
                fn try_from_value(value: #value_ref serde_json::Value) -> Result<Self, GenericPropertError> {
                    #impl_try_from_value
                }

                #impl_conversion
            }
        ));

        name
    }
}
