use proc_macro::TokenStream;
use syn::{Ident, ItemEnum, Type};

fn get_enum_variants(input_enum: ItemEnum) -> Vec<(Ident, Type)> {
    let mut variants = Vec::new();

    for variant in input_enum.variants {
        let syn::Fields::Unnamed(fields) = variant.fields else {
            panic!("Fields for variant were not unnamed");
        };

        let mut fields = fields.unnamed.into_iter();
        let field = fields.next().expect("Variant had no fields");
        assert!(fields.next().is_none(), "Variant had more than one field");

        variants.push((variant.ident, field.ty));
    }

    variants
}

#[proc_macro_attribute]
pub fn enum_from(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input_enum: syn::ItemEnum = syn::parse(input.clone()).unwrap();

    let enum_name = input_enum.ident.clone();
    let enum_variants = get_enum_variants(input_enum);

    let from_impls = enum_variants.into_iter().map(|(var, ty)| {
        quote::quote! {
            impl From<#ty> for #enum_name {
                fn from(value: #ty) -> Self {
                    #enum_name::#var(value)
                }
            }
        }
    });

    let from_impls = from_impls.into_iter().map(Into::<TokenStream>::into);

    TokenStream::from_iter(std::iter::once(input).chain(from_impls))
}

#[proc_macro_attribute]
pub fn enum_try_from(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input_enum: syn::ItemEnum = syn::parse(input.clone()).unwrap();

    let enum_name = input_enum.ident.clone();
    let enum_variants = get_enum_variants(input_enum);

    let try_from_impls = enum_variants.into_iter().map(|(var, ty)| {
        quote::quote! {
            impl TryFrom<#enum_name> for #ty {
                type Error = String;

                fn try_from(value: #enum_name) -> Result<Self, Self::Error> {
                    if let #enum_name::#var(inner) = value {
                        Ok(inner)
                    } else {
                        return Err(String::from("Invalid variant"))
                    }
                }
            }
            impl<'a> TryFrom<&'a #enum_name> for &'a #ty {
                type Error = String;

                fn try_from(value: &'a #enum_name) -> Result<Self, Self::Error> {
                    if let #enum_name::#var(inner) = value {
                        Ok(inner)
                    } else {
                        return Err(String::from("Invalid variant"))
                    }
                }
            }
        }
    });

    let try_from_impls = try_from_impls.into_iter().map(Into::<TokenStream>::into);

    TokenStream::from_iter(std::iter::once(input).chain(try_from_impls))
}
