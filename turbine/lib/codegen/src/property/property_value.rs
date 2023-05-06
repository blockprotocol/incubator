use std::collections::{BTreeMap, HashMap};

use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use syn::{Token, Visibility};
use type_system::{
    url::{BaseUrl, VersionedUrl},
    Array, DataTypeReference, Object, OneOf, PropertyTypeReference, PropertyValues, ValueOrArray,
};

use crate::{
    name::{Location, NameResolver, PropertyName},
    property::{inner::InnerGenerator, PathSegment, State},
    shared,
    shared::{generate_property_object_conversion_body, ConversionFunction, Property, Variant},
};

#[derive(Debug, Copy, Clone)]
struct SelfVariant<'a>(&'a TokenStream);

impl ToTokens for SelfVariant<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = self.0;
        tokens.extend(quote!(:: #name));
    }
}

#[derive(Debug, Copy, Clone)]
pub(super) struct SelfType<'a> {
    variant: Option<SelfVariant<'a>>,
}

impl<'a> SelfType<'a> {
    const fn hoist(self) -> bool {
        self.variant.is_none()
    }

    fn hoisted_visibility(self) -> Option<Visibility> {
        self.hoist()
            .then_some(Visibility::Public(<Token![pub]>::default()))
    }

    pub(super) const fn enum_(name: &'a TokenStream) -> Self {
        SelfType {
            variant: Some(SelfVariant(name)),
        }
    }

    pub(super) const fn struct_() -> Self {
        SelfType { variant: None }
    }
}

impl ToTokens for SelfType<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let variant = self.variant;
        tokens.extend(quote!(Self #variant));
    }
}

pub(super) struct SelfVariants {
    pub(super) owned: TokenStream,
    pub(super) ref_: TokenStream,
    pub(super) mut_: TokenStream,
}

impl From<&Location<'_>> for SelfVariants {
    fn from(value: &Location) -> Self {
        let name = Ident::new(value.name.value.as_str(), Span::call_site());
        let name_ref = Ident::new(value.name_ref.value.as_str(), Span::call_site());
        let name_mut = Ident::new(value.name_mut.value.as_str(), Span::call_site());

        Self {
            owned: name.to_token_stream(),
            ref_: name_ref.to_token_stream(),
            mut_: name_mut.to_token_stream(),
        }
    }
}

pub(super) struct ConversionBody {
    pub(super) into_owned: Option<TokenStream>,
    pub(super) as_ref: Option<TokenStream>,
    pub(super) as_mut: Option<TokenStream>,
}

pub(super) struct PropertyValue {
    pub(super) body: TokenStream,
    pub(super) try_from: TokenStream,
    pub(super) conversion: ConversionBody,
}

fn properties<'a>(
    id: &VersionedUrl,
    object: &'a Object<ValueOrArray<PropertyTypeReference>, 1>,
    resolver: &NameResolver,
    property_names: &HashMap<&VersionedUrl, PropertyName>,
    locations: &HashMap<&VersionedUrl, Location>,
) -> BTreeMap<&'a BaseUrl, Property> {
    shared::properties(
        id,
        object.properties(),
        object.required(),
        resolver,
        property_names,
        locations,
    )
}

pub(super) struct PropertyValueGenerator<'a> {
    pub(super) id: &'a VersionedUrl,
    pub(super) variant: Variant,

    pub(super) self_type: SelfType<'a>,
    pub(super) self_variants: &'a SelfVariants,

    pub(super) resolver: &'a NameResolver<'a>,
    pub(super) locations: &'a HashMap<&'a VersionedUrl, Location<'a>>,

    pub(super) value: &'a PropertyValues,

    pub(super) state: &'a mut State,
}

impl<'a> PropertyValueGenerator<'a> {
    fn data_type_conversion(&self, inner_type: &TokenStream) -> ConversionBody {
        let SelfVariants { owned, ref_, mut_ } = &self.self_variants;

        match self.variant {
            Variant::Owned => {
                // needs `as_ref` and `as_mut`
                let as_ref = self.self_type.variant.map_or_else(|| {
                    // we hoist, therefore we need to generate the whole function body
                    quote! {
                        let Self(value) = self;

                        // TODO: we need to get the type!
                        #ref_(<#inner_type as Type>::as_ref(value))
                    }
                }, |variant| {
                    // not hoisting, therefore need to generate match arm
                    quote! {
                        Self #variant (value) => <#ref_> #variant (<#inner_type as Type>::as_ref(value))
                    }
                });

                let as_mut = self.self_type.variant.map_or_else(|| quote! {
                        let Self(value) = self;

                        #mut_(<#inner_type as Type>::as_mut(value))
                    }, |variant| quote! {
                        Self #variant (value) => <#mut_> #variant (<#inner_type as TypeMut>::as_mut(value))
                    });

                ConversionBody {
                    into_owned: None,
                    as_ref: Some(as_ref),
                    as_mut: Some(as_mut),
                }
            }
            Variant::Ref | Variant::Mut => {
                let cast = match self.variant {
                    Variant::Ref => quote!(TypeRef),
                    Variant::Mut => quote!(TypeMut),
                    Variant::Owned => unreachable!(),
                };

                // needs `into_owned`
                let into_owned = self.self_type.variant.map_or_else(|| quote! {
                        let Self(value) = self;

                        #owned(<#inner_type as #cast>::into_owned(value))
                    }, |variant| quote! {
                        Self #variant (value) => <#owned> #variant (<#inner_type as #cast>::into_owned(value))
                    });

                ConversionBody {
                    into_owned: Some(into_owned),
                    as_ref: None,
                    as_mut: None,
                }
            }
        }
    }

    fn data_type(&mut self, reference: &DataTypeReference) -> PropertyValue {
        let location = &self.locations[reference.url()];
        let vis = self.self_type.hoisted_visibility();

        let suffix = match self.variant {
            Variant::Owned => None,
            Variant::Ref => Some(quote!(::Ref<'a>)),
            Variant::Mut => Some(quote!(::Mut<'a>)),
        };

        let type_name_raw = location
            .alias
            .value
            .as_ref()
            .unwrap_or(&location.name.value);
        let mut type_name = Ident::new(type_name_raw, Span::call_site()).to_token_stream();

        if let Some(suffix) = suffix {
            type_name = quote!(<#type_name as Type>#suffix);
        }

        let cast = match self.variant {
            Variant::Owned => quote!(as DataType),
            Variant::Ref => quote!(as DataTypeRef<'a>),
            Variant::Mut => quote!(as DataTypeMut<'a>),
        };

        let self_type = self.self_type;
        let try_from = quote!({
            let value = <#type_name #cast>::try_from_value(value)
                .change_context(GenericPropertyError::Data);

            value.map(#self_type)
        });

        let conversion = self.data_type_conversion(&type_name);

        PropertyValue {
            body: quote!((#vis #type_name)),
            try_from,
            conversion,
        }
    }

    fn object_conversion(&self, properties: &BTreeMap<&BaseUrl, Property>) -> ConversionBody {
        let SelfVariants { owned, ref_, mut_ } = &self.self_variants;

        let property_names: Vec<_> = properties
            .values()
            .map(|Property { name, .. }| name)
            .collect();

        // TODO: change the content of entity properties!
        match self.variant {
            Variant::Owned => {
                // needs `as_ref` & `as_mut`
                let as_ref = self.self_type.variant.map_or_else(
                    || {
                        let body = generate_property_object_conversion_body(
                            &quote!(#ref_),
                            ConversionFunction::AsRef,
                            properties,
                        );

                        quote! {
                            let Self { #(#property_names),* } = self;

                            #body
                        }
                    },
                    |variant| {
                        let body = generate_property_object_conversion_body(
                            &quote!(<#ref_> #variant),
                            ConversionFunction::AsRef,
                            properties,
                        );

                        quote! {
                            Self #variant { #(#property_names),* } => #body
                        }
                    },
                );

                let as_mut = self.self_type.variant.map_or_else(
                    || {
                        let body = generate_property_object_conversion_body(
                            &quote!(#mut_),
                            ConversionFunction::AsMut,
                            properties,
                        );

                        quote! {
                            let Self { #(#property_names),* } = self;

                            #body
                        }
                    },
                    |variant| {
                        let body = generate_property_object_conversion_body(
                            &quote!(<#mut_> #variant),
                            ConversionFunction::AsMut,
                            properties,
                        );

                        quote! {
                            Self #variant { #(#property_names),* } => #body
                        }
                    },
                );

                ConversionBody {
                    into_owned: None,
                    as_ref: Some(as_ref),
                    as_mut: Some(as_mut),
                }
            }
            Variant::Ref | Variant::Mut => {
                // needs `into_owned`
                let into_owned = self.self_type.variant.map_or_else(
                    || {
                        let body = generate_property_object_conversion_body(
                            &quote!(#owned),
                            ConversionFunction::IntoOwned {
                                variant: self.variant,
                            },
                            properties,
                        );

                        quote! {
                            let Self { #(#property_names),* } = self;

                            #body
                        }
                    },
                    |variant| {
                        let body = generate_property_object_conversion_body(
                            &quote!(<#owned> #variant),
                            ConversionFunction::IntoOwned {
                                variant: self.variant,
                            },
                            properties,
                        );

                        quote! {
                            Self #variant { #(#property_names),* } => #body
                        }
                    },
                );

                ConversionBody {
                    into_owned: Some(into_owned),
                    as_ref: None,
                    as_mut: None,
                }
            }
        }
    }

    fn object(&mut self, object: &Object<ValueOrArray<PropertyTypeReference>, 1>) -> PropertyValue {
        let property_names = self
            .resolver
            .property_names(object.properties().values().map(|property| match property {
                ValueOrArray::Value(value) => value.url(),
                ValueOrArray::Array(value) => value.items().url(),
            }));

        let properties = properties(
            self.id,
            object,
            self.resolver,
            &property_names,
            self.locations,
        );

        let try_from = shared::generate_properties_try_from_value(
            self.variant,
            &properties,
            &Ident::new("GenericPropertyError", Span::call_site()),
            &self.self_type.to_token_stream(),
        );

        let visibility = self.self_type.hoisted_visibility();

        let conversion = self.object_conversion(&properties);
        let fields = properties.iter().map(|(base, property)| {
            shared::generate_property(
                base,
                property,
                self.variant,
                visibility.as_ref(),
                &mut self.state.import,
            )
        });

        let mutability = match self.variant {
            Variant::Owned => Some(quote!(mut)),
            Variant::Ref | Variant::Mut => None,
        };

        let clone = match self.variant {
            Variant::Owned => Some(quote!(.clone())),
            Variant::Ref | Variant::Mut => None,
        };

        // TODO: integration tests on example project w/ bootstrapping and such

        PropertyValue {
            body: quote!({
                #(#fields),*
            }),
            try_from: quote!('variant: {
                let serde_json::Value::Object(#mutability properties) = value #clone else {
                    break 'variant Err(Report::new(GenericPropertyError::ExpectedObject))
                };

                #try_from
            }),
            conversion,
        }
    }

    fn array_conversion(&self, inner_type: &TokenStream) -> ConversionBody {
        let SelfVariants { owned, ref_, mut_ } = &self.self_variants;

        // The inner type will be `Inner` (for now), which does not implement the `Type` trait,
        // therefore we call the function directly, as it is directly implemented instead.
        match self.variant {
            Variant::Owned => {
                // needs `as_ref` and `as_mut`
                let as_ref = self.self_type.variant.map_or_else(
                    || {
                        quote! {
                            let Self(value) = self;

                            #ref_(
                                value
                                    .iter()
                                    .map(|value| #inner_type::as_ref(value))
                                    .collect()
                            )
                        }
                    },
                    |variant| {
                        quote! {
                            Self #variant (value) => <#ref_> #variant(
                                value
                                    .iter()
                                    .map(|value| #inner_type::as_ref(value))
                                    .collect()
                            )
                        }
                    },
                );

                let as_mut = self.self_type.variant.map_or_else(
                    || {
                        quote! {
                            let Self(value) = self;

                            #mut_(
                                value
                                    .iter_mut()
                                    .map(|value| #inner_type::as_mut(value))
                                    .collect()
                            )
                        }
                    },
                    |variant| {
                        quote! {
                            Self #variant (value) => <#mut_> #variant(
                                value
                                    .iter_mut()
                                    .map(|value| #inner_type::as_mut(value))
                                    .collect()
                            )
                        }
                    },
                );

                ConversionBody {
                    into_owned: None,
                    as_ref: Some(as_ref),
                    as_mut: Some(as_mut),
                }
            }
            Variant::Ref | Variant::Mut => {
                // `into_owned`
                let into_owned = self.self_type.variant.map_or_else(
                    || {
                        quote! {
                            let Self(value) = self;

                            #owned(
                                value
                                    .into_iter()
                                    .map(|value| #inner_type::into_owned(value))
                                    .collect()
                            )
                        }
                    },
                    |variant| {
                        quote! {
                            Self #variant (value) => <#owned> #variant (
                                value
                                    .into_iter()
                                    .map(|value| #inner_type::into_owned(value))
                                    .collect()
                            )
                        }
                    },
                );

                ConversionBody {
                    into_owned: Some(into_owned),
                    as_ref: None,
                    as_mut: None,
                }
            }
        }
    }

    fn array(&mut self, array: &Array<OneOf<PropertyValues>>) -> PropertyValue {
        let items = array.items();

        self.state.stack.push(PathSegment::Array);
        let inner = InnerGenerator {
            id: self.id,
            variant: self.variant,
            values: items.one_of(),
            resolver: self.resolver,
            locations: self.locations,
            state: self.state,
        }
        .finish();
        self.state.stack.pop();

        let vis = self.self_type.hoisted_visibility();

        let lifetime = self
            .variant
            .into_lifetime()
            .map(|lifetime| quote!(<#lifetime>));

        let suffix = match self.variant {
            Variant::Ref => Some(quote!(.map(|array| array.into_boxed_slice()))),
            _ => None,
        };

        let self_type = self.self_type;
        let try_from = quote!({
            match value {
                serde_json::Value::Array(array) => turbine::fold_iter_reports(
                    array.into_iter().map(|value| <#inner #lifetime>::try_from_value(value))
                )
                #suffix
                .map(#self_type)
                .change_context(GenericPropertyError::Array),
                _ => Err(Report::new(GenericPropertyError::ExpectedArray))
            }
        });

        let conversion = self.array_conversion(&inner.to_token_stream());

        // in theory we could do some more hoisting, e.g. if we have multiple OneOf that are
        // Array
        self.state.import.vec = true;

        PropertyValue {
            body: quote!((#vis Vec<#inner #lifetime>)),
            try_from,
            conversion,
        }
    }

    pub(super) fn finish(mut self) -> PropertyValue {
        match self.value {
            PropertyValues::DataTypeReference(reference) => self.data_type(reference),
            PropertyValues::PropertyTypeObject(object) => self.object(object),
            // TODO: automatically flatten, different modes?, inner data-type reference should just
            // be a  newtype?
            PropertyValues::ArrayOfPropertyValues(array) => self.array(array),
        }
    }
}
