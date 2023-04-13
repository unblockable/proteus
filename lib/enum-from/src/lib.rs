use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn enum_from(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input_enum: syn::ItemEnum = syn::parse(input.clone()).unwrap();

    let enum_name = input_enum.ident;

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

    let from_impls = variants.into_iter().map(|(variant, ty)| {
        quote::quote! {
            impl From<#ty> for #enum_name {
                fn from(value: #ty) -> Self {
                    #enum_name::#variant(value)
                }
            }
        }
    });

    let from_impls = from_impls.into_iter().map(Into::<TokenStream>::into);

    TokenStream::from_iter(std::iter::once(input).chain(from_impls))
}
