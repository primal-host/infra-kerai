/// Extract JSONB metadata from syn items.
use serde_json::{json, Map, Value};

/// Extract visibility as a string.
pub fn visibility_str(vis: &syn::Visibility) -> &'static str {
    match vis {
        syn::Visibility::Public(_) => "pub",
        syn::Visibility::Restricted(r) => {
            if r.path.is_ident("crate") {
                "pub(crate)"
            } else if r.path.is_ident("super") {
                "pub(super)"
            } else if r.path.is_ident("self") {
                "pub(self)"
            } else {
                "pub(restricted)"
            }
        }
        syn::Visibility::Inherited => "private",
    }
}

/// Extract metadata from a function signature.
pub fn fn_metadata(sig: &syn::Signature, vis: &syn::Visibility) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    if sig.asyncness.is_some() {
        m.insert("async".into(), json!(true));
    }
    if sig.unsafety.is_some() {
        m.insert("unsafe".into(), json!(true));
    }
    if sig.constness.is_some() {
        m.insert("const".into(), json!(true));
    }
    if sig.abi.is_some() {
        let abi = sig
            .abi
            .as_ref()
            .and_then(|a| a.name.as_ref())
            .map(|n| n.value());
        if let Some(abi) = abi {
            m.insert("abi".into(), json!(abi));
        }
    }
    if !sig.generics.params.is_empty() {
        let params: Vec<String> = sig
            .generics
            .params
            .iter()
            .map(|p| quote::quote!(#p).to_string())
            .collect();
        m.insert("generics".into(), json!(params));
    }
    Value::Object(m)
}

/// Extract metadata from a struct.
pub fn struct_metadata(item: &syn::ItemStruct, vis: &syn::Visibility) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    if !item.generics.params.is_empty() {
        let params: Vec<String> = item
            .generics
            .params
            .iter()
            .map(|p| quote::quote!(#p).to_string())
            .collect();
        m.insert("generics".into(), json!(params));
    }
    extract_derives(&item.attrs, &mut m);
    extract_cfg(&item.attrs, &mut m);
    Value::Object(m)
}

/// Extract metadata from an enum.
pub fn enum_metadata(item: &syn::ItemEnum, vis: &syn::Visibility) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    if !item.generics.params.is_empty() {
        let params: Vec<String> = item
            .generics
            .params
            .iter()
            .map(|p| quote::quote!(#p).to_string())
            .collect();
        m.insert("generics".into(), json!(params));
    }
    extract_derives(&item.attrs, &mut m);
    extract_cfg(&item.attrs, &mut m);
    Value::Object(m)
}

/// Extract metadata from a trait definition.
pub fn trait_metadata(item: &syn::ItemTrait, vis: &syn::Visibility) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    if item.unsafety.is_some() {
        m.insert("unsafe".into(), json!(true));
    }
    if !item.generics.params.is_empty() {
        let params: Vec<String> = item
            .generics
            .params
            .iter()
            .map(|p| quote::quote!(#p).to_string())
            .collect();
        m.insert("generics".into(), json!(params));
    }
    if !item.supertraits.is_empty() {
        let supers: Vec<String> = item
            .supertraits
            .iter()
            .map(|s| quote::quote!(#s).to_string())
            .collect();
        m.insert("supertraits".into(), json!(supers));
    }
    extract_cfg(&item.attrs, &mut m);
    Value::Object(m)
}

/// Extract metadata from an impl block.
pub fn impl_metadata(item: &syn::ItemImpl) -> Value {
    let mut m = Map::new();
    if item.unsafety.is_some() {
        m.insert("unsafe".into(), json!(true));
    }
    if let Some((_, ref trait_path, _)) = item.trait_ {
        m.insert(
            "trait".into(),
            json!(quote::quote!(#trait_path).to_string()),
        );
    }
    let self_ty = &item.self_ty;
    m.insert("self_ty".into(), json!(quote::quote!(#self_ty).to_string()));
    if !item.generics.params.is_empty() {
        let params: Vec<String> = item
            .generics
            .params
            .iter()
            .map(|p| quote::quote!(#p).to_string())
            .collect();
        m.insert("generics".into(), json!(params));
    }
    extract_cfg(&item.attrs, &mut m);
    Value::Object(m)
}

/// Extract metadata from a const item.
pub fn const_metadata(vis: &syn::Visibility) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    Value::Object(m)
}

/// Extract metadata from a static item.
pub fn static_metadata(item: &syn::ItemStatic) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(&item.vis)));
    if matches!(item.mutability, syn::StaticMutability::Mut(_)) {
        m.insert("mutable".into(), json!(true));
    }
    Value::Object(m)
}

/// Extract metadata from a use statement.
pub fn use_metadata(vis: &syn::Visibility) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    Value::Object(m)
}

/// Extract metadata from attributes list.
pub fn attrs_metadata(attrs: &[syn::Attribute]) -> Value {
    let mut m = Map::new();
    extract_derives(attrs, &mut m);
    extract_cfg(attrs, &mut m);
    let attr_list: Vec<String> = attrs
        .iter()
        .filter(|a| {
            !a.path().is_ident("derive") && !a.path().is_ident("cfg") && !a.path().is_ident("doc")
        })
        .map(|a| quote::quote!(#a).to_string())
        .collect();
    if !attr_list.is_empty() {
        m.insert("attributes".into(), json!(attr_list));
    }
    Value::Object(m)
}

/// Extract metadata for a field.
pub fn field_metadata(vis: &syn::Visibility, ty: &syn::Type) -> Value {
    let mut m = Map::new();
    m.insert("visibility".into(), json!(visibility_str(vis)));
    m.insert("type".into(), json!(quote::quote!(#ty).to_string()));
    Value::Object(m)
}

/// Extract #[derive(...)] trait names from attributes.
fn extract_derives(attrs: &[syn::Attribute], m: &mut Map<String, Value>) {
    let mut derives = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("derive") {
            if let Ok(nested) = attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            ) {
                for path in nested {
                    derives.push(quote::quote!(#path).to_string());
                }
            }
        }
    }
    if !derives.is_empty() {
        m.insert("derives".into(), json!(derives));
    }
}

/// Extract #[cfg(...)] conditions from attributes.
fn extract_cfg(attrs: &[syn::Attribute], m: &mut Map<String, Value>) {
    let mut cfgs = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("cfg") {
            cfgs.push(quote::quote!(#attr).to_string());
        }
    }
    if !cfgs.is_empty() {
        m.insert("cfg".into(), json!(cfgs));
    }
}
