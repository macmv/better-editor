#[proc_macro_error::proc_macro_error]
#[proc_macro_derive(Config)]
pub fn config(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = syn::parse_macro_input!(input as syn::DeriveInput);

  let stream = match input.data {
    syn::Data::Struct(s) => struct_config(&input.ident, s),
    syn::Data::Enum(e) => enum_config(&input.ident, e),
    _ => proc_macro_error::abort_call_site!("only structs and enums are supported"),
  };

  stream.into()
}

fn struct_config(ident: &syn::Ident, s: syn::DataStruct) -> proc_macro2::TokenStream {
  if s.fields.iter().any(|f| f.ident.is_none()) {
    proc_macro_error::abort_call_site!("fields must be named");
  }

  let required_keys = s.fields.iter().filter_map(|f| {
    let id = f.ident.as_ref().unwrap();
    Some(to_kebab_case(&id.to_string()))
  });

  let key_ident = s.fields.iter().map(|f| f.ident.as_ref().unwrap());
  let key_str = key_ident.clone().map(|i| to_kebab_case(&i.to_string()));

  quote::quote! {
    impl ::be_config::parse::ParseTable for #ident {
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
  }
}

fn enum_config(input: &syn::Ident, e: syn::DataEnum) -> proc_macro2::TokenStream {
  proc_macro_error::abort_call_site!("todo: enums");
}

fn to_kebab_case(name: &str) -> String {
  let mut out = String::new();
  for c in name.chars() {
    if c.is_ascii_uppercase() {
      if !out.is_empty() {
        out.push('-');
      }
      out.push(c.to_ascii_lowercase());
    } else if c == '_' {
      out.push('-');
    } else {
      out.push(c);
    }
  }
  out
}
