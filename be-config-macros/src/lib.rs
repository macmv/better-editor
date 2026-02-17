#[proc_macro_error::proc_macro_error]
#[proc_macro_derive(Config)]
pub fn config(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = syn::parse_macro_input!(input as syn::DeriveInput);

  let syn::Data::Struct(s) = input.data else {
    proc_macro_error::abort_call_site!("only structs are supported");
  };

  if s.fields.iter().any(|f| f.ident.is_none()) {
    proc_macro_error::abort_call_site!("fields must be named");
  }

  let name = &input.ident;

  let required_keys = s.fields.iter().filter_map(|f| {
    let id = f.ident.as_ref().unwrap();
    Some(id.to_string())
  });

  let key_ident = s.fields.iter().map(|f| f.ident.as_ref().unwrap());
  let key_str = key_ident.clone().map(|i| i.to_string());

  let stream = quote::quote! {
    impl ::be_config::parse::ParseTable for #name {
      fn required_keys() -> &'static [&'static str] {
        &[#(#required_keys),*]
      }

      fn set_key(
        &mut self,
        key: &str,
        value: ::be_config::parse::DeValue,
        de: &mut ::be_config::parse::Parser,
      ) -> bool {
        match key {
          #(#key_str => self.#key_ident = de.value(value),)*
          _ => return false,
        }

        true
      }
    }
  };

  stream.into()
}
