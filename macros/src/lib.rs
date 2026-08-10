//! Attribute-macro sugar for [`pipeline-viz`](https://docs.rs/pipeline-viz).
//!
//! Enable it through the parent crate — `pipeline-viz = { features = ["macros"] }`
//! — and read the documentation there. Nothing here is useful on its own.
//!
//! Both macros expand to *exactly* the calls a user would write against the
//! runtime API, resolved through [`pipeline_viz::global`]. There is no second
//! state machine and no separate code path, so the two surfaces cannot drift
//! apart. With no tracker installed, or with the `viz` feature off, every
//! generated call is a no-op.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, quote_spanned};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{parse_macro_input, Error, Expr, ItemFn, Meta, ReturnType, Token, Type};

/// Registers the annotated function's stage with the installed tracker the
/// first time it runs, then executes the body unchanged.
///
/// ```ignore
/// #[track_node(kind = Sink, name = "Database Committer", inputs = ["account_job"])]
/// async fn commit(block: Block) { /* ... */ }
/// ```
///
/// | Argument | Default | Meaning |
/// |---|---|---|
/// | `id` | the function's name | Node id used by `track_job` and the wire protocol |
/// | `kind` | `Transform` | `Source`, `Transform`, or `Sink` |
/// | `name` | the id | Display name in the dashboard |
/// | `inputs` | `[]` | Ids of the nodes that feed this one |
#[proc_macro_attribute]
pub fn track_node(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let args = parse_macro_input!(args with Punctuated::<Meta, Token![,]>::parse_terminated);

    match expand_node(args, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Tracks one work item for the duration of the annotated function.
///
/// ```ignore
/// #[track_job(node = "committer", id = block.number, job_type = "Block")]
/// async fn commit(block: Block) { /* ... */ }
/// ```
///
/// | Argument | Required | Meaning |
/// |---|---|---|
/// | `node` | yes | Id of the node the item is entering |
/// | `id` | yes | Expression evaluated in the function body's scope |
/// | `job_type` | no | Category shown beside the item |
/// | `meta(k = expr, ..)` | no | Extra fields shown in the detail panel |
///
/// The guard completes when the body returns normally — including an early
/// `return` or a `?` that produced an `Err`. A panic instead drops the guard,
/// which is what marks the item abandoned.
#[proc_macro_attribute]
pub fn track_job(args: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let args = parse_macro_input!(args with Punctuated::<Meta, Token![,]>::parse_terminated);

    match expand_job(args, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_node(
    args: Punctuated<Meta, Token![,]>,
    mut function: ItemFn,
) -> syn::Result<TokenStream2> {
    let mut id = None;
    let mut name = None;
    let mut kind = None;
    let mut inputs = None;

    for arg in args {
        let meta = name_value(&arg)?;
        match ident_of(&meta.0)?.as_str() {
            "id" => id = Some(meta.1),
            "name" => name = Some(meta.1),
            "kind" => kind = Some(meta.1),
            "inputs" => inputs = Some(meta.1),
            other => {
                return Err(Error::new(
                    arg.span(),
                    format!("unknown `track_node` argument `{other}`; expected one of `id`, `name`, `kind`, `inputs`"),
                ))
            }
        }
    }

    let id = match id {
        Some(id) => quote!(#id),
        // The function name is the obvious id and keeps the common case to a
        // bare `#[track_node]`.
        None => {
            let literal = function.sig.ident.to_string();
            quote!(#literal)
        }
    };
    let name = name.map_or_else(|| quote!(#id), |name| quote!(#name));
    let kind = match kind {
        Some(kind) => quote_spanned!(kind.span()=> ::pipeline_viz::NodeKind::#kind),
        None => quote!(::pipeline_viz::NodeKind::Transform),
    };
    let inputs = match inputs {
        Some(inputs) => quote!(#inputs),
        None => quote!([] as [&str; 0]),
    };

    let block = function.block.clone();
    function.block = syn::parse_quote!({
        // Registration is idempotent on the collector, but emitting it once per
        // call would put a message on the channel for every item processed.
        static __PIPELINE_VIZ_REGISTER: ::std::sync::Once = ::std::sync::Once::new();
        __PIPELINE_VIZ_REGISTER.call_once(|| {
            if let ::core::option::Option::Some(__pipeline_viz_tracker) = ::pipeline_viz::global() {
                __pipeline_viz_tracker.register_node_named(#id, #name, #kind, #inputs);
            }
        });
        #block
    });

    Ok(quote!(#function))
}

fn expand_job(
    args: Punctuated<Meta, Token![,]>,
    mut function: ItemFn,
) -> syn::Result<TokenStream2> {
    let span = function.sig.ident.span();
    let mut node = None;
    let mut id = None;
    let mut job_type = None;
    let mut meta = Vec::new();

    for arg in args {
        // `meta(key = expr, ..)` is a list; everything else is a name-value.
        if let Meta::List(list) = &arg {
            if list.path.is_ident("meta") {
                let entries =
                    list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
                for entry in entries {
                    let (path, value) = name_value(&entry)?;
                    let key = ident_of(&path)?;
                    meta.push(quote!(.meta(#key, #value)));
                }
                continue;
            }
        }

        let (path, value) = name_value(&arg)?;
        match ident_of(&path)?.as_str() {
            "node" => node = Some(value),
            "id" => id = Some(value),
            "job_type" => job_type = Some(value),
            other => {
                return Err(Error::new(
                    arg.span(),
                    format!("unknown `track_job` argument `{other}`; expected one of `node`, `id`, `job_type`, `meta(..)`"),
                ))
            }
        }
    }

    let node = node.ok_or_else(|| {
        Error::new(
            span,
            "`track_job` needs the node the item is entering: `#[track_job(node = \"committer\", id = ..)]`",
        )
    })?;
    let id = id.ok_or_else(|| {
        Error::new(
            span,
            "`track_job` needs an id expression for the item: `#[track_job(node = .., id = block.number)]`",
        )
    })?;
    let job_type = job_type.map(|job_type| quote!(.job_type(#job_type)));

    let block = &function.block;
    let output = &function.sig.output;

    // The body moves into a closure or an async block so that an early `return`
    // leaves the body rather than the whole function, and the guard is still
    // completed. That requires naming the type the body produces.
    let body = if function.sig.asyncness.is_some() {
        match annotation(output) {
            Some(annotation) => {
                quote!(let __pipeline_viz_output: #annotation = async #block.await;)
            }
            None => quote!(let __pipeline_viz_output = async #block.await;),
        }
    } else {
        match annotation(output) {
            Some(annotation) => quote!(let __pipeline_viz_output: #annotation = (|| #block)();),
            None => quote!(let __pipeline_viz_output = (|| #block)();),
        }
    };

    function.block = syn::parse_quote!({
        let __pipeline_viz_job = ::pipeline_viz::global().map(|__pipeline_viz_tracker| {
            __pipeline_viz_tracker
                .job(#node)
                .id(#id)
                #job_type
                #(#meta)*
                .start()
        });

        #body

        if let ::core::option::Option::Some(__pipeline_viz_guard) = __pipeline_viz_job {
            __pipeline_viz_guard.complete();
        }

        __pipeline_viz_output
    });

    Ok(quote!(#function))
}

/// The type to bind the body's result to, or `None` when it cannot be named.
///
/// `-> impl Trait` and `-> _` are not valid in a `let`, so those bodies are left
/// to inference. That is enough for every case except a `?` whose error type
/// inference then has nothing to work from, which reports itself clearly.
fn annotation(output: &ReturnType) -> Option<&Type> {
    match output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => match **ty {
            Type::ImplTrait(_) | Type::Infer(_) => None,
            ref ty => Some(ty),
        },
    }
}

fn name_value(meta: &Meta) -> syn::Result<(syn::Path, Expr)> {
    match meta {
        Meta::NameValue(pair) => Ok((pair.path.clone(), pair.value.clone())),
        other => Err(Error::new(
            other.span(),
            "expected `name = value`, for example `node = \"committer\"`",
        )),
    }
}

fn ident_of(path: &syn::Path) -> syn::Result<String> {
    path.get_ident()
        .map(ToString::to_string)
        .ok_or_else(|| Error::new(path.span(), "expected a plain argument name"))
}
