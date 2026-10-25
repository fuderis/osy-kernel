use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Expr, Ident, LitStr, Meta, parse_macro_input};

struct ToolMeta {
    name: LitStr,
    description: LitStr,
    handler: Ident,
    action_type: proc_macro2::TokenStream,
    no_action: bool,
}

struct SkillMeta {
    module: Option<Ident>,
    description: String,
    prompt: String,
    tools: Vec<ToolMeta>,
}

#[proc_macro_derive(AgentSkill, attributes(skill, tool))]
pub fn derive_agent_skill(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let enum_name = &input.ident;

    let Data::Enum(data_enum) = input.data else {
        panic!("Skills can only be derived for enums");
    };

    let mut skills_list_arms = Vec::new();
    let mut tools_list_arms = Vec::new();
    let mut tool_call_arms = Vec::new();

    for variant in data_enum.variants {
        let variant_name = &variant.ident;
        let meta = parse_variant_attributes(variant_name, &variant.attrs);

        let module = meta.module.unwrap_or_else(|| {
            panic!(
                "#[skill(module = \"...\")] is required for variant {}",
                variant_name
            )
        });

        let base_prompt = format!(
            "Your domain responsibility: {}.\n\
            If the user request cannot be fulfilled within your tools domain, immediately report that you cannot perform it — do NOT execute random or unrelated functions.",
            meta.description
        );

        let final_prompt = if meta.prompt.trim().is_empty() {
            base_prompt
        } else {
            format!("{}\n\n{}", base_prompt, meta.prompt)
        };

        let description = meta.description;

        // 1. Forming skills_list()
        skills_list_arms.push(quote! {
            ::osy_share::Skill::new(
                str!(#enum_name::#variant_name),
                #description,
                #final_prompt,
            )
        });

        // 2. Generating tools_list()
        let mut tool_typed_tokens = Vec::new();
        let mut tool_match_arms = Vec::new();

        for tool in meta.tools {
            let tool_name_str = tool.name;
            let tool_desc = tool.description;
            let handler_fn = tool.handler;
            let action_type = tool.action_type;
            let no_action = tool.no_action;

            // selecting Tool constructor depends on no_action
            let tool_constructor = if no_action {
                quote! {
                    Tool::new(
                        #tool_name_str,
                        #tool_desc,
                    )
                }
            } else {
                quote! {
                    Tool::typed::<#action_type>(
                        #tool_name_str,
                        #tool_desc,
                    )
                }
            };

            tool_typed_tokens.push(tool_constructor);

            // call processing depending on the availability of the action
            if no_action {
                tool_match_arms.push(quote! {
                    #tool_name_str => #module::#handler_fn(tx.clone()).await,
                });
            } else {
                tool_match_arms.push(quote! {
                    #tool_name_str => #module::#handler_fn(tx.clone(), payload.parse_payload()?).await,
                });
            }
        }

        tools_list_arms.push(quote! {
            Self::#variant_name => vec![
                #(#tool_typed_tokens,)*
            ],
        });

        tool_call_arms.push(quote! {
            Self::#variant_name => match tool.trim() {
                #(#tool_match_arms)*
                _ => Err(Error::UnknownTool(tool).into()),
            },
        });
    }

    let expanded = quote! {
        impl #enum_name {
            pub fn skills_list() -> Vec<::osy_share::Skill> {
                vec![
                    #(#skills_list_arms,)*
                ]
            }
        }

        impl ::osy_share::SkillExt for #enum_name {
            fn tools_list(&self) -> Vec<Tool> {
                match self {
                    #(#tools_list_arms)*
                }
            }

            async fn tool_call(&self, tx: Sender<Bytes>, tool: String, payload: ::osy_share::ToolQuery<::serde_json::Value>) -> Result<()> {
                match self {
                    #(#tool_call_arms)*
                }
            }
        }
    };

    TokenStream::from(expanded)
}

fn parse_variant_attributes(variant_name: &Ident, attrs: &[Attribute]) -> SkillMeta {
    let mut meta = SkillMeta {
        module: None,
        description: String::new(),
        prompt: String::new(),
        tools: Vec::new(),
    };

    for attr in attrs {
        if attr.path().is_ident("skill") {
            let nested = attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .unwrap_or_else(|err| {
                    panic!(
                        "Failed to parse #[skill(...)] attribute on variant {}: {}",
                        variant_name, err
                    )
                });

            for item in nested {
                if let Meta::NameValue(nv) = item {
                    if nv.path.is_ident("module") {
                        if let Expr::Lit(expr_lit) = nv.value {
                            if let syn::Lit::Str(s) = expr_lit.lit {
                                meta.module = Some(Ident::new(&s.value(), s.span()));
                            }
                        }
                    } else if nv.path.is_ident("description") {
                        if let Expr::Lit(expr_lit) = nv.value {
                            if let syn::Lit::Str(s) = expr_lit.lit {
                                meta.description = s.value();
                            }
                        }
                    } else if nv.path.is_ident("prompt") {
                        if let Expr::Lit(expr_lit) = nv.value {
                            if let syn::Lit::Str(s) = expr_lit.lit {
                                meta.prompt = s.value();
                            }
                        }
                    }
                }
            }
        } else if attr.path().is_ident("tool") {
            let nested = attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
                )
                .unwrap_or_else(|err| {
                    panic!(
                        "Failed to parse #[tool(...)] attribute on variant {}: {}",
                        variant_name, err
                    )
                });

            let mut tool_name: Option<LitStr> = None;
            let mut tool_desc: Option<LitStr> = None;
            let mut explicit_action: Option<String> = None;
            let mut no_action = false;

            for item in nested {
                match item {
                    Meta::NameValue(nv) => {
                        if nv.path.is_ident("name") {
                            if let Expr::Lit(expr_lit) = nv.value {
                                if let syn::Lit::Str(s) = expr_lit.lit {
                                    tool_name = Some(s);
                                }
                            }
                        } else if nv.path.is_ident("description") {
                            if let Expr::Lit(expr_lit) = nv.value {
                                if let syn::Lit::Str(s) = expr_lit.lit {
                                    tool_desc = Some(s);
                                }
                            }
                        } else if nv.path.is_ident("action") {
                            if let Expr::Lit(expr_lit) = nv.value {
                                if let syn::Lit::Str(s) = expr_lit.lit {
                                    explicit_action = Some(s.value());
                                }
                            }
                        }
                    }
                    Meta::Path(path) => {
                        if path.is_ident("no_action") {
                            no_action = true;
                        }
                    }
                    _ => {}
                }
            }

            let name_lit = tool_name.expect("#[tool(name = \"...\")] is required");
            let desc_lit = tool_desc.expect("#[tool(description = \"...\")] is required");
            let raw_name = name_lit.value();

            // handler's name is always output as handle_{raw_name}
            let handler_name = format!("handle_{}", raw_name);
            let module_ident = meta
                .module
                .as_ref()
                .expect("Module must be specified before tools");
            let handler = Ident::new(&handler_name, name_lit.span());

            // defining type of action structure:
            // 1. If an explicit action is specified (for example, action = "media::VolumeAction")
            // 2. Otherwise, generate the rule ToolName + Action (PascalCase)
            let action_type = if let Some(custom_action) = explicit_action {
                let parsed_path: syn::Path = syn::parse_str(&custom_action)
                    .unwrap_or_else(|_| panic!("Invalid action path string: `{}`", custom_action));
                quote! { #parsed_path }
            } else {
                let pascal_name: String = raw_name
                    .split('_')
                    .map(|part| {
                        let mut c = part.chars();
                        match c.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().chain(c).collect(),
                        }
                    })
                    .collect();

                let struct_name = format!("{}Action", pascal_name);
                let struct_ident = Ident::new(&struct_name, name_lit.span());
                quote! { #module_ident::#struct_ident }
            };

            meta.tools.push(ToolMeta {
                name: name_lit,
                description: desc_lit,
                handler,
                action_type,
                no_action,
            });
        }
    }

    meta
}
