use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Expr, Ident, LitStr, Meta, parse_macro_input};

struct ToolAttr {
    name: LitStr,
    handler: Ident,
}

struct SkillMeta {
    module: Option<Ident>,
    description: String,
    prompt: String,
    tools: Vec<ToolAttr>,
}

#[proc_macro_derive(AgentSkill, attributes(skill, tools))]
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

        // --------------------------------------------------------------------
        // Формирование общего системного промпта для каждого скилла
        // --------------------------------------------------------------------
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

        // 1. Формируем элементы для skills_list()
        skills_list_arms.push(quote! {
            ::osy_share::Skill::new(
                str!(#enum_name::#variant_name),
                #description,
                #final_prompt,
            )
        });

        // 2. Формируем match-ветку для tools_list()
        tools_list_arms.push(quote! {
            Self::#variant_name => #module::tools_list(),
        });

        // 3. Формируем match-ветки для tool_call()
        let mut tool_match_arms = Vec::new();
        for tool in meta.tools {
            let tool_str = &tool.name; // LitStr: "infra_sync_config"
            let handler_fn = &tool.handler; // Ident: handle_infra_sync_config

            tool_match_arms.push(quote! {
                #tool_str => #module::#handler_fn(tx.clone(), ::serde_json::from_value(payload)?).await,
            });
        }

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

            async fn tool_call(&self, tx: Sender<Bytes>, tool: String, payload: JsonValue) -> Result<()> {
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

    // --- ПРОХОД 1: Извлекаем метаданные skill (module, description, prompt) ---
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
        }
    }

    // --- ПРОХОД 2: Прямое маппирование name -> handle_name ---
    for attr in attrs {
        if attr.path().is_ident("tools") {
            let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
            let nested = attr.parse_args_with(parser).unwrap_or_else(|err| {
                panic!(
                    "Failed to parse #[tools(...)] attribute on variant {}: {}",
                    variant_name, err
                )
            });

            for expr in nested {
                if let Expr::Lit(expr_lit) = expr {
                    if let syn::Lit::Str(name_lit) = expr_lit.lit {
                        let raw_name = name_lit.value().trim().to_string();

                        // Строго: "disk_list" -> "handle_disk_list"
                        let handler_name = format!("handle_{}", raw_name);
                        let handler = Ident::new(&handler_name, name_lit.span());
                        let clean_lit = LitStr::new(&raw_name, name_lit.span());

                        meta.tools.push(ToolAttr {
                            name: clean_lit,
                            handler,
                        });
                    }
                }
            }
        }
    }

    meta
}
