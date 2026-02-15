/// Recursive AST walker that converts syn types into NodeRow/EdgeRow vectors.
use serde_json::{json, Value};
use uuid::Uuid;

use super::kinds;
use super::metadata;
use super::path_builder::PathContext;

/// A row to be inserted into kerai.nodes.
#[derive(Debug, Clone)]
pub struct NodeRow {
    pub id: String,
    pub instance_id: String,
    pub kind: String,
    pub language: Option<String>,
    pub content: Option<String>,
    pub parent_id: Option<String>,
    pub position: i32,
    pub path: Option<String>,
    pub metadata: Value,
    pub span_start: Option<i32>,
    #[allow(dead_code)]
    pub span_end: Option<i32>,
}

/// A row to be inserted into kerai.edges.
#[derive(Debug, Clone)]
pub struct EdgeRow {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub metadata: Value,
}

/// Walk context passed through the recursion.
struct WalkCtx {
    instance_id: String,
    nodes: Vec<NodeRow>,
    edges: Vec<EdgeRow>,
    path_ctx: PathContext,
}

impl WalkCtx {
    fn new_node(
        &mut self,
        kind: &str,
        content: Option<String>,
        parent_id: Option<&str>,
        position: i32,
        meta: Value,
        span_start: Option<i32>,
        span_end: Option<i32>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        self.nodes.push(NodeRow {
            id: id.clone(),
            instance_id: self.instance_id.clone(),
            kind: kind.to_string(),
            language: Some("rust".to_string()),
            content,
            parent_id: parent_id.map(|s| s.to_string()),
            position,
            path: self.path_ctx.path(),
            metadata: meta,
            span_start,
            span_end,
        });
        id
    }

    fn new_edge(&mut self, source_id: &str, target_id: &str, relation: &str) {
        self.edges.push(EdgeRow {
            id: Uuid::new_v4().to_string(),
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            relation: relation.to_string(),
            metadata: json!({}),
        });
    }
}

/// Get line number from a proc_macro2::Span.
fn span_start_line(span: proc_macro2::Span) -> Option<i32> {
    let start = span.start();
    if start.line > 0 {
        Some(start.line as i32)
    } else {
        None
    }
}

fn span_end_line(span: proc_macro2::Span) -> Option<i32> {
    let end = span.end();
    if end.line > 0 {
        Some(end.line as i32)
    } else {
        None
    }
}

/// Helper to quote a syn node to string via the quote crate.
fn to_token_string(tokens: impl quote::ToTokens) -> String {
    quote::quote!(#tokens).to_string()
}

/// Walk a syn::File and produce NodeRow/EdgeRow vectors.
pub fn walk_file(
    file: &syn::File,
    file_node_id: &str,
    instance_id: &str,
    path_ctx: PathContext,
) -> (Vec<NodeRow>, Vec<EdgeRow>) {
    let mut ctx = WalkCtx {
        instance_id: instance_id.to_string(),
        nodes: Vec::new(),
        edges: Vec::new(),
        path_ctx,
    };

    // Walk inner attributes
    for (pos, attr) in file.attrs.iter().enumerate() {
        walk_attribute(&mut ctx, attr, file_node_id, pos as i32, true);
    }

    // Walk items
    for (pos, item) in file.items.iter().enumerate() {
        walk_item(&mut ctx, item, file_node_id, pos as i32);
    }

    (ctx.nodes, ctx.edges)
}

fn walk_item(ctx: &mut WalkCtx, item: &syn::Item, parent_id: &str, position: i32) {
    match item {
        syn::Item::Fn(item_fn) => walk_fn(ctx, item_fn, parent_id, position),
        syn::Item::Struct(item_struct) => walk_struct(ctx, item_struct, parent_id, position),
        syn::Item::Enum(item_enum) => walk_enum(ctx, item_enum, parent_id, position),
        syn::Item::Impl(item_impl) => walk_impl(ctx, item_impl, parent_id, position),
        syn::Item::Trait(item_trait) => walk_trait(ctx, item_trait, parent_id, position),
        syn::Item::Mod(item_mod) => walk_mod(ctx, item_mod, parent_id, position),
        syn::Item::Use(item_use) => walk_use(ctx, item_use, parent_id, position),
        syn::Item::Const(item_const) => walk_const(ctx, item_const, parent_id, position),
        syn::Item::Static(item_static) => walk_static(ctx, item_static, parent_id, position),
        syn::Item::Type(item_type) => walk_type_alias(ctx, item_type, parent_id, position),
        syn::Item::Macro(item_macro) => walk_macro(ctx, item_macro, parent_id, position),
        syn::Item::ExternCrate(item_extern) => {
            walk_extern_crate(ctx, item_extern, parent_id, position)
        }
        syn::Item::ForeignMod(item_foreign) => {
            walk_foreign_mod(ctx, item_foreign, parent_id, position)
        }
        syn::Item::Union(item_union) => walk_union(ctx, item_union, parent_id, position),
        syn::Item::TraitAlias(item_alias) => {
            walk_trait_alias(ctx, item_alias, parent_id, position)
        }
        _ => {
            ctx.new_node(
                "item_other",
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
    }
}

fn walk_fn(ctx: &mut WalkCtx, item_fn: &syn::ItemFn, parent_id: &str, position: i32) {
    let name = item_fn.sig.ident.to_string();
    let meta = metadata::fn_metadata(&item_fn.sig, &item_fn.vis);
    let span = item_fn.sig.ident.span();

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::FN,
        Some(name),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(item_fn.block.brace_token.span.close()),
    );

    for (i, attr) in item_fn.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    for (i, arg) in item_fn.sig.inputs.iter().enumerate() {
        walk_fn_arg(ctx, arg, &node_id, i as i32);
    }

    if let syn::ReturnType::Type(_, ty) = &item_fn.sig.output {
        walk_type(ctx, ty, &node_id, item_fn.sig.inputs.len() as i32);
    }

    walk_block(ctx, &item_fn.block, &node_id, (item_fn.sig.inputs.len() + 1) as i32);

    ctx.path_ctx.pop();
}

fn walk_struct(ctx: &mut WalkCtx, item: &syn::ItemStruct, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let meta = metadata::struct_metadata(item, &item.vis);
    let span = item.ident.span();

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::STRUCT,
        Some(name),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(span),
    );

    for (i, attr) in item.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    match &item.fields {
        syn::Fields::Named(fields) => {
            for (i, field) in fields.named.iter().enumerate() {
                walk_field(ctx, field, &node_id, i as i32);
            }
        }
        syn::Fields::Unnamed(fields) => {
            for (i, field) in fields.unnamed.iter().enumerate() {
                walk_field(ctx, field, &node_id, i as i32);
            }
        }
        syn::Fields::Unit => {}
    }

    ctx.path_ctx.pop();
}

fn walk_field(ctx: &mut WalkCtx, field: &syn::Field, parent_id: &str, position: i32) {
    let name = field.ident.as_ref().map(|i| i.to_string());
    let meta = metadata::field_metadata(&field.vis, &field.ty);
    let span = field
        .ident
        .as_ref()
        .map(|i| i.span())
        .unwrap_or_else(proc_macro2::Span::call_site);

    if let Some(ref n) = name {
        ctx.path_ctx.push(n);
    }

    let node_id = ctx.new_node(
        kinds::FIELD,
        name.clone(),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(span),
    );

    for (i, attr) in field.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    if name.is_some() {
        ctx.path_ctx.pop();
    }
}

fn walk_enum(ctx: &mut WalkCtx, item: &syn::ItemEnum, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let meta = metadata::enum_metadata(item, &item.vis);
    let span = item.ident.span();

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::ENUM,
        Some(name),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(span),
    );

    for (i, attr) in item.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    for (i, variant) in item.variants.iter().enumerate() {
        walk_variant(ctx, variant, &node_id, i as i32);
    }

    ctx.path_ctx.pop();
}

fn walk_variant(ctx: &mut WalkCtx, variant: &syn::Variant, parent_id: &str, position: i32) {
    let name = variant.ident.to_string();
    let span = variant.ident.span();
    let mut meta = serde_json::Map::new();

    if let Some((_, ref expr)) = variant.discriminant {
        meta.insert("discriminant".into(), json!(to_token_string(expr)));
    }

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::VARIANT,
        Some(name),
        Some(parent_id),
        position,
        Value::Object(meta),
        span_start_line(span),
        span_end_line(span),
    );

    for (i, attr) in variant.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    match &variant.fields {
        syn::Fields::Named(fields) => {
            for (i, field) in fields.named.iter().enumerate() {
                walk_field(ctx, field, &node_id, i as i32);
            }
        }
        syn::Fields::Unnamed(fields) => {
            for (i, field) in fields.unnamed.iter().enumerate() {
                walk_field(ctx, field, &node_id, i as i32);
            }
        }
        syn::Fields::Unit => {}
    }

    ctx.path_ctx.pop();
}

fn walk_impl(ctx: &mut WalkCtx, item: &syn::ItemImpl, parent_id: &str, position: i32) {
    let meta = metadata::impl_metadata(item);
    let self_ty = &item.self_ty;
    let self_ty_str = to_token_string(self_ty);
    let label = if let Some((_, ref trait_path, _)) = item.trait_ {
        format!("impl {} for {}", to_token_string(trait_path), self_ty_str)
    } else {
        format!("impl {}", self_ty_str)
    };

    ctx.path_ctx.push(&label.replace(' ', "_").replace("::", "_"));
    let node_id = ctx.new_node(
        kinds::IMPL,
        Some(label),
        Some(parent_id),
        position,
        meta,
        None,
        None,
    );

    for (i, attr) in item.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    for (i, impl_item) in item.items.iter().enumerate() {
        walk_impl_item(ctx, impl_item, &node_id, i as i32);
    }

    ctx.path_ctx.pop();
}

fn walk_impl_item(ctx: &mut WalkCtx, item: &syn::ImplItem, parent_id: &str, position: i32) {
    match item {
        syn::ImplItem::Fn(method) => {
            let name = method.sig.ident.to_string();
            let meta = metadata::fn_metadata(&method.sig, &method.vis);
            let span = method.sig.ident.span();

            ctx.path_ctx.push(&name);
            let node_id = ctx.new_node(
                kinds::FN,
                Some(name),
                Some(parent_id),
                position,
                meta,
                span_start_line(span),
                span_end_line(method.block.brace_token.span.close()),
            );

            for (i, attr) in method.attrs.iter().enumerate() {
                walk_attribute(ctx, attr, &node_id, i as i32, false);
            }

            for (i, arg) in method.sig.inputs.iter().enumerate() {
                walk_fn_arg(ctx, arg, &node_id, i as i32);
            }

            if let syn::ReturnType::Type(_, ty) = &method.sig.output {
                walk_type(ctx, ty, &node_id, method.sig.inputs.len() as i32);
            }

            walk_block(ctx, &method.block, &node_id, (method.sig.inputs.len() + 1) as i32);

            ctx.path_ctx.pop();
        }
        syn::ImplItem::Const(c) => {
            let name = c.ident.to_string();
            ctx.path_ctx.push(&name);
            ctx.new_node(
                kinds::CONST,
                Some(name),
                Some(parent_id),
                position,
                metadata::const_metadata(&c.vis),
                span_start_line(c.ident.span()),
                span_end_line(c.ident.span()),
            );
            ctx.path_ctx.pop();
        }
        syn::ImplItem::Type(t) => {
            let name = t.ident.to_string();
            ctx.path_ctx.push(&name);
            ctx.new_node(
                kinds::TYPE_ALIAS,
                Some(name),
                Some(parent_id),
                position,
                json!({"visibility": metadata::visibility_str(&t.vis)}),
                span_start_line(t.ident.span()),
                span_end_line(t.ident.span()),
            );
            ctx.path_ctx.pop();
        }
        syn::ImplItem::Macro(m) => {
            let mac_path = &m.mac.path;
            ctx.new_node(
                kinds::MACRO_CALL,
                Some(to_token_string(mac_path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        _ => {
            ctx.new_node(
                "impl_item_other",
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
    }
}

fn walk_trait(ctx: &mut WalkCtx, item: &syn::ItemTrait, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let meta = metadata::trait_metadata(item, &item.vis);
    let span = item.ident.span();

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::TRAIT,
        Some(name),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(span),
    );

    for (i, attr) in item.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    for (i, trait_item) in item.items.iter().enumerate() {
        walk_trait_item(ctx, trait_item, &node_id, i as i32);
    }

    ctx.path_ctx.pop();
}

fn walk_trait_item(ctx: &mut WalkCtx, item: &syn::TraitItem, parent_id: &str, position: i32) {
    match item {
        syn::TraitItem::Fn(method) => {
            let name = method.sig.ident.to_string();
            let meta = metadata::fn_metadata(&method.sig, &syn::Visibility::Inherited);
            let span = method.sig.ident.span();

            ctx.path_ctx.push(&name);
            let node_id = ctx.new_node(
                kinds::FN,
                Some(name),
                Some(parent_id),
                position,
                meta,
                span_start_line(span),
                span_end_line(span),
            );

            for (i, arg) in method.sig.inputs.iter().enumerate() {
                walk_fn_arg(ctx, arg, &node_id, i as i32);
            }

            if let syn::ReturnType::Type(_, ty) = &method.sig.output {
                walk_type(ctx, ty, &node_id, method.sig.inputs.len() as i32);
            }

            if let Some(block) = &method.default {
                walk_block(ctx, block, &node_id, (method.sig.inputs.len() + 1) as i32);
            }

            ctx.path_ctx.pop();
        }
        syn::TraitItem::Type(t) => {
            let name = t.ident.to_string();
            ctx.path_ctx.push(&name);
            ctx.new_node(
                kinds::TYPE_ALIAS,
                Some(name),
                Some(parent_id),
                position,
                json!({}),
                span_start_line(t.ident.span()),
                span_end_line(t.ident.span()),
            );
            ctx.path_ctx.pop();
        }
        syn::TraitItem::Const(c) => {
            let name = c.ident.to_string();
            ctx.path_ctx.push(&name);
            ctx.new_node(
                kinds::CONST,
                Some(name),
                Some(parent_id),
                position,
                json!({}),
                span_start_line(c.ident.span()),
                span_end_line(c.ident.span()),
            );
            ctx.path_ctx.pop();
        }
        syn::TraitItem::Macro(m) => {
            let mac_path = &m.mac.path;
            ctx.new_node(
                kinds::MACRO_CALL,
                Some(to_token_string(mac_path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        _ => {
            ctx.new_node(
                "trait_item_other",
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
    }
}

fn walk_mod(ctx: &mut WalkCtx, item: &syn::ItemMod, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let span = item.ident.span();
    let mut meta = serde_json::Map::new();
    meta.insert(
        "visibility".into(),
        json!(metadata::visibility_str(&item.vis)),
    );

    let is_test = item.attrs.iter().any(|a| {
        if a.path().is_ident("cfg") {
            to_token_string(a).contains("test")
        } else {
            false
        }
    });
    if is_test {
        meta.insert("test".into(), json!(true));
    }

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::MODULE,
        Some(name),
        Some(parent_id),
        position,
        Value::Object(meta),
        span_start_line(span),
        span_end_line(span),
    );

    for (i, attr) in item.attrs.iter().enumerate() {
        walk_attribute(ctx, attr, &node_id, i as i32, false);
    }

    if let Some((_, items)) = &item.content {
        for (i, sub_item) in items.iter().enumerate() {
            walk_item(ctx, sub_item, &node_id, i as i32);
        }
    }

    ctx.path_ctx.pop();
}

fn walk_use(ctx: &mut WalkCtx, item: &syn::ItemUse, parent_id: &str, position: i32) {
    let content = to_token_string(item);
    let meta = metadata::use_metadata(&item.vis);

    ctx.new_node(
        kinds::USE,
        Some(content),
        Some(parent_id),
        position,
        meta,
        None,
        None,
    );
}

fn walk_const(ctx: &mut WalkCtx, item: &syn::ItemConst, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let span = item.ident.span();
    let meta = metadata::const_metadata(&item.vis);

    ctx.path_ctx.push(&name);
    ctx.new_node(
        kinds::CONST,
        Some(name),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(span),
    );
    ctx.path_ctx.pop();
}

fn walk_static(ctx: &mut WalkCtx, item: &syn::ItemStatic, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let span = item.ident.span();
    let meta = metadata::static_metadata(item);

    ctx.path_ctx.push(&name);
    ctx.new_node(
        kinds::STATIC,
        Some(name),
        Some(parent_id),
        position,
        meta,
        span_start_line(span),
        span_end_line(span),
    );
    ctx.path_ctx.pop();
}

fn walk_type_alias(ctx: &mut WalkCtx, item: &syn::ItemType, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let span = item.ident.span();

    ctx.path_ctx.push(&name);
    ctx.new_node(
        kinds::TYPE_ALIAS,
        Some(name),
        Some(parent_id),
        position,
        json!({"visibility": metadata::visibility_str(&item.vis)}),
        span_start_line(span),
        span_end_line(span),
    );
    ctx.path_ctx.pop();
}

fn walk_macro(ctx: &mut WalkCtx, item: &syn::ItemMacro, parent_id: &str, position: i32) {
    let mac_path = &item.mac.path;
    let name = to_token_string(mac_path);

    let kind = if item.ident.is_some() {
        kinds::MACRO_DEF
    } else {
        kinds::MACRO_CALL
    };

    let content = if let Some(ref ident) = item.ident {
        ident.to_string()
    } else {
        name.clone()
    };

    let mut meta = serde_json::Map::new();
    if kind == kinds::MACRO_CALL {
        meta.insert("macro_path".into(), json!(name));
    }

    ctx.new_node(
        kind,
        Some(content),
        Some(parent_id),
        position,
        Value::Object(meta),
        None,
        None,
    );
}

fn walk_extern_crate(
    ctx: &mut WalkCtx,
    item: &syn::ItemExternCrate,
    parent_id: &str,
    position: i32,
) {
    let name = item.ident.to_string();
    ctx.new_node(
        kinds::EXTERN_CRATE,
        Some(name),
        Some(parent_id),
        position,
        json!({"visibility": metadata::visibility_str(&item.vis)}),
        span_start_line(item.ident.span()),
        span_end_line(item.ident.span()),
    );
}

fn walk_foreign_mod(
    ctx: &mut WalkCtx,
    item: &syn::ItemForeignMod,
    parent_id: &str,
    position: i32,
) {
    let abi = item
        .abi
        .name
        .as_ref()
        .map(|n| n.value())
        .unwrap_or_default();

    ctx.new_node(
        kinds::FOREIGN_MOD,
        Some(format!("extern \"{}\"", abi)),
        Some(parent_id),
        position,
        json!({"abi": abi}),
        None,
        None,
    );
}

fn walk_union(ctx: &mut WalkCtx, item: &syn::ItemUnion, parent_id: &str, position: i32) {
    let name = item.ident.to_string();
    let span = item.ident.span();

    ctx.path_ctx.push(&name);
    let node_id = ctx.new_node(
        kinds::UNION,
        Some(name),
        Some(parent_id),
        position,
        json!({"visibility": metadata::visibility_str(&item.vis)}),
        span_start_line(span),
        span_end_line(span),
    );

    for (i, field) in item.fields.named.iter().enumerate() {
        walk_field(ctx, field, &node_id, i as i32);
    }

    ctx.path_ctx.pop();
}

fn walk_trait_alias(
    ctx: &mut WalkCtx,
    item: &syn::ItemTraitAlias,
    parent_id: &str,
    position: i32,
) {
    let name = item.ident.to_string();

    ctx.path_ctx.push(&name);
    ctx.new_node(
        kinds::TRAIT_ALIAS,
        Some(name),
        Some(parent_id),
        position,
        json!({"visibility": metadata::visibility_str(&item.vis)}),
        span_start_line(item.ident.span()),
        span_end_line(item.ident.span()),
    );
    ctx.path_ctx.pop();
}

fn walk_attribute(
    ctx: &mut WalkCtx,
    attr: &syn::Attribute,
    parent_id: &str,
    position: i32,
    is_inner: bool,
) {
    if attr.path().is_ident("doc") {
        let doc_text = if let syn::Meta::NameValue(nv) = &attr.meta {
            if let syn::Expr::Lit(expr_lit) = &nv.value {
                if let syn::Lit::Str(s) = &expr_lit.lit {
                    Some(s.value())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let content = doc_text.unwrap_or_else(|| to_token_string(attr));
        let mut meta = serde_json::Map::new();
        if is_inner {
            meta.insert("inner".into(), json!(true));
        }

        let node_id = ctx.new_node(
            kinds::DOC_COMMENT,
            Some(content),
            Some(parent_id),
            position,
            Value::Object(meta),
            None,
            None,
        );

        ctx.new_edge(&node_id, parent_id, "documents");
        return;
    }

    let content = to_token_string(attr);
    let mut meta = serde_json::Map::new();
    if is_inner {
        meta.insert("inner".into(), json!(true));
    }

    ctx.new_node(
        kinds::ATTRIBUTE,
        Some(content),
        Some(parent_id),
        position,
        Value::Object(meta),
        None,
        None,
    );
}

fn walk_fn_arg(ctx: &mut WalkCtx, arg: &syn::FnArg, parent_id: &str, position: i32) {
    match arg {
        syn::FnArg::Receiver(recv) => {
            ctx.new_node(
                kinds::PARAM,
                Some(to_token_string(recv)),
                Some(parent_id),
                position,
                json!({"is_self": true}),
                None,
                None,
            );
        }
        syn::FnArg::Typed(pat_type) => {
            let pat = &pat_type.pat;
            let ty = &pat_type.ty;
            ctx.new_node(
                kinds::PARAM,
                Some(to_token_string(pat)),
                Some(parent_id),
                position,
                json!({"type": to_token_string(ty)}),
                None,
                None,
            );
        }
    }
}

fn walk_block(ctx: &mut WalkCtx, block: &syn::Block, parent_id: &str, position: i32) {
    let node_id = ctx.new_node(
        kinds::BLOCK,
        None,
        Some(parent_id),
        position,
        json!({}),
        None,
        None,
    );

    for (i, stmt) in block.stmts.iter().enumerate() {
        walk_stmt(ctx, stmt, &node_id, i as i32);
    }
}

fn walk_stmt(ctx: &mut WalkCtx, stmt: &syn::Stmt, parent_id: &str, position: i32) {
    match stmt {
        syn::Stmt::Local(local) => {
            let pat = &local.pat;
            let node_id = ctx.new_node(
                kinds::STMT_LOCAL,
                Some(to_token_string(pat)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );

            walk_pat(ctx, &local.pat, &node_id, 0);

            if let Some(init) = &local.init {
                walk_expr(ctx, &init.expr, &node_id, 1);
                if let Some((_, diverge)) = &init.diverge {
                    walk_expr(ctx, diverge, &node_id, 2);
                }
            }
        }
        syn::Stmt::Item(item) => {
            walk_item(ctx, item, parent_id, position);
        }
        syn::Stmt::Expr(expr, _semi) => {
            walk_expr(ctx, expr, parent_id, position);
        }
        syn::Stmt::Macro(stmt_macro) => {
            let mac_path = &stmt_macro.mac.path;
            ctx.new_node(
                kinds::MACRO_CALL,
                Some(to_token_string(mac_path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
    }
}

fn walk_expr(ctx: &mut WalkCtx, expr: &syn::Expr, parent_id: &str, position: i32) {
    match expr {
        syn::Expr::Call(call) => {
            let node_id = ctx.new_node(
                kinds::EXPR_CALL,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &call.func, &node_id, 0);
            for (i, arg) in call.args.iter().enumerate() {
                walk_expr(ctx, arg, &node_id, (i + 1) as i32);
            }
        }
        syn::Expr::MethodCall(mc) => {
            let method_name = mc.method.to_string();
            let node_id = ctx.new_node(
                kinds::EXPR_METHOD_CALL,
                Some(method_name),
                Some(parent_id),
                position,
                json!({}),
                span_start_line(mc.method.span()),
                span_end_line(mc.method.span()),
            );
            walk_expr(ctx, &mc.receiver, &node_id, 0);
            for (i, arg) in mc.args.iter().enumerate() {
                walk_expr(ctx, arg, &node_id, (i + 1) as i32);
            }
        }
        syn::Expr::If(expr_if) => {
            let node_id = ctx.new_node(
                kinds::EXPR_IF,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &expr_if.cond, &node_id, 0);
            walk_block(ctx, &expr_if.then_branch, &node_id, 1);
            if let Some((_, else_branch)) = &expr_if.else_branch {
                walk_expr(ctx, else_branch, &node_id, 2);
            }
        }
        syn::Expr::Match(expr_match) => {
            let node_id = ctx.new_node(
                kinds::EXPR_MATCH,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &expr_match.expr, &node_id, 0);
            for (i, arm) in expr_match.arms.iter().enumerate() {
                let arm_id = ctx.new_node(
                    kinds::EXPR_MATCH_ARM,
                    None,
                    Some(&node_id),
                    (i + 1) as i32,
                    json!({}),
                    None,
                    None,
                );
                walk_pat(ctx, &arm.pat, &arm_id, 0);
                if let Some((_, guard_expr)) = &arm.guard {
                    walk_expr(ctx, guard_expr, &arm_id, 1);
                }
                walk_expr(ctx, &arm.body, &arm_id, 2);
            }
        }
        syn::Expr::Closure(closure) => {
            let node_id = ctx.new_node(
                kinds::EXPR_CLOSURE,
                None,
                Some(parent_id),
                position,
                json!({
                    "async": closure.asyncness.is_some(),
                    "move": closure.capture.is_some(),
                }),
                None,
                None,
            );
            for (i, arg) in closure.inputs.iter().enumerate() {
                walk_pat(ctx, arg, &node_id, i as i32);
            }
            walk_expr(ctx, &closure.body, &node_id, closure.inputs.len() as i32);
        }
        syn::Expr::Block(expr_block) => {
            let node_id = ctx.new_node(
                kinds::EXPR_BLOCK,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_block(ctx, &expr_block.block, &node_id, 0);
        }
        syn::Expr::ForLoop(for_loop) => {
            let node_id = ctx.new_node(
                kinds::EXPR_FOR,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_pat(ctx, &for_loop.pat, &node_id, 0);
            walk_expr(ctx, &for_loop.expr, &node_id, 1);
            walk_block(ctx, &for_loop.body, &node_id, 2);
        }
        syn::Expr::While(while_loop) => {
            let node_id = ctx.new_node(
                kinds::EXPR_WHILE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &while_loop.cond, &node_id, 0);
            walk_block(ctx, &while_loop.body, &node_id, 1);
        }
        syn::Expr::Loop(loop_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_LOOP,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_block(ctx, &loop_expr.body, &node_id, 0);
        }
        syn::Expr::Return(ret) => {
            let node_id = ctx.new_node(
                kinds::EXPR_RETURN,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            if let Some(expr) = &ret.expr {
                walk_expr(ctx, expr, &node_id, 0);
            }
        }
        syn::Expr::Break(brk) => {
            let node_id = ctx.new_node(
                kinds::EXPR_BREAK,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            if let Some(expr) = &brk.expr {
                walk_expr(ctx, expr, &node_id, 0);
            }
        }
        syn::Expr::Continue(_) => {
            ctx.new_node(
                kinds::EXPR_CONTINUE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Expr::Assign(assign) => {
            let node_id = ctx.new_node(
                kinds::EXPR_ASSIGN,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &assign.left, &node_id, 0);
            walk_expr(ctx, &assign.right, &node_id, 1);
        }
        syn::Expr::Binary(bin) => {
            let op = &bin.op;
            let node_id = ctx.new_node(
                kinds::EXPR_BINARY,
                Some(to_token_string(op)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &bin.left, &node_id, 0);
            walk_expr(ctx, &bin.right, &node_id, 1);
        }
        syn::Expr::Unary(un) => {
            let op = &un.op;
            let node_id = ctx.new_node(
                kinds::EXPR_UNARY,
                Some(to_token_string(op)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &un.expr, &node_id, 0);
        }
        syn::Expr::Field(field_expr) => {
            let member = &field_expr.member;
            let node_id = ctx.new_node(
                kinds::EXPR_FIELD,
                Some(to_token_string(member)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &field_expr.base, &node_id, 0);
        }
        syn::Expr::Index(idx) => {
            let node_id = ctx.new_node(
                kinds::EXPR_INDEX,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &idx.expr, &node_id, 0);
            walk_expr(ctx, &idx.index, &node_id, 1);
        }
        syn::Expr::Reference(ref_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_REFERENCE,
                None,
                Some(parent_id),
                position,
                json!({"mutable": ref_expr.mutability.is_some()}),
                None,
                None,
            );
            walk_expr(ctx, &ref_expr.expr, &node_id, 0);
        }
        syn::Expr::Struct(struct_expr) => {
            let path = &struct_expr.path;
            let node_id = ctx.new_node(
                kinds::EXPR_STRUCT,
                Some(to_token_string(path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, fv) in struct_expr.fields.iter().enumerate() {
                let member = &fv.member;
                let field_id = ctx.new_node(
                    kinds::FIELD,
                    Some(to_token_string(member)),
                    Some(&node_id),
                    i as i32,
                    json!({}),
                    None,
                    None,
                );
                walk_expr(ctx, &fv.expr, &field_id, 0);
            }
            if let Some(rest) = &struct_expr.rest {
                walk_expr(ctx, rest, &node_id, struct_expr.fields.len() as i32);
            }
        }
        syn::Expr::Tuple(tuple) => {
            let node_id = ctx.new_node(
                kinds::EXPR_TUPLE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, elem) in tuple.elems.iter().enumerate() {
                walk_expr(ctx, elem, &node_id, i as i32);
            }
        }
        syn::Expr::Array(arr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_ARRAY,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, elem) in arr.elems.iter().enumerate() {
                walk_expr(ctx, elem, &node_id, i as i32);
            }
        }
        syn::Expr::Cast(cast) => {
            let ty = &cast.ty;
            let node_id = ctx.new_node(
                kinds::EXPR_CAST,
                None,
                Some(parent_id),
                position,
                json!({"target_type": to_token_string(ty)}),
                None,
                None,
            );
            walk_expr(ctx, &cast.expr, &node_id, 0);
        }
        syn::Expr::Path(expr_path) => {
            let path = &expr_path.path;
            ctx.new_node(
                kinds::EXPR_PATH,
                Some(to_token_string(path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Expr::Lit(expr_lit) => {
            let lit = &expr_lit.lit;
            ctx.new_node(
                kinds::LIT,
                Some(to_token_string(lit)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Expr::Range(range) => {
            let node_id = ctx.new_node(
                kinds::EXPR_RANGE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            if let Some(start) = &range.start {
                walk_expr(ctx, start, &node_id, 0);
            }
            if let Some(end) = &range.end {
                walk_expr(ctx, end, &node_id, 1);
            }
        }
        syn::Expr::Let(let_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_LET,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_pat(ctx, &let_expr.pat, &node_id, 0);
            walk_expr(ctx, &let_expr.expr, &node_id, 1);
        }
        syn::Expr::Async(async_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_ASYNC,
                None,
                Some(parent_id),
                position,
                json!({"move": async_expr.capture.is_some()}),
                None,
                None,
            );
            walk_block(ctx, &async_expr.block, &node_id, 0);
        }
        syn::Expr::Await(await_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_AWAIT,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &await_expr.base, &node_id, 0);
        }
        syn::Expr::Try(try_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_TRY,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &try_expr.expr, &node_id, 0);
        }
        syn::Expr::Unsafe(unsafe_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_UNSAFE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_block(ctx, &unsafe_expr.block, &node_id, 0);
        }
        syn::Expr::Const(const_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_CONST,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_block(ctx, &const_expr.block, &node_id, 0);
        }
        syn::Expr::Paren(paren) => {
            walk_expr(ctx, &paren.expr, parent_id, position);
        }
        syn::Expr::Repeat(repeat) => {
            let node_id = ctx.new_node(
                kinds::EXPR_REPEAT,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            walk_expr(ctx, &repeat.expr, &node_id, 0);
            walk_expr(ctx, &repeat.len, &node_id, 1);
        }
        syn::Expr::Macro(mac) => {
            let mac_path = &mac.mac.path;
            ctx.new_node(
                kinds::MACRO_CALL,
                Some(to_token_string(mac_path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Expr::Yield(yield_expr) => {
            let node_id = ctx.new_node(
                kinds::EXPR_YIELD,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            if let Some(expr) = &yield_expr.expr {
                walk_expr(ctx, expr, &node_id, 0);
            }
        }
        _ => {
            ctx.new_node(
                kinds::EXPR_OTHER,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
    }
}

fn walk_pat(ctx: &mut WalkCtx, pat: &syn::Pat, parent_id: &str, position: i32) {
    match pat {
        syn::Pat::Ident(pat_ident) => {
            let name = pat_ident.ident.to_string();
            let mut meta = serde_json::Map::new();
            if pat_ident.by_ref.is_some() {
                meta.insert("by_ref".into(), json!(true));
            }
            if pat_ident.mutability.is_some() {
                meta.insert("mutable".into(), json!(true));
            }
            let node_id = ctx.new_node(
                kinds::PAT_IDENT,
                Some(name),
                Some(parent_id),
                position,
                Value::Object(meta),
                span_start_line(pat_ident.ident.span()),
                span_end_line(pat_ident.ident.span()),
            );
            if let Some((_, sub_pat)) = &pat_ident.subpat {
                walk_pat(ctx, sub_pat, &node_id, 0);
            }
        }
        syn::Pat::Struct(pat_struct) => {
            let path = &pat_struct.path;
            let node_id = ctx.new_node(
                kinds::PAT_STRUCT,
                Some(to_token_string(path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, fp) in pat_struct.fields.iter().enumerate() {
                let member = &fp.member;
                let field_id = ctx.new_node(
                    kinds::FIELD,
                    Some(to_token_string(member)),
                    Some(&node_id),
                    i as i32,
                    json!({}),
                    None,
                    None,
                );
                walk_pat(ctx, &fp.pat, &field_id, 0);
            }
        }
        syn::Pat::TupleStruct(pat_ts) => {
            let path = &pat_ts.path;
            let node_id = ctx.new_node(
                kinds::PAT_TUPLE_STRUCT,
                Some(to_token_string(path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, elem) in pat_ts.elems.iter().enumerate() {
                walk_pat(ctx, elem, &node_id, i as i32);
            }
        }
        syn::Pat::Tuple(pat_tuple) => {
            let node_id = ctx.new_node(
                kinds::PAT_TUPLE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, elem) in pat_tuple.elems.iter().enumerate() {
                walk_pat(ctx, elem, &node_id, i as i32);
            }
        }
        syn::Pat::Or(pat_or) => {
            let node_id = ctx.new_node(
                kinds::PAT_OR,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, case) in pat_or.cases.iter().enumerate() {
                walk_pat(ctx, case, &node_id, i as i32);
            }
        }
        syn::Pat::Slice(pat_slice) => {
            let node_id = ctx.new_node(
                kinds::PAT_SLICE,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
            for (i, elem) in pat_slice.elems.iter().enumerate() {
                walk_pat(ctx, elem, &node_id, i as i32);
            }
        }
        syn::Pat::Rest(_) => {
            ctx.new_node(
                kinds::PAT_REST,
                Some("..".to_string()),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Pat::Wild(_) => {
            ctx.new_node(
                kinds::PAT_WILD,
                Some("_".to_string()),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Pat::Reference(pat_ref) => {
            let node_id = ctx.new_node(
                kinds::PAT_REF,
                None,
                Some(parent_id),
                position,
                json!({"mutable": pat_ref.mutability.is_some()}),
                None,
                None,
            );
            walk_pat(ctx, &pat_ref.pat, &node_id, 0);
        }
        syn::Pat::Path(pat_path) => {
            let path = &pat_path.path;
            ctx.new_node(
                kinds::PAT_PATH,
                Some(to_token_string(path)),
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
        syn::Pat::Paren(paren) => {
            walk_pat(ctx, &paren.pat, parent_id, position);
        }
        _ => {
            ctx.new_node(
                kinds::PAT_OTHER,
                None,
                Some(parent_id),
                position,
                json!({}),
                None,
                None,
            );
        }
    }
}

fn walk_type(ctx: &mut WalkCtx, ty: &syn::Type, parent_id: &str, position: i32) {
    let kind = match ty {
        syn::Type::Path(_) => kinds::TYPE_PATH,
        syn::Type::Reference(_) => kinds::TYPE_REFERENCE,
        syn::Type::Tuple(_) => kinds::TYPE_TUPLE,
        syn::Type::Array(_) => kinds::TYPE_ARRAY,
        syn::Type::Slice(_) => kinds::TYPE_SLICE,
        syn::Type::BareFn(_) => kinds::TYPE_FN,
        syn::Type::ImplTrait(_) => kinds::TYPE_IMPL_TRAIT,
        syn::Type::TraitObject(_) => kinds::TYPE_DYN_TRAIT,
        syn::Type::Never(_) => kinds::TYPE_NEVER,
        syn::Type::Infer(_) => kinds::TYPE_INFER,
        _ => kinds::TYPE_OTHER,
    };

    ctx.new_node(
        kind,
        Some(to_token_string(ty)),
        Some(parent_id),
        position,
        json!({}),
        None,
        None,
    );
}
