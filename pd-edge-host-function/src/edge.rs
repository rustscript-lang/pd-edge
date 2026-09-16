use quote::{format_ident, quote};
use syn::{
    Error, Expr, FnArg, Ident, ItemFn, LitStr, Meta, Pat, PatIdent, ReturnType, Token, Type,
    punctuated::Punctuated,
};

pub(crate) fn expand_pd_edge_host_function(
    attr: Punctuated<Meta, Token![,]>,
    mut item: ItemFn,
) -> Result<proc_macro2::TokenStream, Error> {
    let edge_attr = parse_edge_host_attr(&attr)?;
    let was_async = item.sig.asyncness.is_some();
    let docs = doc_string(&item.attrs);
    if docs.trim().is_empty() {
        return Err(Error::new_spanned(
            &item.sig.ident,
            "#[pd_edge_host_function] requires /// doc comments",
        ));
    }
    if edge_attr.scope.is_some() && !edge_attr.bind_params.is_empty() {
        return Err(Error::new_spanned(
            &item.sig.ident,
            "scoped pd_edge_host_function does not support bind(...); scoped registrations must be self-contained",
        ));
    }

    transform_async_edge_function(&mut item, &edge_attr)?;
    validate_edge_bind_names(&item, &edge_attr.bind_params)?;
    for input in &item.sig.inputs {
        validate_edge_param(input, &edge_attr.bind_params)?;
    }
    validate_edge_return_type(&item.sig.output)?;

    let (wrapper_name, impl_name) = wrapper_and_impl_names(&item.sig.ident);
    if item.sig.ident != impl_name {
        item.sig.ident = impl_name.clone();
    }
    let wrapper = generate_edge_host_binder(&item, &wrapper_name, &edge_attr)?;
    let static_wrapper =
        generate_scoped_edge_host_static_wrapper(&item, &wrapper_name, &edge_attr, was_async)?;
    let descriptor_factory =
        generate_edge_host_descriptor_factory(&item, &wrapper_name, &edge_attr)?;
    let registration = generate_edge_host_registration(&wrapper_name, &edge_attr)?;
    Ok(quote! {
        #item
        #wrapper
        #static_wrapper
        #descriptor_factory
        #registration
    })
}

fn doc_string(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }
            match &attr.meta {
                Meta::NameValue(name_value) => match &name_value.value {
                    syn::Expr::Lit(expr_lit) => match &expr_lit.lit {
                        syn::Lit::Str(value) => Some(value.value().trim().to_string()),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            }
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

struct EdgeHostAttr {
    name: Expr,
    scope: Option<EdgeHostScopeAttr>,
    bind_params: Vec<Ident>,
}

#[derive(Clone, Copy)]
enum EdgeHostScopeAttr {
    Runtime,
    Http,
    HttpExtension,
    Io,
    Transport,
    Mqtt,
    WebSocket,
    WebRtc,
    Proxy,
    Console,
}

fn parse_edge_host_attr(args: &Punctuated<Meta, Token![,]>) -> Result<EdgeHostAttr, Error> {
    let mut name = None;
    let mut scope = None;
    let mut bind_params = Vec::new();

    for meta in args {
        match meta {
            Meta::NameValue(name_value) if name_value.path.is_ident("name") => {
                if name.is_some() {
                    return Err(Error::new_spanned(
                        name_value,
                        "duplicate name argument in #[pd_edge_host_function(...)]",
                    ));
                }
                name = Some(name_value.value.clone());
            }
            Meta::NameValue(name_value) if name_value.path.is_ident("scope") => {
                if scope.is_some() {
                    return Err(Error::new_spanned(
                        name_value,
                        "duplicate scope argument in #[pd_edge_host_function(...)]",
                    ));
                }
                scope = Some(parse_edge_scope(&name_value.value)?);
            }
            Meta::List(list) if list.path.is_ident("bind") => {
                let idents =
                    list.parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated)?;
                bind_params.extend(idents.into_iter());
            }
            other => {
                return Err(Error::new_spanned(
                    other,
                    "expected #[pd_edge_host_function(name = ..., scope = ..., bind(...))]",
                ));
            }
        }
    }

    let Some(name) = name else {
        return Err(Error::new(
            proc_macro2::Span::call_site(),
            "expected #[pd_edge_host_function(name = ..., scope = ..., bind(...))]",
        ));
    };

    Ok(EdgeHostAttr {
        name,
        scope,
        bind_params,
    })
}

fn parse_edge_scope(value: &Expr) -> Result<EdgeHostScopeAttr, Error> {
    let scope_name = match value {
        Expr::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return Err(Error::new_spanned(
                    value,
                    "scope must be one of runtime, http, http_extension, io, transport, mqtt, websocket, webrtc, proxy, or console",
                ));
            };
            if path.path.segments.len() != 1 {
                return Err(Error::new_spanned(
                    value,
                    "scope must be one of runtime, http, http_extension, io, transport, mqtt, websocket, webrtc, proxy, or console",
                ));
            }
            segment.ident.to_string()
        }
        Expr::Lit(expr_lit) => match &expr_lit.lit {
            syn::Lit::Str(value) => value.value(),
            _ => {
                return Err(Error::new_spanned(
                    value,
                    "scope must be one of runtime, http, http_extension, io, transport, mqtt, websocket, webrtc, proxy, or console",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                value,
                "scope must be one of runtime, http, http_extension, io, transport, mqtt, websocket, webrtc, proxy, or console",
            ));
        }
    };

    match scope_name.as_str() {
        "runtime" => Ok(EdgeHostScopeAttr::Runtime),
        "http" => Ok(EdgeHostScopeAttr::Http),
        "http_extension" | "http_extensions" => Ok(EdgeHostScopeAttr::HttpExtension),
        "io" | "io_override" | "io_overrides" => Ok(EdgeHostScopeAttr::Io),
        "transport" => Ok(EdgeHostScopeAttr::Transport),
        "mqtt" => Ok(EdgeHostScopeAttr::Mqtt),
        "websocket" => Ok(EdgeHostScopeAttr::WebSocket),
        "webrtc" => Ok(EdgeHostScopeAttr::WebRtc),
        "proxy" => Ok(EdgeHostScopeAttr::Proxy),
        "console" => Ok(EdgeHostScopeAttr::Console),
        _ => Err(Error::new_spanned(
            value,
            "scope must be one of runtime, http, http_extension, io, transport, mqtt, websocket, webrtc, proxy, or console",
        )),
    }
}

fn edge_scope_tokens(scope: EdgeHostScopeAttr) -> proc_macro2::TokenStream {
    match scope {
        EdgeHostScopeAttr::Runtime => {
            quote!(crate::abi_impl::registry::EdgeHostScope::Runtime)
        }
        EdgeHostScopeAttr::Http => quote!(crate::abi_impl::registry::EdgeHostScope::Http),
        EdgeHostScopeAttr::HttpExtension => {
            quote!(crate::abi_impl::registry::EdgeHostScope::HttpExtension)
        }
        EdgeHostScopeAttr::Io => quote!(crate::abi_impl::registry::EdgeHostScope::Io),
        EdgeHostScopeAttr::Transport => {
            quote!(crate::abi_impl::registry::EdgeHostScope::Transport)
        }
        EdgeHostScopeAttr::Mqtt => quote!(crate::abi_impl::registry::EdgeHostScope::Mqtt),
        EdgeHostScopeAttr::WebSocket => {
            quote!(crate::abi_impl::registry::EdgeHostScope::WebSocket)
        }
        EdgeHostScopeAttr::WebRtc => {
            quote!(crate::abi_impl::registry::EdgeHostScope::WebRtc)
        }
        EdgeHostScopeAttr::Proxy => {
            quote!(crate::abi_impl::registry::EdgeHostScope::Proxy)
        }
        EdgeHostScopeAttr::Console => {
            quote!(crate::abi_impl::registry::EdgeHostScope::Console)
        }
    }
}

fn find_context_param_ident(item: &ItemFn) -> Option<Ident> {
    item.sig.inputs.iter().find_map(|input| {
        let FnArg::Typed(pat_type) = input else {
            return None;
        };
        if !is_edge_context_type(&pat_type.ty) {
            return None;
        }
        match pat_type.pat.as_ref() {
            Pat::Ident(PatIdent { ident, .. }) => Some(ident.clone()),
            _ => None,
        }
    })
}

fn async_scope_prepare_stmt(
    item: &ItemFn,
    attr: &EdgeHostAttr,
) -> Result<proc_macro2::TokenStream, Error> {
    let Some(scope) = attr.scope else {
        return Ok(quote!());
    };
    let requires_prepare = matches!(
        scope,
        EdgeHostScopeAttr::Http | EdgeHostScopeAttr::HttpExtension
    );
    if !requires_prepare {
        return Ok(quote!());
    }
    let Some(context_ident) = find_context_param_ident(item) else {
        return Err(Error::new_spanned(
            &item.sig.ident,
            "async scoped http host functions must accept SharedProxyVmContext",
        ));
    };
    let scope_tokens = edge_scope_tokens(scope);
    let name_expr = &attr.name;
    Ok(quote! {
        crate::abi_impl::prepare_scoped_host_call(
            #context_ident.clone(),
            #scope_tokens,
            #name_expr,
        )
        .await?;
    })
}

fn transform_async_edge_function(item: &mut ItemFn, attr: &EdgeHostAttr) -> Result<(), Error> {
    if item.sig.asyncness.is_none() {
        return Ok(());
    }

    for input in &item.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            return Err(Error::new_spanned(input, "methods are not supported"));
        };
        if is_value_slice_type(&pat_type.ty) {
            return Err(Error::new_spanned(
                &pat_type.ty,
                "async edge host functions do not support raw args; use typed parameters instead",
            ));
        }
        if edge_arg_decoder_kind(&pat_type.ty)
            .ok()
            .is_some_and(edge_arg_decoder_is_borrowed)
        {
            return Err(Error::new_spanned(
                &pat_type.ty,
                "async edge host functions do not support borrowed typed parameters; use owned String, Value, or VmMap inputs",
            ));
        }
    }

    let Some(vm_ident) = find_vm_param_ident(item) else {
        return Err(Error::new_spanned(
            &item.sig.ident,
            "async edge host functions must accept a Vm parameter so the macro can schedule the future",
        ));
    };

    match edge_output_kind(&item.sig.output)? {
        Some(EdgeOutputKind::ResultCallOutcome) => {}
        Some(EdgeOutputKind::CallOutcome) => {
            return Err(Error::new_spanned(
                &item.sig.output,
                "async edge host functions must return Result<CallOutcome, VmError>",
            ));
        }
        None => {
            return Err(Error::new_spanned(
                &item.sig.output,
                "edge host functions must return CallOutcome or Result<CallOutcome, VmError>",
            ));
        }
    }

    let original_block = item.block.clone();
    let prepare_stmt = async_scope_prepare_stmt(item, attr)?;
    let error_type = declared_result_error_type(&item.sig.output)?;
    item.sig.asyncness = None;
    *item.block = syn::parse2(quote!({
        crate::abi_impl::schedule_current_future_call(#vm_ident, async move {
            #prepare_stmt
            // One macro-level return-type normalization: the authored body is
            // awaited into an explicitly typed `VmResult<CallOutcome>`, so the
            // unannotated `Ok(CallOutcome::...)` returns inside edge host
            // bodies never need per-call-site annotations.
            let __pd_edge_body_result: ::vm::VmResult<::vm::CallOutcome> =
                async move #original_block .await;
            let __pd_edge_outcome = match __pd_edge_body_result {
                Ok(outcome) => outcome,
                Err(error) => return Err(error),
            };
            match __pd_edge_outcome {
                ::vm::CallOutcome::Return(values) => Ok(values),
                ::vm::CallOutcome::Halt => Err(<#error_type>::from(::vm::VmError::HostError(
                    "async edge host functions must not return Halt".to_string(),
                ))),
                ::vm::CallOutcome::Yield => Err(<#error_type>::from(::vm::VmError::HostError(
                    "async edge host functions must not return Yield".to_string(),
                ))),
                ::vm::CallOutcome::Pending(_) => Err(<#error_type>::from(::vm::VmError::HostError(
                    "async edge host functions must not return Pending".to_string(),
                ))),
            }
        })
    }))?;
    Ok(())
}

/// Error type of a declared `Result<CallOutcome, E>` edge host function return.
fn declared_result_error_type(output: &ReturnType) -> Result<Type, Error> {
    let ReturnType::Type(_, ty) = output else {
        return Ok(syn::parse_quote!(::vm::VmError));
    };
    match unwrap_result_type_with_error(ty)? {
        Some((inner, error)) if is_call_outcome_type(&inner) => Ok(error),
        _ => Ok(syn::parse_quote!(::vm::VmError)),
    }
}

fn validate_edge_bind_names(item: &ItemFn, bind_params: &[Ident]) -> Result<(), Error> {
    let params = item
        .sig
        .inputs
        .iter()
        .filter_map(|input| match input {
            FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
                Pat::Ident(ident) => Some(ident.ident.to_string()),
                _ => None,
            },
            FnArg::Receiver(_) => None,
        })
        .collect::<Vec<_>>();

    for bind in bind_params {
        if !params.iter().any(|name| name == &bind.to_string()) {
            return Err(Error::new_spanned(
                bind,
                format!(
                    "bind parameter '{}' does not match any function parameter",
                    bind
                ),
            ));
        }
    }

    Ok(())
}

fn validate_edge_param(arg: &FnArg, bind_params: &[Ident]) -> Result<(), Error> {
    let FnArg::Typed(pat_type) = arg else {
        return Err(Error::new_spanned(arg, "methods are not supported"));
    };
    let Pat::Ident(PatIdent { ident, .. }) = pat_type.pat.as_ref() else {
        return Err(Error::new_spanned(
            &pat_type.pat,
            "edge host parameters must use identifier patterns",
        ));
    };

    if is_vm_context_type(&pat_type.ty)
        || is_edge_async_ops_type(&pat_type.ty)
        || is_edge_context_type(&pat_type.ty)
        || is_value_slice_type(&pat_type.ty)
    {
        if bind_params.iter().any(|candidate| candidate == ident) {
            return Err(Error::new_spanned(
                ident,
                "special edge host parameters must not be listed in bind(...)",
            ));
        }
        return Ok(());
    }

    if bind_params.iter().any(|candidate| candidate == ident) {
        return Ok(());
    }

    edge_arg_decoder_kind(&pat_type.ty).map(|_| ())
}

fn validate_edge_return_type(output: &ReturnType) -> Result<(), Error> {
    match edge_output_kind(output)? {
        Some(_) => Ok(()),
        None => Err(Error::new_spanned(
            output,
            "edge host functions must return CallOutcome or Result<CallOutcome, VmError>",
        )),
    }
}

fn generate_edge_host_binder(
    item: &ItemFn,
    wrapper_name: &syn::Ident,
    attr: &EdgeHostAttr,
) -> Result<proc_macro2::TokenStream, Error> {
    let impl_name = &item.sig.ident;
    let vis = &item.vis;
    let name_expr = &attr.name;
    let mut binder_params = Vec::<proc_macro2::TokenStream>::new();
    let mut binder_setup = Vec::<proc_macro2::TokenStream>::new();
    let mut closure_setup = Vec::<proc_macro2::TokenStream>::new();
    let mut call_args = Vec::<proc_macro2::TokenStream>::new();
    let mut extract_stmts = Vec::<proc_macro2::TokenStream>::new();
    let mut arg_index = 0usize;
    let mut raw_args = false;

    binder_params.push(quote!(bind_vm: &mut ::vm::Vm));
    binder_params.push(quote!(bind_context: &crate::abi_impl::SharedProxyVmContext));
    binder_params.push(quote!(bind_async_ops: &crate::abi_impl::SharedVmAsyncOps));

    for input in &item.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            return Err(Error::new_spanned(input, "methods are not supported"));
        };
        let Pat::Ident(PatIdent { ident, .. }) = pat_type.pat.as_ref() else {
            return Err(Error::new_spanned(
                &pat_type.pat,
                "edge host parameters must use identifier patterns",
            ));
        };
        let ty = &pat_type.ty;

        if is_vm_context_type(ty) {
            call_args.push(quote!(vm));
            continue;
        }

        if is_edge_async_ops_type(ty) {
            binder_setup.push(quote!(let #ident = bind_async_ops.clone();));
            closure_setup.push(quote!(let #ident = #ident.clone();));
            call_args.push(quote!(#ident));
            continue;
        }

        if is_edge_context_type(ty) {
            binder_setup.push(quote!(let #ident = bind_context.clone();));
            closure_setup.push(quote!(let #ident = #ident.clone();));
            call_args.push(quote!(#ident));
            continue;
        }

        if is_value_slice_type(ty) {
            raw_args = true;
            call_args.push(quote!(args));
            continue;
        }

        if attr.bind_params.iter().any(|candidate| candidate == ident) {
            binder_params.push(quote!(#ident: #ty));
            binder_setup.push(quote!(let #ident = #ident.clone();));
            closure_setup.push(quote!(let #ident = #ident.clone();));
            call_args.push(quote!(#ident));
            continue;
        }

        let decoder = edge_arg_decoder_kind(ty)?;
        extract_stmts.push(edge_extract_stmt(ident, decoder, arg_index, wrapper_name));
        call_args.push(quote!(#ident));
        arg_index += 1;
    }

    let arity_check = if raw_args {
        None
    } else {
        Some(quote! {
            if args.len() != #arg_index {
                return Err(::vm::VmError::HostError(format!(
                    "expected {} arguments, got {}",
                    #arg_index,
                    args.len()
                )));
            }
        })
    };

    let call_expr = match edge_output_kind(&item.sig.output)? {
        Some(EdgeOutputKind::ResultCallOutcome) => quote!(#impl_name(#(#call_args),*)),
        Some(EdgeOutputKind::CallOutcome) => quote!(Ok(#impl_name(#(#call_args),*))),
        None => {
            return Err(Error::new_spanned(
                &item.sig.output,
                "edge host functions must return CallOutcome or Result<CallOutcome, VmError>",
            ));
        }
    };

    Ok(quote! {
        #[allow(dead_code)]
        #vis fn #wrapper_name(#(#binder_params),*) {
            #(#binder_setup)*
            crate::abi_impl::bind_async_host_handler(bind_vm, bind_async_ops, #name_expr, move |vm, args| {
                #arity_check
                #(#closure_setup)*
                #(#extract_stmts)*
                #call_expr
            });
        }
    })
}

fn generate_scoped_edge_host_static_wrapper(
    item: &ItemFn,
    wrapper_name: &syn::Ident,
    attr: &EdgeHostAttr,
    was_async: bool,
) -> Result<proc_macro2::TokenStream, Error> {
    let Some(scope) = attr.scope else {
        return Ok(quote!());
    };
    let impl_name = &item.sig.ident;
    let static_wrapper_name = format_ident!("__pd_edge_static_{}", wrapper_name);
    let uses_vm = scoped_wrapper_uses_vm(item);
    let scope_tokens = edge_scope_tokens(scope);
    let scope_requires_prepare = matches!(
        scope,
        EdgeHostScopeAttr::Http | EdgeHostScopeAttr::HttpExtension
    );
    let args_only_sync_fast_path = !uses_vm && !was_async;
    let prepare_context_ident = format_ident!("__pd_edge_prepare_context");
    let mut setup_stmts = Vec::<proc_macro2::TokenStream>::new();
    let mut call_args = Vec::<proc_macro2::TokenStream>::new();
    let mut extract_stmts = Vec::<proc_macro2::TokenStream>::new();
    let mut arg_index = 0usize;

    for input in &item.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            return Err(Error::new_spanned(input, "methods are not supported"));
        };
        let Pat::Ident(PatIdent { ident, .. }) = pat_type.pat.as_ref() else {
            return Err(Error::new_spanned(
                &pat_type.pat,
                "edge host parameters must use identifier patterns",
            ));
        };
        let ty = &pat_type.ty;

        if is_vm_context_type(ty) {
            call_args.push(quote!(vm));
            continue;
        }

        if is_edge_async_ops_type(ty) {
            setup_stmts.push(quote!(let #ident = crate::abi_impl::current_async_ops()?;));
            call_args.push(quote!(#ident));
            continue;
        }

        if is_edge_context_type(ty) {
            if args_only_sync_fast_path && scope_requires_prepare {
                setup_stmts.push(quote!(let #ident = #prepare_context_ident.clone();));
            } else {
                setup_stmts.push(quote!(let #ident = crate::abi_impl::current_vm_context()?;));
            }
            call_args.push(quote!(#ident));
            continue;
        }

        if is_value_slice_type(ty) {
            return Err(Error::new_spanned(
                ty,
                "scoped pd_edge_host_function does not support raw args",
            ));
        }

        if attr.bind_params.iter().any(|candidate| candidate == ident) {
            return Err(Error::new_spanned(
                ident,
                "scoped pd_edge_host_function does not support bind(...)",
            ));
        }

        let decoder = edge_arg_decoder_kind(ty)?;
        extract_stmts.push(edge_extract_stmt(ident, decoder, arg_index, wrapper_name));
        call_args.push(quote!(#ident));
        arg_index += 1;
    }

    let _ = u8::try_from(arg_index).map_err(|_| {
        Error::new_spanned(
            &item.sig.ident,
            "edge host functions must have 255 arguments or fewer",
        )
    })?;
    let call_expr = match edge_output_kind(&item.sig.output)? {
        Some(EdgeOutputKind::ResultCallOutcome) => quote!(#impl_name(#(#call_args),*)),
        Some(EdgeOutputKind::CallOutcome) => quote!(Ok(#impl_name(#(#call_args),*))),
        None => {
            return Err(Error::new_spanned(
                &item.sig.output,
                "edge host functions must return CallOutcome or Result<CallOutcome, VmError>",
            ));
        }
    };

    if uses_vm {
        Ok(quote! {
            fn #static_wrapper_name(
                vm: &mut ::vm::Vm,
                args: &[::vm::Value],
            ) -> Result<::vm::CallOutcome, ::vm::VmError> {
                if args.len() != #arg_index {
                    return Err(::vm::VmError::HostError(format!(
                        "expected {} arguments, got {}",
                        #arg_index,
                        args.len()
                    )));
                }
                #(#setup_stmts)*
                let __pd_edge_outcome = {
                    #(#extract_stmts)*
                    #call_expr
                }?;
                Ok(__pd_edge_outcome)
            }
        })
    } else if args_only_sync_fast_path && scope_requires_prepare {
        let name_expr = &attr.name;
        Ok(quote! {
            fn #static_wrapper_name(
                args: &[::vm::Value],
            ) -> Result<::vm::CallOutcome, ::vm::VmError> {
                if args.len() != #arg_index {
                    return Err(::vm::VmError::HostError(format!(
                        "expected {} arguments, got {}",
                        #arg_index,
                        args.len()
                    )));
                }
                let #prepare_context_ident = crate::abi_impl::current_vm_context()?;
                if crate::abi_impl::scoped_host_call_can_run_synchronously(
                    &#prepare_context_ident,
                    #scope_tokens,
                    #name_expr,
                )? {
                    #(#setup_stmts)*
                    #(#extract_stmts)*
                    let __pd_edge_outcome = #call_expr?;
                    return Ok(__pd_edge_outcome);
                }
                let __pd_edge_owned_args = args.to_vec();
                crate::abi_impl::schedule_current_args_future_call(async move {
                    let args = &__pd_edge_owned_args;
                    #(#setup_stmts)*
                    #(#extract_stmts)*
                    crate::abi_impl::prepare_scoped_host_call(
                        #prepare_context_ident.clone(),
                        #scope_tokens,
                        #name_expr,
                    )
                    .await?;
                    let __pd_edge_outcome = #call_expr?;
                    match __pd_edge_outcome {
                        ::vm::CallOutcome::Return(values) => Ok(values),
                        ::vm::CallOutcome::Halt => Err(::vm::VmError::HostError(
                            "sync scoped host fast-path future must not return Halt".to_string(),
                        )),
                        ::vm::CallOutcome::Yield => Err(::vm::VmError::HostError(
                            "sync scoped host fast-path future must not return Yield".to_string(),
                        )),
                        ::vm::CallOutcome::Pending(_) => Err(::vm::VmError::HostError(
                            "sync scoped host fast-path future must not return Pending".to_string(),
                        )),
                    }
                })
            }
        })
    } else {
        Ok(quote! {
            fn #static_wrapper_name(
                args: &[::vm::Value],
            ) -> Result<::vm::CallOutcome, ::vm::VmError> {
                if args.len() != #arg_index {
                    return Err(::vm::VmError::HostError(format!(
                        "expected {} arguments, got {}",
                        #arg_index,
                        args.len()
                    )));
                }
                #(#setup_stmts)*
                let __pd_edge_outcome = {
                    #(#extract_stmts)*
                    #call_expr
                }?;
                Ok(__pd_edge_outcome)
            }
        })
    }
}

/// Generates the core `HostFunctionDescriptor` factory for an edge host
/// function.
///
/// The factory joins the two halves of the contract: the guest schema is
/// derived by the runtime from the function's ABI declaration (the single
/// schema/effect source), and this expansion contributes exactly the runtime
/// binding — the generated static wrapper and the binding class it
/// implements. Scoped functions are the discoverable edge module surface;
/// unscoped functions have no static edge adapter and therefore declare no
/// descriptor.
fn generate_edge_host_descriptor_factory(
    item: &ItemFn,
    wrapper_name: &syn::Ident,
    attr: &EdgeHostAttr,
) -> Result<proc_macro2::TokenStream, Error> {
    let Some(_scope) = attr.scope else {
        return Ok(quote!());
    };
    let descriptor_name = format_ident!("__pd_edge_descriptor_{}", wrapper_name);
    let static_wrapper_name = format_ident!("__pd_edge_static_{}", wrapper_name);
    let name_expr = &attr.name;
    let observed_params = edge_guest_params(item)?;
    let (binding, adapter) = if scoped_wrapper_uses_vm(item) {
        (
            quote!(::vm::host_extension::HostBindingKind::StaticStack),
            quote!(::vm::host_extension::HostAdapterDescriptor::StaticStack(#static_wrapper_name)),
        )
    } else {
        (
            quote!(::vm::host_extension::HostBindingKind::StaticArgs),
            quote!(::vm::host_extension::HostAdapterDescriptor::StaticArgs(#static_wrapper_name)),
        )
    };
    Ok(quote! {
        #[allow(dead_code)]
        fn #descriptor_name() -> ::vm::host_extension::HostFunctionDescriptor {
            crate::abi_impl::descriptor::edge_function_descriptor(
                #name_expr,
                &[#(#observed_params),*],
                #binding,
                #adapter,
            )
        }
    })
}

/// The guest-visible parameter list of an edge host function, in declaration
/// order.
///
/// Only the parameters the guest can pass are listed: the runtime-injected
/// `Vm`/context/async-op parameters and the `bind(...)` injections are not
/// guest arguments. The value type is the wire shape the decoder accepts.
fn edge_guest_params(item: &ItemFn) -> Result<Vec<proc_macro2::TokenStream>, Error> {
    let mut params = Vec::new();
    for input in &item.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            return Err(Error::new_spanned(input, "methods are not supported"));
        };
        let Pat::Ident(PatIdent { ident, .. }) = pat_type.pat.as_ref() else {
            return Err(Error::new_spanned(
                &pat_type.pat,
                "edge host parameters must use identifier patterns",
            ));
        };
        let ty = &pat_type.ty;
        if is_vm_context_type(ty) || is_edge_async_ops_type(ty) || is_edge_context_type(ty) {
            continue;
        }
        let value_type = match edge_arg_decoder_kind(ty)? {
            EdgeArgDecoderKind::String | EdgeArgDecoderKind::StringRef => {
                quote!(String)
            }
            EdgeArgDecoderKind::Int => quote!(Int),
            EdgeArgDecoderKind::Bool => quote!(Bool),
            EdgeArgDecoderKind::Value | EdgeArgDecoderKind::ValueRef => quote!(Unknown),
            EdgeArgDecoderKind::Map | EdgeArgDecoderKind::MapRef => quote!(Map),
        };
        let name = LitStr::new(&ident.to_string(), ident.span());
        params.push(quote!((#name, crate::abi_impl::descriptor::EdgeGuestValueType::#value_type)));
    }
    Ok(params)
}

/// Registers the function in the edge discovery inventory.
///
/// The inventory carries discovery metadata only (scope + descriptor
/// factory); the name, arity, documentation, and adapter all come from the
/// descriptor.
fn generate_edge_host_registration(
    wrapper_name: &syn::Ident,
    attr: &EdgeHostAttr,
) -> Result<proc_macro2::TokenStream, Error> {
    let Some(scope) = attr.scope else {
        return Ok(quote!());
    };
    let entry_name = format_ident!("__pd_edge_registration_{}", wrapper_name);
    let scope_tokens = edge_scope_tokens(scope);
    let descriptor_name = format_ident!("__pd_edge_descriptor_{}", wrapper_name);

    Ok(quote! {
        #[::linkme::distributed_slice(crate::abi_impl::registry::PD_EDGE_HOST_FUNCTIONS)]
        #[allow(non_upper_case_globals)]
        static #entry_name: crate::abi_impl::registry::EdgeHostRegistration =
            crate::abi_impl::registry::EdgeHostRegistration {
                scope: #scope_tokens,
                descriptor: #descriptor_name,
            };
    })
}

fn scoped_wrapper_uses_vm(item: &ItemFn) -> bool {
    item.sig.inputs.iter().any(|input| match input {
        FnArg::Typed(pat_type) => is_vm_context_type(&pat_type.ty),
        FnArg::Receiver(_) => false,
    })
}

fn find_vm_param_ident(item: &ItemFn) -> Option<Ident> {
    item.sig.inputs.iter().find_map(|input| {
        let FnArg::Typed(pat_type) = input else {
            return None;
        };
        if !is_vm_context_type(&pat_type.ty) {
            return None;
        }
        match pat_type.pat.as_ref() {
            Pat::Ident(PatIdent { ident, .. }) => Some(ident.clone()),
            _ => None,
        }
    })
}

fn wrapper_and_impl_names(name: &syn::Ident) -> (syn::Ident, syn::Ident) {
    let original = name.to_string();
    match original.strip_suffix("_impl") {
        Some(prefix) => (
            syn::Ident::new(prefix, name.span()),
            syn::Ident::new(&original, name.span()),
        ),
        None => (
            syn::Ident::new(&original, name.span()),
            syn::Ident::new(&format!("{original}_impl"), name.span()),
        ),
    }
}

#[derive(Clone, Copy)]
enum EdgeArgDecoderKind {
    String,
    StringRef,
    Int,
    Bool,
    Value,
    ValueRef,
    Map,
    MapRef,
}

#[derive(Clone, Copy)]
enum EdgeOutputKind {
    CallOutcome,
    ResultCallOutcome,
}

fn edge_arg_decoder_kind(ty: &Type) -> Result<EdgeArgDecoderKind, Error> {
    match ty {
        Type::Group(group) => edge_arg_decoder_kind(&group.elem),
        Type::Paren(paren) => edge_arg_decoder_kind(&paren.elem),
        Type::Reference(reference) => {
            if reference.mutability.is_some() {
                return Err(Error::new_spanned(
                    ty,
                    "mutable borrowed edge host argument types are not supported",
                ));
            }
            let Some(inner) = type_path_terminal_ident(reference.elem.as_ref()) else {
                return Err(Error::new_spanned(
                    ty,
                    "unsupported borrowed edge host argument type",
                ));
            };
            match inner.as_str() {
                "str" => Ok(EdgeArgDecoderKind::StringRef),
                "Value" => Ok(EdgeArgDecoderKind::ValueRef),
                "VmMap" => Ok(EdgeArgDecoderKind::MapRef),
                other => Err(Error::new_spanned(
                    ty,
                    format!("unsupported borrowed edge host argument type '&{other}'"),
                )),
            }
        }
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return Err(Error::new_spanned(
                    ty,
                    "unsupported edge host argument type",
                ));
            };
            match segment.ident.to_string().as_str() {
                "String" => Ok(EdgeArgDecoderKind::String),
                "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => {
                    Ok(EdgeArgDecoderKind::Int)
                }
                "bool" => Ok(EdgeArgDecoderKind::Bool),
                "Value" => Ok(EdgeArgDecoderKind::Value),
                "VmMap" => Ok(EdgeArgDecoderKind::Map),
                other => Err(Error::new_spanned(
                    ty,
                    format!("unsupported edge host argument type '{other}'"),
                )),
            }
        }
        _ => Err(Error::new_spanned(
            ty,
            "unsupported edge host argument type",
        )),
    }
}

fn edge_extract_stmt(
    ident: &Ident,
    decoder: EdgeArgDecoderKind,
    arg_index: usize,
    wrapper_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    let label = LitStr::new(
        &format!("{} {}", wrapper_name, ident),
        proc_macro2::Span::call_site(),
    );
    let index = syn::Index::from(arg_index);
    match decoder {
        EdgeArgDecoderKind::String => quote! {
            let #ident = match args.get(#index) {
                Some(::vm::Value::String(value)) => value.to_string(),
                Some(_) => return Err(::vm::VmError::TypeMismatch("string")),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::StringRef => quote! {
            let #ident = match args.get(#index) {
                Some(::vm::Value::String(value)) => value.as_str(),
                Some(_) => return Err(::vm::VmError::TypeMismatch("string")),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::Int => quote! {
            let #ident = match args.get(#index) {
                Some(::vm::Value::Int(value)) => *value,
                Some(_) => return Err(::vm::VmError::TypeMismatch("int")),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::Bool => quote! {
            let #ident = match args.get(#index) {
                Some(::vm::Value::Bool(value)) => *value,
                Some(_) => return Err(::vm::VmError::TypeMismatch("bool")),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::Value => quote! {
            let #ident = match args.get(#index) {
                Some(value) => value.clone(),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::ValueRef => quote! {
            let #ident = match args.get(#index) {
                Some(value) => value,
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::Map => quote! {
            let #ident = match args.get(#index) {
                Some(::vm::Value::Map(entries)) => entries.as_ref().clone(),
                Some(_) => return Err(::vm::VmError::TypeMismatch("map")),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
        EdgeArgDecoderKind::MapRef => quote! {
            let #ident = match args.get(#index) {
                Some(::vm::Value::Map(entries)) => entries.as_ref(),
                Some(_) => return Err(::vm::VmError::TypeMismatch("map")),
                None => {
                    return Err(::vm::VmError::HostError(format!(
                        "missing argument: {}",
                        #label
                    )));
                }
            };
        },
    }
}

fn edge_arg_decoder_is_borrowed(decoder: EdgeArgDecoderKind) -> bool {
    matches!(
        decoder,
        EdgeArgDecoderKind::StringRef | EdgeArgDecoderKind::ValueRef | EdgeArgDecoderKind::MapRef
    )
}

fn type_path_terminal_ident(ty: &Type) -> Option<String> {
    match ty {
        Type::Group(group) => type_path_terminal_ident(&group.elem),
        Type::Paren(paren) => type_path_terminal_ident(&paren.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn edge_output_kind(output: &ReturnType) -> Result<Option<EdgeOutputKind>, Error> {
    match output {
        ReturnType::Default => Ok(None),
        ReturnType::Type(_, ty) => {
            if is_call_outcome_type(ty) {
                return Ok(Some(EdgeOutputKind::CallOutcome));
            }
            if is_host_call_result_type(ty) {
                return Ok(Some(EdgeOutputKind::ResultCallOutcome));
            }
            if let Some(inner) = unwrap_result_type(ty)?
                && is_call_outcome_type(&inner)
            {
                return Ok(Some(EdgeOutputKind::ResultCallOutcome));
            }
            Ok(None)
        }
    }
}

fn unwrap_result_type(ty: &Type) -> Result<Option<Type>, Error> {
    Ok(unwrap_result_type_with_error(ty)?.map(|(ok, _)| ok))
}

/// Splits a `Result<T, E>` type into its `Ok` and `Err` types.
fn unwrap_result_type_with_error(ty: &Type) -> Result<Option<(Type, Type)>, Error> {
    match ty {
        Type::Group(group) => unwrap_result_type_with_error(&group.elem),
        Type::Paren(paren) => unwrap_result_type_with_error(&paren.elem),
        Type::Reference(reference) => unwrap_result_type_with_error(&reference.elem),
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return Ok(None);
            };
            if segment.ident != "Result" {
                return Ok(None);
            }
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return Err(Error::new_spanned(
                    &segment.arguments,
                    "Result<T, E> requires generic arguments",
                ));
            };
            let Some(syn::GenericArgument::Type(inner)) = args.args.first() else {
                return Err(Error::new_spanned(
                    args,
                    "Result<T, E> requires a return type argument",
                ));
            };
            let error = match args.args.iter().nth(1) {
                Some(syn::GenericArgument::Type(error)) => error.clone(),
                _ => syn::parse_quote!(::vm::VmError),
            };
            Ok(Some((inner.clone(), error)))
        }
        _ => Ok(None),
    }
}

fn is_call_outcome_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_call_outcome_type(&group.elem),
        Type::Paren(paren) => is_call_outcome_type(&paren.elem),
        Type::Reference(reference) => is_call_outcome_type(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "CallOutcome"),
        _ => false,
    }
}

fn is_host_call_result_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_host_call_result_type(&group.elem),
        Type::Paren(paren) => is_host_call_result_type(&paren.elem),
        Type::Reference(reference) => is_host_call_result_type(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "HostCallResult"),
        _ => false,
    }
}

fn is_vm_context_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_vm_context_type(&group.elem),
        Type::Paren(paren) => is_vm_context_type(&paren.elem),
        Type::Reference(reference) => is_vm_context_type(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Vm"),
        _ => false,
    }
}

fn is_edge_async_ops_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_edge_async_ops_type(&group.elem),
        Type::Paren(paren) => is_edge_async_ops_type(&paren.elem),
        Type::Reference(reference) => is_edge_async_ops_type(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "SharedVmAsyncOps"),
        _ => false,
    }
}

fn is_edge_context_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_edge_context_type(&group.elem),
        Type::Paren(paren) => is_edge_context_type(&paren.elem),
        Type::Reference(reference) => is_edge_context_type(&reference.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "SharedProxyVmContext"),
        _ => false,
    }
}

fn is_value_slice_type(ty: &Type) -> bool {
    match ty {
        Type::Group(group) => is_value_slice_type(&group.elem),
        Type::Paren(paren) => is_value_slice_type(&paren.elem),
        Type::Reference(reference) => matches!(
            reference.elem.as_ref(),
            Type::Slice(slice)
                if matches!(
                    slice.elem.as_ref(),
                    Type::Path(path)
                        if path
                            .path
                            .segments
                            .last()
                            .is_some_and(|segment| segment.ident == "Value")
                )
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_str;

    fn expand(attr: &str, item: &str) -> String {
        let args = syn::parse_str::<syn::Meta>(attr)
            .map(|meta| {
                let mut list = Punctuated::new();
                list.push(meta);
                list
            })
            .expect("test attribute should parse");
        let item: ItemFn = parse_str(item).expect("test function should parse");
        expand_pd_edge_host_function(args, item)
            .expect("expansion should succeed")
            .to_string()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect()
    }

    fn expand_with_args(attr: &str, item: &str) -> String {
        let args =
            syn::parse::Parser::parse_str(Punctuated::<Meta, Token![,]>::parse_terminated, attr)
                .expect("test attribute should parse");
        let item: ItemFn = parse_str(item).expect("test function should parse");
        expand_pd_edge_host_function(args, item)
            .expect("expansion should succeed")
            .to_string()
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect()
    }

    #[test]
    fn async_expansion_normalizes_the_body_result_type() {
        let expanded = expand_with_args(
            r#"name = "runtime::sleep", scope = runtime"#,
            r#"
            /// Sleeps.
            async fn runtime_sleep(_vm: &mut ::vm::Vm, ms: i64) -> Result<::vm::CallOutcome, ::vm::VmError> {
                Ok(::vm::CallOutcome::Return(::vm::CallReturn::none()))
            }
            "#,
        );
        assert!(
            expanded
                .contains("let__pd_edge_body_result:::vm::VmResult<::vm::CallOutcome>=asyncmove"),
            "the authored body must be awaited into an explicitly typed result: {expanded}"
        );
        assert!(
            !expanded.contains("__pd_edge_outcome=(asyncmove"),
            "the untyped await inference must be gone: {expanded}"
        );
    }

    #[test]
    fn async_expansion_preserves_a_declared_custom_error_type() {
        let expanded = expand_with_args(
            r#"name = "runtime::sleep", scope = runtime"#,
            r#"
            /// Sleeps.
            async fn runtime_sleep(_vm: &mut ::vm::Vm, ms: i64) -> Result<::vm::CallOutcome, ::edge::CustomError> {
                Ok(::vm::CallOutcome::Return(::vm::CallReturn::none()))
            }
            "#,
        );
        assert!(
            expanded.contains("<::edge::CustomError>::from(::vm::VmError::HostError"),
            "a declared custom error type must be preserved: {expanded}"
        );
    }

    #[test]
    fn scoped_vm_functions_declare_a_static_stack_descriptor() {
        let expanded = expand_with_args(
            r#"name = "tcp::stream::get_phase", scope = transport"#,
            r#"
            /// Reports the phase.
            fn stream_get_phase_impl(vm: &mut ::vm::Vm, handle: i64) -> ::vm::CallOutcome {
                ::vm::CallOutcome::Return(::vm::CallReturn::none())
            }
            "#,
        );
        assert!(
            expanded.contains("fn__pd_edge_descriptor_stream_get_phase()->::vm::host_extension::HostFunctionDescriptor{"),
            "a scoped function must expose one descriptor factory: {expanded}"
        );
        assert!(
            expanded.contains("::vm::host_extension::HostBindingKind::StaticStack,::vm::host_extension::HostAdapterDescriptor::StaticStack(__pd_edge_static_stream_get_phase)"),
            "the descriptor must carry the stack adapter: {expanded}"
        );
        assert!(
            expanded.contains("descriptor:__pd_edge_descriptor_stream_get_phase"),
            "the discovery inventory must reference the descriptor: {expanded}"
        );
        assert!(
            !expanded.contains("EdgeHostRegistration{scope:crate::abi_impl::registry::EdgeHostScope::Transport,name:"),
            "the inventory must not duplicate the guest name: {expanded}"
        );
    }

    #[test]
    fn args_only_scoped_functions_declare_a_static_args_descriptor() {
        let expanded = expand_with_args(
            r#"name = "io::exists", scope = io"#,
            r#"
            /// Reports existence.
            fn io_exists(path: &str) -> Result<::vm::CallOutcome, ::vm::VmError> {
                Ok(::vm::CallOutcome::Return(::vm::CallReturn::none()))
            }
            "#,
        );
        assert!(
            expanded.contains("::vm::host_extension::HostBindingKind::StaticArgs,::vm::host_extension::HostAdapterDescriptor::StaticArgs(__pd_edge_static_io_exists)"),
            "an args-only function must declare the args adapter: {expanded}"
        );
    }

    #[test]
    fn unscoped_functions_declare_no_edge_adapter_or_descriptor() {
        let expanded = expand(
            r#"name = "test::unscoped""#,
            r#"
            /// Unscoped.
            fn test_unscoped(vm: &mut ::vm::Vm) -> Result<::vm::CallOutcome, ::vm::VmError> {
                Ok(::vm::CallOutcome::Return(::vm::CallReturn::none()))
            }
            "#,
        );
        assert!(
            !expanded.contains("__pd_edge_descriptor_test_unscoped"),
            "an unscoped function has no discoverable edge adapter: {expanded}"
        );
        assert!(
            expanded.contains("bind_async_host_handler"),
            "an unscoped function still binds through its edge binder: {expanded}"
        );
    }
}
