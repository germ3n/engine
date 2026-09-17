use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemStruct, Fields};

#[proc_macro_attribute]
pub fn Networkable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut ast = parse_macro_input!(item as ItemStruct);
    let name = ast.ident.clone();

    let mut generated_logic = Vec::new();

    if let Fields::Named(ref mut fields) = ast.fields {
        for (idx, field) in fields.named.iter_mut().enumerate() {
            let is_networked = field.attrs.iter().any(|attr| {
                attr.meta.path().is_ident("Networked")
            });

            if is_networked {
                let field_name = &field.ident;
                let field_type = &field.ty;
                
                generated_logic.push(quote! {
                    println!(
                        "Field idx {} -> Syncing {} of type {}", 
                        #idx, 
                        stringify!(#field_name), 
                        stringify!(#field_type)
                    );
                });

                // Strip the helper attribute from the final AST so the compiler doesn't panic
                field.attrs.retain(|attr| !attr.meta.path().is_ident("Networked"));
            }
        }
    }

    let gen = quote! {
        #ast

        impl Networkable for #name {
            fn entity_id(&self) -> i32 {
                self.base.entity_id
            }

            fn sync_network_vars(&self) {
                #(#generated_logic)*
            }
        }
    };

    gen.into()
}