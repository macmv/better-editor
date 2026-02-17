#[proc_macro_error::proc_macro_error]
#[proc_macro_derive(Config, attributes(config))]
pub fn config(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
  let input = syn::parse_macro_input!(input as syn::DeriveInput);

  let stream = match input.data {
    syn::Data::Struct(s) => struct_config(&input.ident, s),
    syn::Data::Enum(e) => {
      if e.variants.iter().any(|v| !matches!(v.fields, syn::Fields::Unit)) {
        tagged_enum_config(&input.ident, &input.attrs, e)
      } else {
        string_enum_config(&input.ident, e)
      }
    }
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
          #(#key_str => de.partial_value(&mut self.#key_ident, value),)*
          _ => return false,
        }

        true
      }
    }
  }
}

fn tagged_enum_config(
  ident: &syn::Ident,
  attrs: &[syn::Attribute],
  e: syn::DataEnum,
) -> proc_macro2::TokenStream {
  let mut tag = None::<syn::LitStr>;
  for attr in attrs {
    if !attr.path().is_ident("config") {
      continue;
    }

    if let Err(err) = attr.parse_nested_meta(|meta| {
      if meta.path.is_ident("tag") {
        tag = Some(meta.value()?.parse()?);
        return Ok(());
      }

      Err(meta.error("unsupported config attribute; expected `tag = \"...\"`"))
    }) {
      proc_macro_error::abort!(attr, "{}", err);
    }
  }

  let Some(tag) = tag else {
    proc_macro_error::abort_call_site!("enum Config derives require #[config(tag = \"...\")]");
  };

  let mut variant_arms = vec![];
  for variant in &e.variants {
    if variant.discriminant.is_some() {
      proc_macro_error::abort!(
        variant,
        "enum Config derives do not support variants with discriminants"
      );
    }

    let variant_ident = &variant.ident;
    let variant_tag = to_kebab_case(&variant_ident.to_string());
    let variant_tag_lit = syn::LitStr::new(&variant_tag, variant_ident.span());

    match &variant.fields {
      syn::Fields::Unit => {
        variant_arms.push(quote::quote! {
          #variant_tag_lit => {
            if !is_empty {
              de.warn(format!("unknown key for variant '{}'", #variant_tag_lit), 0..0);
            }
            #ident::#variant_ident
          }
        });
      }
      syn::Fields::Unnamed(f) => {
        if f.unnamed.len() != 1 {
          proc_macro_error::abort!(variant, "enum Config derives only support a single value");
        }

        variant_arms.push(quote::quote! {
          #variant_tag_lit => #ident::#variant_ident(de.complete_value(rest))
        });
      }
      syn::Fields::Named(_) => {
        proc_macro_error::abort!(variant, "enum Config derives do not support inline structs");
      }
    }
  }

  quote::quote! {
    impl ::be_config::parse::ParseValue for #ident {
      fn parse(
        &mut self,
        value: ::be_config::parse::DeValue,
        de: &mut ::be_config::parse::Parser,
      ) -> ::std::result::Result<(), String> {
        let ::be_config::parse::DeValue::Table(mut table) = value else {
          return Err("expected table".to_string());
        };

        let tag_value = table
          .remove(#tag)
          .ok_or_else(|| format!("missing key: '{}'", #tag))?
          .into_inner();
        let ::be_config::parse::DeValue::String(tag) = tag_value else {
          return Err(format!("expected '{}' to be a string", #tag));
        };

        let is_empty = table.is_empty();
        let rest = ::be_config::parse::DeValue::Table(table);

        *self = match tag.as_ref() {
          #(#variant_arms,)*
          _ => return Err(format!(
            "unknown {} variant: '{}'",
            #tag,
            tag.as_ref()
          )),
        };

        Ok(())
      }
    }
  }
}

fn string_enum_config(ident: &syn::Ident, e: syn::DataEnum) -> proc_macro2::TokenStream {
  let mut variant_arms = vec![];
  for variant in &e.variants {
    if variant.discriminant.is_some() {
      proc_macro_error::abort!(
        variant,
        "enum Config derives do not support variants with discriminants"
      );
    }

    let variant_ident = &variant.ident;
    let variant_tag = to_kebab_case(&variant_ident.to_string());
    let variant_tag_lit = syn::LitStr::new(&variant_tag, variant_ident.span());

    variant_arms.push(quote::quote! {
      #variant_tag_lit => #ident::#variant_ident
    });
  }

  quote::quote! {
    impl ::be_config::parse::ParseValue for #ident {
      fn parse(
        &mut self,
        value: ::be_config::parse::DeValue,
        de: &mut ::be_config::parse::Parser,
      ) -> ::std::result::Result<(), String> {
        let ::be_config::parse::DeValue::String(mut s) = value else {
          return Err("expected string".to_string());
        };

        *self = match &*s {
          #(#variant_arms,)*
          s => return Err(format!("unknown variant: '{s}'")),
        };

        Ok(())
      }
    }
  }
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
