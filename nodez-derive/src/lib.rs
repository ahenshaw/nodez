//! **Prototype.** Derive macros for `nodez`.
//!
//! `#[derive(SocketType)]` turns a Rust type into a wire type: a name, a color,
//! a shape, and optionally an inline editor. `#[derive(NodeType)]` turns a
//! struct into a kind of node: the field types give the sockets, their arity and
//! their widgets, and the generated reader resolves each field from its link or
//! its inline value.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Expr, Fields, Ident, LitStr, Type, parse_macro_input};

// ---------------------------------------------------------------- SocketType

#[proc_macro_derive(SocketType, attributes(socket))]
pub fn derive_socket_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match socket_type_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[derive(Default)]
struct SocketAttrs {
    name: Option<LitStr>,
    color: Option<LitStr>,
    shape: Option<LitStr>,
    widget: Option<Ident>,
    description: Option<LitStr>,
    wildcard: bool,
}

fn parse_socket_attrs(input: &DeriveInput) -> syn::Result<SocketAttrs> {
    let mut attrs = SocketAttrs::default();
    for attr in &input.attrs {
        if !attr.path().is_ident("socket") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            match key.as_str() {
                "name" => attrs.name = Some(meta.value()?.parse()?),
                "color" => attrs.color = Some(meta.value()?.parse()?),
                "shape" => attrs.shape = Some(meta.value()?.parse()?),
                "description" => attrs.description = Some(meta.value()?.parse()?),
                "widget" => attrs.widget = Some(meta.value()?.parse()?),
                "wildcard" => attrs.wildcard = true,
                other => {
                    return Err(meta.error(format!(
                        "unknown socket option `{other}`; expected name, color, shape, \
                         widget, description or wildcard"
                    )));
                }
            }
            Ok(())
        })?;
    }
    Ok(attrs)
}

fn socket_type_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let ident = &input.ident;
    let attrs = parse_socket_attrs(input)?;

    let name = attrs
        .name
        .map(|n| n.value())
        .unwrap_or_else(|| ident.to_string());
    // A type that says nothing about color gets one derived from its name,
    // which is stable and usually good enough to tell sockets apart.
    let color = match attrs.color.as_ref() {
        Some(lit) => {
            let (r, g, b) = parse_hex(lit)?;
            quote!(::nodez::__macro_support::color(#r, #g, #b))
        }
        None => quote!(::nodez::auto_color(#name)),
    };
    let shape = shape_variant(attrs.shape.as_ref())?;
    let description = attrs.description.map(|d| d.value()).unwrap_or_default();
    let wildcard = attrs.wildcard;

    // How the value is edited inline, if it can be. Types that say nothing are
    // link-only, which is what the trait defaults give.
    let editor = match attrs.widget.as_ref().map(ToString::to_string).as_deref() {
        None => quote! {},
        Some("text") => newtype_editor(input, quote!(::nodez::Widget::text()), Scalar::Text)?,
        Some("int") => newtype_editor(input, quote!(::nodez::Widget::int()), Scalar::Int)?,
        Some("float") => newtype_editor(input, quote!(::nodez::Widget::float()), Scalar::Float)?,
        Some("checkbox") => {
            newtype_editor(input, quote!(::nodez::Widget::Checkbox), Scalar::Bool)?
        }
        Some("choice") => choice_editor(input)?,
        Some(other) => {
            return Err(syn::Error::new_spanned(
                attrs.widget.as_ref().unwrap(),
                format!(
                    "unknown widget `{other}`; expected text, int, float, checkbox or choice"
                ),
            ));
        }
    };

    let froms = conversions(input)?;

    Ok(quote! {
        impl ::nodez::typed::SocketType for #ident {
            const NAME: &'static str = #name;

            fn data_type() -> ::nodez::DataTypeBuilder {
                ::nodez::DataTypeBuilder::new(#name, #color)
                .shape(::nodez::SocketShape::#shape)
                .description(#description)
                .wildcard(#wildcard)
            }

            #editor
        }

        #froms
    })
}

enum Scalar {
    Text,
    Int,
    Float,
    Bool,
}

/// The `to_value` / `from_value` pair for a one-field tuple struct.
fn newtype_editor(
    input: &DeriveInput,
    widget: TokenStream2,
    scalar: Scalar,
) -> syn::Result<TokenStream2> {
    newtype_field(input)?;
    let (to, from) = match scalar {
        Scalar::Text => (
            quote!(::nodez::Value::Text(self.0.clone())),
            quote!(value.as_str().map(|s| Self(s.to_owned()))),
        ),
        Scalar::Int => (
            quote!(::nodez::Value::Int(self.0)),
            quote!(value.as_i64().map(Self)),
        ),
        Scalar::Float => (
            quote!(::nodez::Value::Float(self.0)),
            quote!(value.as_f64().map(Self)),
        ),
        Scalar::Bool => (
            quote!(::nodez::Value::Bool(self.0)),
            quote!(value.as_bool().map(Self)),
        ),
    };
    Ok(quote! {
        fn widget() -> ::nodez::Widget { #widget }
        fn to_value(&self) -> ::nodez::Value { #to }
        fn from_value(value: &::nodez::Value) -> Option<Self> { #from }
    })
}

/// A fieldless enum becomes a dropdown, one option per variant.
fn choice_editor(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "#[socket(widget = choice)] needs a fieldless enum",
        ));
    };
    let mut options = Vec::new();
    let mut to_arms = Vec::new();
    let mut from_arms = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "#[socket(widget = choice)] needs every variant to be fieldless",
            ));
        }
        let ident = &variant.ident;
        // A variant may spell its option differently: `V3_9` is `3.9` to a
        // reader, and Rust has no way to name that variant directly.
        let mut renamed = None;
        for attr in &variant.attrs {
            if !attr.path().is_ident("socket") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    renamed = Some(meta.value()?.parse::<LitStr>()?.value());
                    Ok(())
                } else {
                    Err(meta.error("a variant only takes `rename`"))
                }
            })?;
        }
        let text = renamed.unwrap_or_else(|| kebab_case(&ident.to_string()));
        options.push(quote!(#text));
        to_arms.push(quote!(Self::#ident => #text));
        from_arms.push(quote!(#text => Some(Self::#ident)));
    }
    Ok(quote! {
        fn widget() -> ::nodez::Widget {
            ::nodez::Widget::combo([#(#options),*])
        }
        fn to_value(&self) -> ::nodez::Value {
            ::nodez::Value::Choice(match self { #(#to_arms),* }.to_owned())
        }
        fn from_value(value: &::nodez::Value) -> Option<Self> {
            match value.as_str()? { #(#from_arms,)* _ => None }
        }
    })
}

/// `From` impls so a template default can be written as a bare literal.
fn conversions(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let ident = &input.ident;
    let Ok(inner) = newtype_field(input) else {
        return Ok(quote! {});
    };
    let extra = if is_type(&inner, "String") {
        quote! {
            impl From<&str> for #ident {
                fn from(value: &str) -> Self { Self(value.to_owned()) }
            }
        }
    } else {
        quote! {}
    };
    Ok(quote! {
        impl From<#inner> for #ident {
            fn from(value: #inner) -> Self { Self(value) }
        }
        #extra
    })
}

fn newtype_field(input: &DeriveInput) -> syn::Result<Type> {
    if let Data::Struct(data) = &input.data
        && let Fields::Unnamed(fields) = &data.fields
        && fields.unnamed.len() == 1
    {
        return Ok(fields.unnamed[0].ty.clone());
    }
    Err(syn::Error::new_spanned(
        &input.ident,
        "an editable socket type must be a one-field tuple struct, e.g. `struct Text(String)`",
    ))
}

// ---------------------------------------------------------------------- Node

#[proc_macro_derive(NodeType, attributes(node, input, param))]
pub fn derive_node_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match node_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

#[derive(Default)]
struct NodeAttrs {
    id: Option<LitStr>,
    label: Option<LitStr>,
    category: Option<LitStr>,
    description: Option<LitStr>,
    width: Option<Expr>,
    output: Option<Type>,
    output_name: Option<LitStr>,
    header_color: Option<LitStr>,
    produces: Option<Type>,
    keywords: Option<LitStr>,
}

fn node_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let ident = &input.ident;
    let mut attrs = NodeAttrs::default();
    for attr in &input.attrs {
        if !attr.path().is_ident("node") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            match key.as_str() {
                "id" => attrs.id = Some(meta.value()?.parse()?),
                "label" => attrs.label = Some(meta.value()?.parse()?),
                "category" => attrs.category = Some(meta.value()?.parse()?),
                "description" => attrs.description = Some(meta.value()?.parse()?),
                "width" => attrs.width = Some(meta.value()?.parse()?),
                "output" => attrs.output = Some(meta.value()?.parse()?),
                "output_name" => attrs.output_name = Some(meta.value()?.parse()?),
                "header_color" => attrs.header_color = Some(meta.value()?.parse()?),
                "produces" => attrs.produces = Some(meta.value()?.parse()?),
                "keywords" => attrs.keywords = Some(meta.value()?.parse()?),
                other => {
                    return Err(meta.error(format!(
                        "unknown node option `{other}`; expected id, label, category, \
                         description, width, output, output_name, header_color, produces \
                         or keywords"
                    )));
                }
            }
            Ok(())
        })?;
    }

    let id = attrs.id.as_ref().map(|i| i.value()).unwrap_or_else(|| {
        snake_case(&ident.to_string())
    });
    let label = attrs
        .label
        .map(|l| l.value())
        .unwrap_or_else(|| title_case(&ident.to_string()));
    let category = attrs
        .category
        .map(|c| c.value())
        .unwrap_or_else(|| "Misc".to_owned());
    let description = attrs.description.map(|d| d.value()).unwrap_or_default();
    let width = attrs
        .width
        .map(|w| quote!(#w))
        .unwrap_or_else(|| quote!(150.0f32));
    // Extra search terms for the add menu, comma-separated.
    let keywords = match attrs.keywords.as_ref() {
        Some(lit) => {
            let terms: Vec<String> = lit
                .value()
                .split(',')
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty())
                .collect();
            quote!(template = template.keywords([#(#terms),*]);)
        }
        None => quote!(),
    };
    let header_color = match attrs.header_color.as_ref() {
        Some(lit) => {
            let (r, g, b) = parse_hex(lit)?;
            quote!(template = template.header_color(
                ::nodez::__macro_support::color(#r, #g, #b)
            );)
        }
        None => quote!(),
    };

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(NodeType)] needs a struct with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            ident,
            "#[derive(NodeType)] needs a struct with named fields",
        ));
    };

    let mut schema = Vec::new();
    let mut reads = Vec::new();

    for field in &fields.named {
        let field_ident = field.ident.as_ref().expect("named field");
        let name = field_ident.to_string();
        let spec = parse_field(field)?;

        match spec.role {
            Role::Param => {
                let ty = &field.ty;
                let label = spec.label.clone().unwrap_or_else(|| title_case(&name));
                let hint = spec_hint(&spec.hint, &label);
                let default = spec
                    .default
                    .map(|d| quote!(.default_value(::nodez::typed::SocketType::to_value(
                        &::core::convert::Into::<#ty>::into(#d)
                    ))))
                    .unwrap_or_default();
                let show_label = spec.show_label;
                schema.push(quote! {
                    template = template.param(
                        ::nodez::ParamSpec::new(
                            #name,
                            ::nodez::typed::__private::with_hint(
                                <#ty as ::nodez::typed::SocketType>::widget(),
                                #hint,
                            ),
                        )
                        .label(#label)
                        .show_label(#show_label)
                        #default
                    );
                });
                reads.push(quote! {
                    #field_ident: ::nodez::typed::__private::param::<#ty, __N>(ctx, #name)?
                });
            }
            Role::Input { arity, payload } => {
                let label = spec.label.clone().unwrap_or_else(|| title_case(&name));
                let description = spec.description.unwrap_or_default();
                let has_default = spec.default.is_some();
                let default = spec
                    .default
                    .map(|d| quote!(socket = socket.default_value(
                        ::nodez::typed::SocketType::to_value(
                            &::core::convert::Into::<#payload>::into(#d)
                        )
                    );))
                    .unwrap_or_default();
                let hint = spec_hint(&spec.hint, &label);
                let min = spec
                    .min
                    .as_ref()
                    .map_or_else(|| quote!(None), |v| quote!(Some(f64::from(#v))));
                let max = spec
                    .max
                    .as_ref()
                    .map_or_else(|| quote!(None), |v| quote!(Some(f64::from(#v))));
                // Two combinations are silently meaningless, and a macro cannot
                // see trait impls to reject them at expansion. Check when the
                // template is built instead, so they fail loudly in dev builds.
                let default_needs_widget = if has_default { quote! {
                    debug_assert!(
                        widget != ::nodez::Widget::None,
                        "input `{}` has a default, but its type `{}` has no inline \
                         editor, so nothing can ever show or use that value",
                        #name,
                        <#payload as ::nodez::typed::SocketType>::NAME,
                    );
                } } else { quote!() };
                let optional_needs_link_only = if matches!(arity, Arity::Optional) { quote! {
                    debug_assert!(
                        widget == ::nodez::Widget::None,
                        "input `{}` is Option<{}>, but `{}` is editable, so the socket \
                         always has a value and the Option can never be None",
                        #name,
                        <#payload as ::nodez::typed::SocketType>::NAME,
                        <#payload as ::nodez::typed::SocketType>::NAME,
                    );
                } } else { quote!() };
                let multi = matches!(arity, Arity::Multi)
                    .then(|| quote!(socket = socket.multi();))
                    .unwrap_or_default();
                // `Option<T>` is the only way a field says an input may be
                // left unwired, so it is the only thing that can tell the
                // editor which inputs are still to be filled in.
                let optional = matches!(arity, Arity::Optional)
                    .then(|| quote!(socket = socket.optional();))
                    .unwrap_or_default();

                schema.push(quote! {
                    {
                        let ty = ::nodez::typed::__private::register_type::<#payload>(types);
                        let mut socket = ::nodez::SocketSpec::new(#name, ty)
                            .label(#label)
                            .description(#description);
                        // A payload type that offers a widget gets an inline
                        // editor; one that does not is link-only. The check is
                        // at runtime because a macro cannot see trait impls.
                        let widget = ::nodez::typed::__private::with_range(
                            ::nodez::typed::__private::with_hint(
                                <#payload as ::nodez::typed::SocketType>::widget(),
                                #hint,
                            ),
                            #min,
                            #max,
                        );
                        #default_needs_widget
                        #optional_needs_link_only
                        if widget != ::nodez::Widget::None {
                            socket = socket.editable(widget);
                        }
                        #default
                        #multi
                        #optional
                        template = template.input(socket);
                    }
                });

                let reader = match arity {
                    Arity::Required => quote!(required),
                    Arity::Optional => quote!(optional),
                    Arity::Multi => quote!(multi),
                };
                reads.push(quote! {
                    #field_ident: ::nodez::typed::__private::#reader::<#payload, __N>(ctx, #name)?
                });
            }
        }
    }

    let output = match &attrs.output {
        Some(ty) => {
            let socket = attrs
                .output_name
                .map(|n| n.value())
                .unwrap_or_else(|| "out".to_owned());
            quote! {
                {
                    let ty = ::nodez::typed::__private::register_type::<#ty>(types);
                    template = template.output(
                        ::nodez::SocketSpec::new(#socket, ty)
                            .label(<#ty as ::nodez::typed::SocketType>::NAME),
                    );
                }
            }
        }
        None => quote! {},
    };

    // The output socket's type is what downstream nodes downcast to, so the
    // rule's output type is pinned to it rather than written a second time.
    let produces = match (&attrs.output, &attrs.produces) {
        (Some(_), Some(ty)) => {
            return Err(syn::Error::new_spanned(
                ty,
                "a node with an `output` socket already says what it produces; \
                 `produces` is only for a node that has no output socket",
            ));
        }
        (Some(ty), None) | (None, Some(ty)) => quote!(#ty),
        (None, None) => quote!(()),
    };

    Ok(quote! {
        impl ::nodez::typed::NodeType for #ident {
            const ID: &'static str = #id;

            type Output = #produces;

            fn template(
                types: &mut ::nodez::TypeRegistry,
            ) -> ::nodez::NodeTemplate {
                let mut template = ::nodez::NodeTemplate::new(#id, #label)
                    .category(#category)
                    .description(#description)
                    .width(#width);
                #keywords
                #header_color
                #(#schema)*
                #output
                template
            }

            fn from_context<__N: ::nodez::NodeData>(
                ctx: &::nodez::EvalContext<'_, ::nodez::typed::Payload, __N>,
            ) -> ::core::result::Result<Self, ::nodez::typed::NodeError> {
                Ok(Self { #(#reads),* })
            }
        }
    })
}

enum Arity {
    Required,
    Optional,
    Multi,
}

enum Role {
    Param,
    Input { arity: Arity, payload: Box<Type> },
}

struct ParsedField {
    role: Role,
    label: Option<String>,
    description: Option<String>,
    default: Option<Expr>,
    hint: Option<String>,
    min: Option<Expr>,
    max: Option<Expr>,
    show_label: bool,
}

/// A field carrying `#[input]` is a socket; anything else is a plain parameter.
fn parse_field(field: &syn::Field) -> syn::Result<ParsedField> {
    let mut label = None;
    let mut description = None;
    let mut default = None;
    let mut hint = None;
    let mut min: Option<Expr> = None;
    let mut max: Option<Expr> = None;
    let mut show_label = true;
    let mut is_input = false;

    for attr in &field.attrs {
        let input_attr = attr.path().is_ident("input");
        if !input_attr && !attr.path().is_ident("param") {
            continue;
        }
        is_input = input_attr;
        // A bare `#[input]` carries no options.
        if matches!(attr.meta, syn::Meta::Path(_)) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            match key.as_str() {
                "label" => label = Some(meta.value()?.parse::<LitStr>()?.value()),
                "description" => {
                    description = Some(meta.value()?.parse::<LitStr>()?.value());
                }
                "default" => default = Some(meta.value()?.parse()?),
                "hint" => hint = Some(meta.value()?.parse::<LitStr>()?.value()),
                "min" => min = Some(meta.value()?.parse()?),
                "max" => max = Some(meta.value()?.parse()?),
                "hide_label" => show_label = false,
                other => {
                    return Err(meta.error(format!(
                        "unknown field option `{other}`; expected label, description, \
                         default, hint, min, max or hide_label"
                    )));
                }
            }
            Ok(())
        })?;
    }

    let role = if is_input {
        let (arity, payload) = classify(&field.ty);
        Role::Input {
            arity,
            payload: Box::new(payload),
        }
    } else {
        Role::Param
    };

    Ok(ParsedField {
        role,
        label,
        description,
        default,
        hint,
        min,
        max,
        show_label,
    })
}

/// A text box with no hint of its own shows the field's label as placeholder.
fn spec_hint(hint: &Option<String>, label: &str) -> String {
    hint.clone().unwrap_or_else(|| label.to_owned())
}

/// The outermost wrapper is the socket's arity; everything inside is payload.
fn classify(ty: &Type) -> (Arity, Type) {
    if let Some(inner) = wrapped(ty, "Multi") {
        return (Arity::Multi, inner);
    }
    if let Some(inner) = wrapped(ty, "Option") {
        return (Arity::Optional, inner);
    }
    (Arity::Required, ty.clone())
}

fn wrapped(ty: &Type, name: &str) -> Option<Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != name {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    })
}

// ------------------------------------------------------------------ helpers

fn is_type(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(p) if p.path.segments.last().is_some_and(|s| s.ident == name))
}

fn parse_hex(lit: &LitStr) -> syn::Result<(u8, u8, u8)> {
    let text = lit.value();
    let hex = text.strip_prefix('#').unwrap_or(&text);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(syn::Error::new_spanned(
            lit,
            format!("`{text}` is not a #RRGGBB color"),
        ));
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).expect("checked above");
    Ok((byte(0), byte(2), byte(4)))
}

fn shape_variant(shape: Option<&LitStr>) -> syn::Result<Ident> {
    let name = shape.map(|s| s.value()).unwrap_or_else(|| "circle".to_owned());
    let variant = match name.as_str() {
        "circle" => "Circle",
        "diamond" => "Diamond",
        "diamond_dot" => "DiamondDot",
        "square" => "Square",
        other => {
            return Err(syn::Error::new_spanned(
                shape.expect("named above"),
                format!(
                    "unknown shape `{other}`; expected circle, diamond, diamond_dot or square"
                ),
            ));
        }
    };
    Ok(format_ident!("{}", variant))
}

fn kebab_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn snake_case(name: &str) -> String {
    kebab_case(name).replace('-', "_")
}

fn title_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 2);
    let mut capitalize = true;
    for c in name.chars() {
        if c == '_' {
            out.push(' ');
            capitalize = true;
        } else if capitalize {
            out.extend(c.to_uppercase());
            capitalize = false;
        } else {
            out.push(c);
        }
    }
    out
}
