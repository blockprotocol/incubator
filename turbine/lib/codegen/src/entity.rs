use std::{
    collections::{BTreeMap, HashMap},
    ops::Deref,
    str::FromStr,
};

use once_cell::sync::Lazy;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use type_system::{
    url::{BaseUrl, VersionedUrl},
    EntityType, EntityTypeReference,
};

use crate::{
    name::{Location, NameResolver, PropertyName},
    shared,
    shared::{generate_mod, imports, Property, PropertyKind},
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Variant {
    Owned,
    Ref,
    Mut,
}

const RESERVED: &[&str] = &[
    "Type",
    "TypeRef",
    "EntityType",
    "EntityTypeRef",
    "VersionedUrlRef",
    "GenericEntityError",
    "Entity",
    "LinkData",
    "Serialize",
    "Properties",
    "PropertiesRef",
    "PropertiesMut",
    "Report",
    "HashMap",
];

static LINK_REF: Lazy<EntityTypeReference> = Lazy::new(|| {
    EntityTypeReference::new(
        VersionedUrl::from_str(
            "https://blockprotocol.org/@blockprotocol/types/entity-type/link/v/1",
        )
        .expect("should be valid url"),
    )
});

#[derive(Debug, Copy, Clone)]
struct Import {
    vec: bool,
    box_: bool,
}

struct State {
    is_link: bool,
    import: Import,
}

fn properties<'a>(
    entity: &'a EntityType,
    resolver: &NameResolver,
    property_names: &HashMap<&VersionedUrl, PropertyName>,
    locations: &HashMap<&VersionedUrl, Location>,
) -> BTreeMap<&'a BaseUrl, Property> {
    shared::properties(
        entity.id(),
        entity.properties(),
        entity.required(),
        resolver,
        property_names,
        locations,
    )
}

// TODO: adapt code from properties
fn generate_use(
    references: &[&VersionedUrl],
    locations: &HashMap<&VersionedUrl, Location>,
    state: State,
) -> TokenStream {
    let mut imports: Vec<_> = imports(references, locations).collect();

    if state.is_link {
        imports.push(quote!(
            use blockprotocol::entity::LinkData;
        ));
    }

    if state.import.box_ {
        imports.push(quote!(
            use alloc::boxed::Box;
        ));
    }

    if state.import.vec {
        imports.push(quote!(
            use alloc::vec::Vec;
        ));
    }

    quote! {
        use serde::Serialize;
        use blockprotocol::{Type, TypeRef, TypeMut};
        use blockprotocol::{EntityType, EntityTypeRef, EntityTypeMut};
        use blockprotocol::PropertyType as _;
        use blockprotocol::{VersionedUrlRef, GenericEntityError};
        use blockprotocol::entity::Entity;
        use error_stack::{Result, Report};
        use hashbrown::HashMap;

        #(#imports)*
    }
}

fn generate_type(
    variant: Variant,
    location: &Location,
    properties: &BTreeMap<&BaseUrl, Property>,
    state: &mut State,
) -> TokenStream {
    let lifetime = matches!(variant, Variant::Ref | Variant::Mut).then(|| quote!(<'a>));

    let name = match variant {
        Variant::Owned => &location.name,
        Variant::Ref => &location.name_ref,
        Variant::Mut => &location.name_mut,
    };

    let alias = name.alias.as_ref().map(|alias| {
        let alias = Ident::new(alias, Span::call_site());
        let name = Ident::new(&name.value, Span::call_site());

        quote!(pub type #alias #lifetime = #name #lifetime;)
    });

    let name = Ident::new(&name.value, Span::call_site());

    let properties_name = match variant {
        Variant::Owned => Ident::new("Properties", Span::call_site()),
        Variant::Ref => Ident::new("PropertiesRef", Span::call_site()),
        Variant::Mut => Ident::new("PropertiesMut", Span::call_site()),
    };

    let reference = match variant {
        Variant::Owned => None,
        Variant::Ref => Some(quote!(&'a)),
        Variant::Mut => Some(quote!(&'a mut)),
    };

    let mut fields = vec![quote!(pub properties: #properties_name #lifetime)];

    if state.is_link {
        fields.push(quote!(pub link_data: #reference LinkData));
    }

    let properties = properties.iter().map(|(base, property)| {
        let url = base.as_str();
        let Property {
            name,
            type_,
            kind,
            required,
        } = property;

        let type_ = match variant {
            Variant::Owned => type_.to_token_stream(),
            Variant::Ref => quote!(#type_::Ref<'a>),
            Variant::Mut => quote!(#type_::Mut<'a>),
        };

        let mut type_ = match kind {
            PropertyKind::Array if variant == Variant::Owned || variant == Variant::Mut => {
                state.import.vec = true;
                quote!(Vec<#type_>)
            }
            PropertyKind::Array => {
                state.import.box_ = true;
                quote!(Box<[#type_]>)
            }
            PropertyKind::Plain => type_.to_token_stream(),
            PropertyKind::Boxed => {
                state.import.box_ = true;
                quote!(Box<#type_>)
            }
        };

        if !required {
            type_ = quote!(Option<#type_>);
        }

        quote! {
            #[serde(rename = #url)]
            pub #name: #type_
        }
    });

    quote! {
        #[derive(Debug, Clone, Serialize)]
        pub struct #properties_name #lifetime {
            #(#properties),*
        }

        impl #properties_name #lifetime {
            fn try_from_value(properties: #reference HashMap<BaseUrl, Value>) -> Result<Self, GenericEntityError> {
                // TODO: factor out
                todo!()
                // #(#property_try_from;)*
                //
                // #(#properties_fold;)*
                //
                // Ok(Self {
                //     #(#properties_self),*
                // })
            }
        }

        #[derive(Debug, Clone, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #name #lifetime {
            #(#fields),*
        }

        // TODO: factor out try_from_entity!

        #alias
    }
}

#[allow(clippy::too_many_lines)]
fn generate_owned(
    entity: &EntityType,
    location: &Location,
    properties: &BTreeMap<&BaseUrl, Property>,
    state: &mut State,
) -> TokenStream {
    let name = Ident::new(&location.name.value, Span::call_site());
    let name_ref = Ident::new(&location.name_ref.value, Span::call_site());
    let name_mut = Ident::new(&location.name_mut.value, Span::call_site());

    let base_url = entity.id().base_url.as_str();
    let version = entity.id().version;

    let def = generate_type(Variant::Owned, location, properties, state);

    quote! {
        #def

        impl Type for #name {
            type Mut<'a> = #name_mut<'a> where Self: 'a;
            type Ref<'a> = #name_ref<'a> where Self: 'a;

            const ID: VersionedUrlRef<'static>  = url!(#base_url / v / #version);

            fn as_ref(&self) -> Self::Ref<'_> {
                // TODO!
                todo!()
            }

            fn as_mut(&self) -> Self::Mut<'_> {
                // TODO!
                todo!()
            }
        }

        impl EntityType for #name {
            type Error = GenericEntityError;

            fn try_from_entity(value: Entity) -> Option<Result<Self, Self::Error>> {
                // TODO!
                todo!()
            }
        }
    }
}

fn generate_ref(
    location: &Location,
    properties: &BTreeMap<&BaseUrl, Property>,
    state: &mut State,
) -> TokenStream {
    let name = Ident::new(&location.name.value, Span::call_site());
    let name_ref = Ident::new(&location.name_ref.value, Span::call_site());

    let def = generate_type(Variant::Ref, location, properties, state);

    quote! {
        #def

        impl TypeRef for #name_ref<'_> {
            type Owned = #name;

            fn into_owned(self) -> Self::Owned {
                // TODO!
                todo!();
            }
        }

        impl<'a> EntityTypeRef<'a> for #name_ref<'a> {
            type Error = GenericEntityError;

            fn try_from_entity(value: &'a Entity) -> Option<Result<Self, Self::Error>> {
                // TODO!
                todo!()
            }
        }
    }
}

fn generate_mut(
    location: &Location,
    properties: &BTreeMap<&BaseUrl, Property>,
    state: &mut State,
) -> TokenStream {
    let name = Ident::new(&location.name.value, Span::call_site());
    let name_mut = Ident::new(&location.name_mut.value, Span::call_site());

    let def = generate_type(Variant::Mut, location, properties, state);

    quote! {
        #def

        impl TypeMut for #name_mut<'_> {
            type Owned = #name;

            fn into_owned(self) -> Self::Owned {
                // TODO!
                todo!();
            }
        }

        impl<'a> EntityTypeMut<'a> for #name_mut<'a> {
            type Error = GenericEntityError;

            fn try_from_entity(value: &'a mut Entity) -> Option<Result<Self, Self::Error>> {
                // TODO!
                todo!()
            }
        }
    }
}

// Reason: most of the lines are just generation code (TODO: we might want to break up in the
// future?)
#[allow(clippy::too_many_lines)]
pub(crate) fn generate(entity: &EntityType, resolver: &NameResolver) -> TokenStream {
    let url = entity.id();

    let location = resolver.location(url);

    let property_type_references = entity.property_type_references();

    let mut references: Vec<_> = property_type_references
        .iter()
        .map(|reference| reference.url())
        .collect();
    // need to sort, as otherwise results might vary between invocations
    references.sort();

    let mut reserved = RESERVED.to_vec();
    reserved.push(&location.name.value);
    reserved.push(&location.name_ref.value);
    reserved.push(&location.name_mut.value);

    if let Some(name) = &location.name.alias {
        reserved.push(name);
    }
    if let Some(name) = &location.name_ref.alias {
        reserved.push(name);
    }
    if let Some(name) = &location.name_mut.alias {
        reserved.push(name);
    }

    let property_names = resolver.property_names(references.iter().map(Deref::deref));
    let locations = resolver.locations(references.iter().map(Deref::deref), &reserved);

    let properties = properties(entity, resolver, &property_names, &locations);

    let is_link = entity
        .inherits_from()
        .all_of()
        .iter()
        .any(|reference| reference == &*LINK_REF);

    let mut state = State {
        is_link,
        import: Import {
            vec: false,
            box_: false,
        },
    };

    let owned = generate_owned(entity, &location, &properties, &mut state);
    let ref_ = generate_ref(&location, &properties, &mut state);
    let mut_ = generate_mut(&location, &properties, &mut state);

    let mod_ = generate_mod(&location.kind, resolver);
    let use_ = generate_use(&references, &locations, state);

    quote! {
        #use_

        #owned
        #ref_
        #mut_

        #mod_
    }
}
