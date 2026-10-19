//! Stable identities for model authors and their direct API price catalogs.
//!
//! Catalog contents can disappear when a source is offline, so local vendor
//! recognition must not depend on the providers returned by the current fetch.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OfficialProvider {
    pub(crate) id: &'static str,
    pub(crate) family: &'static str,
    pub(crate) region: &'static str,
}

const PROVIDERS: &[OfficialProvider] = &[
    provider("openai", "openai", "global"),
    provider("anthropic", "anthropic", "global"),
    provider("deepseek", "deepseek", "global"),
    provider("volcengine", "bytedance", "cn"),
    provider("zai", "zhipuai", "global"),
    provider("zhipuai", "zhipuai", "cn"),
    provider("moonshotai", "moonshotai", "global"),
    provider("moonshotai-cn", "moonshotai", "cn"),
    provider("google", "google", "global"),
    provider("mistral", "mistral", "global"),
    provider("alibaba", "alibaba", "global"),
    provider("alibaba-cn", "alibaba", "cn"),
    provider("xai", "xai", "global"),
    provider("minimax", "minimax", "global"),
    provider("minimax-cn", "minimax", "cn"),
    provider("stepfun", "stepfun", "cn"),
    provider("stepfun-ai", "stepfun", "global"),
    provider("meta", "meta", "global"),
    provider("cohere", "cohere", "global"),
    provider("ai21", "ai21", "global"),
    provider("tencent-tokenhub", "tencent", "cn"),
    provider("xiaomi", "xiaomi", "global"),
];

const fn provider(
    id: &'static str,
    family: &'static str,
    region: &'static str,
) -> OfficialProvider {
    OfficialProvider { id, family, region }
}

/// Accept only exact source IDs. Display names, model authors hosted elsewhere,
/// subscription plans, and similarly named future providers are not source IDs.
pub(crate) fn source_provider(id: &str) -> Option<OfficialProvider> {
    PROVIDERS.iter().copied().find(|provider| provider.id == id)
}

fn family_providers(family: &str) -> Vec<OfficialProvider> {
    PROVIDERS
        .iter()
        .copied()
        .filter(|provider| provider.family == family)
        .collect()
}

fn exact_provider(id: &str) -> Vec<OfficialProvider> {
    source_provider(id).into_iter().collect()
}

/// Exact lowercase catalog IDs retain their own region. Brand names can refer
/// to several official catalogs; callers must keep those candidates separate.
/// Unicode letters are retained so Chinese vendor names never collapse to "".
pub(crate) fn local_provider_aliases(label: &str) -> Vec<OfficialProvider> {
    let label = label.trim();
    if let Some(provider) = source_provider(label) {
        return vec![provider];
    }
    let normalized: String = label
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let family = match normalized.as_str() {
        "openai" | "gpt" | "chatgpt" => "openai",
        "anthropic" | "claude" => "anthropic",
        "deepseek" | "深度求索" => "deepseek",
        "volcengine" | "volcengineark" | "bytedance" | "doubao" | "字节" | "字节跳动"
        | "火山引擎" | "火山方舟" | "豆包" => "bytedance",
        "zai" => return exact_provider("zai"),
        "zhipuai" => return exact_provider("zhipuai"),
        "zhipu" | "glm" | "智谱" | "智谱ai" | "智谱清言" => "zhipuai",
        "moonshotai" | "moonshot" | "kimi" | "月之暗面" => "moonshotai",
        "moonshotaichina" | "moonshotaicn" => return exact_provider("moonshotai-cn"),
        "google" | "googlegemini" | "gemini" | "谷歌" => "google",
        "mistral" | "mistralai" => "mistral",
        "alibaba" | "alibabacloud" | "aliyun" | "dashscope" | "qwen" | "通义" | "通义千问"
        | "阿里" | "阿里巴巴" | "阿里云" => "alibaba",
        "alibabachina" | "alibabacn" => return exact_provider("alibaba-cn"),
        "xai" | "grok" => "xai",
        "minimax" | "海螺" | "稀宇科技" => "minimax",
        "minimaxminimaxio" | "minimaxglobal" => return exact_provider("minimax"),
        "minimaxminimaxcn" | "minimaxchina" | "minimaxcn" => {
            return exact_provider("minimax-cn");
        }
        "stepfun" | "阶跃星辰" => "stepfun",
        "stepfunchina" | "stepfuncn" => return exact_provider("stepfun"),
        "stepfunai" | "stepfunglobal" => return exact_provider("stepfun-ai"),
        "meta" => "meta",
        "cohere" => "cohere",
        "ai21" | "ai21labs" => "ai21",
        "tencent" | "tencenttokenhub" | "hunyuan" | "腾讯" | "混元" | "腾讯混元" => {
            "tencent"
        }
        "xiaomi" | "mimo" | "小米" | "小米mimo" => "xiaomi",
        _ => return Vec::new(),
    };
    family_providers(family)
}

fn owner_family(owner: &str) -> Option<&'static str> {
    let owner = owner.trim().to_ascii_lowercase();
    if let Some(provider) = source_provider(&owner) {
        return Some(provider.family);
    }
    match owner.as_str() {
        "bytedance" | "bytedance-seed" => Some("bytedance"),
        "z-ai" | "zhipu" => Some("zhipuai"),
        "moonshot" => Some("moonshotai"),
        "qwen" => Some("alibaba"),
        "mistralai" | "mistral-ai" => Some("mistral"),
        "meta-llama" => Some("meta"),
        "ai21labs" | "ai21-labs" => Some("ai21"),
        "tencent" | "tencent-hunyuan" | "hunyuan" => Some("tencent"),
        _ => None,
    }
}

fn has_prefix(slug: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| slug.starts_with(prefix))
}

fn has_series(slug: &str, brand: &str) -> bool {
    slug.strip_prefix(brand).is_some_and(|suffix| {
        suffix.is_empty()
            || suffix.starts_with('-')
            || suffix.as_bytes().first().is_some_and(u8::is_ascii_digit)
    })
}

fn own_model_slug(family: &str, slug: &str) -> bool {
    let slug = slug.trim().to_ascii_lowercase();
    let slug = if let Some((owner, model)) = slug.split_once('/') {
        if owner_family(owner) != Some(family) || model.contains('/') {
            return false;
        }
        model
    } else {
        slug.as_str()
    };
    match family {
        "openai" => {
            has_prefix(
                slug,
                &[
                    "gpt-",
                    "chatgpt-",
                    "text-embedding-",
                    "text-davinci-",
                    "davinci-",
                    "babbage-",
                    "omni-moderation-",
                    "whisper-",
                    "tts-",
                    "dall-e-",
                    "sora-",
                ],
            ) || slug.strip_prefix('o').is_some_and(|suffix| {
                let digits = suffix.bytes().take_while(u8::is_ascii_digit).count();
                digits > 0 && (digits == suffix.len() || suffix[digits..].starts_with('-'))
            })
        }
        "anthropic" => slug.starts_with("claude-"),
        "deepseek" => slug.starts_with("deepseek-"),
        "bytedance" => has_prefix(slug, &["doubao-", "seed-"]),
        "zhipuai" => has_prefix(slug, &["glm-", "chatglm-", "codegeex-"]),
        "moonshotai" => has_prefix(slug, &["kimi-", "moonshot-"]),
        "google" => has_prefix(
            slug,
            &[
                "gemini-",
                "gemma-",
                "lyria-",
                "veo-",
                "imagen-",
                "learnlm-",
                "embedding-",
                "text-embedding-",
                "deep-research-",
            ],
        ),
        "mistral" => has_prefix(
            slug,
            &[
                "mistral-",
                "open-mistral-",
                "mixtral-",
                "open-mixtral-",
                "codestral-",
                "devstral-",
                "labs-devstral-",
                "labs-leanstral-",
                "magistral-",
                "ministral-",
                "pixtral-",
                "voxtral-",
            ],
        ),
        "alibaba" => {
            has_series(slug, "qwen")
                || has_series(slug, "qwq")
                || has_series(slug, "qvq")
                || slug.starts_with("tongyi-")
        }
        "xai" => slug.starts_with("grok-"),
        "minimax" => has_series(slug, "minimax") || has_series(slug, "abab"),
        "stepfun" => has_prefix(slug, &["step-", "stepaudio-"]),
        "meta" => slug.starts_with("muse-"),
        "cohere" => has_prefix(
            slug,
            &[
                "command-",
                "c4ai-",
                "aya-",
                "tiny-aya-",
                "north-",
                "embed-",
                "rerank-",
            ],
        ),
        "ai21" => has_prefix(slug, &["jamba-", "j2-", "jurassic-"]),
        "tencent" => has_series(slug, "hy") || slug.starts_with("hunyuan-"),
        "xiaomi" => slug.starts_with("mimo-"),
        _ => false,
    }
}

/// A direct vendor endpoint may also host another author's models. An explicit
/// canonical owner must identify this vendor; without it, accept only known
/// self-authored model families. Never treat a reseller's model name as proof
/// that its endpoint is the original author's price source.
pub(crate) fn is_official_model(
    provider: OfficialProvider,
    model_slug: &str,
    canonical_model_id: Option<&str>,
    subscription: bool,
) -> bool {
    if subscription
        || source_provider(provider.id) != Some(provider)
        || model_slug.trim().is_empty()
    {
        return false;
    }
    if let Some(canonical) = canonical_model_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return canonical.split_once('/').is_some_and(|(owner, model)| {
            !model.trim().is_empty() && owner_family(owner) == Some(provider.family)
        });
    }
    own_model_slug(provider.family, model_slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(providers: Vec<OfficialProvider>) -> Vec<&'static str> {
        providers.into_iter().map(|provider| provider.id).collect()
    }

    #[test]
    fn source_ids_are_exact_and_exclude_hosts_and_subscriptions() {
        for official in PROVIDERS {
            assert_eq!(source_provider(official.id), Some(*official));
        }
        for excluded in [
            "OpenAI",
            " openai",
            "openai-future",
            "azure",
            "azure-cognitive-services",
            "amazon-bedrock",
            "fireworks-ai",
            "openrouter",
            "google-vertex",
            "google-vertex-anthropic",
            "siliconflow",
            "togetherai",
            "groq",
            "cerebras",
            "deepinfra",
            "nvidia",
            "llama",
            "nova",
            "huggingface",
            "vercel",
            "alibaba-coding-plan",
            "alibaba-token-plan-cn",
            "volcengine-coding-plan",
            "zai-coding-plan",
            "kimi-code-plan-cn",
        ] {
            assert_eq!(source_provider(excluded), None, "{excluded}");
        }
    }

    #[test]
    fn local_aliases_remain_stable_without_any_source_catalog() {
        for (alias, expected) in [
            ("OpenAI", vec!["openai"]),
            ("Claude", vec!["anthropic"]),
            ("深度求索", vec!["deepseek"]),
            ("字节跳动", vec!["volcengine"]),
            ("豆包", vec!["volcengine"]),
            ("智谱", vec!["zai", "zhipuai"]),
            ("GLM", vec!["zai", "zhipuai"]),
            ("Kimi", vec!["moonshotai", "moonshotai-cn"]),
            ("月之暗面", vec!["moonshotai", "moonshotai-cn"]),
            ("通义千问", vec!["alibaba", "alibaba-cn"]),
            ("Alibaba", vec!["alibaba", "alibaba-cn"]),
            ("MiniMax", vec!["minimax", "minimax-cn"]),
            ("阶跃星辰", vec!["stepfun", "stepfun-ai"]),
            ("腾讯混元", vec!["tencent-tokenhub"]),
            ("小米", vec!["xiaomi"]),
        ] {
            assert_eq!(ids(local_provider_aliases(alias)), expected, "{alias}");
        }
        for supplier in [
            "董哥",
            "",
            "unknown supplier",
            "OpenAI reseller",
            "Azure",
            "Bedrock",
        ] {
            assert!(local_provider_aliases(supplier).is_empty(), "{supplier}");
        }
    }

    #[test]
    fn exact_ids_and_explicit_regions_never_expand_to_other_catalogs() {
        for official in PROVIDERS {
            assert_eq!(local_provider_aliases(official.id), vec![*official]);
        }
        for (alias, expected) in [
            ("Z.AI", "zai"),
            ("Zhipu AI", "zhipuai"),
            ("Alibaba (China)", "alibaba-cn"),
            ("Moonshot AI (China)", "moonshotai-cn"),
            ("MiniMax (minimax.cn)", "minimax-cn"),
            ("MiniMax (minimax.io)", "minimax"),
            ("StepFun (China)", "stepfun"),
            ("StepFun (Global)", "stepfun-ai"),
        ] {
            assert_eq!(
                ids(local_provider_aliases(alias)),
                vec![expected],
                "{alias}"
            );
        }
        let international = source_provider("moonshotai").unwrap();
        let domestic = source_provider("moonshotai-cn").unwrap();
        assert_eq!(international.family, domestic.family);
        assert_ne!(international, domestic);
        assert_ne!(international.region, domestic.region);
    }

    #[test]
    fn declared_canonical_owners_must_be_original_authors() {
        let bytedance = source_provider("volcengine").unwrap();
        assert!(is_official_model(
            bytedance,
            "official-endpoint-alias",
            Some("bytedance-seed/seed-2.1"),
            false
        ));
        for canonical in [
            "deepseek/deepseek-v4-flash",
            "zhipuai/glm-5.2",
            "unknown/seed-2.1",
            "seed-2.1",
            "bytedance-seed/",
        ] {
            assert!(
                !is_official_model(bytedance, "doubao-seed-2.1", Some(canonical), false),
                "{canonical}"
            );
        }
        assert!(is_official_model(
            source_provider("zai").unwrap(),
            "glm-5.3",
            Some("zhipuai/glm-5.3"),
            false
        ));
    }

    #[test]
    fn missing_owner_requires_an_explicit_self_authored_model_family() {
        for (id, model) in [
            ("openai", "gpt-6-sol"),
            ("openai", "o3"),
            ("anthropic", "claude-sonnet-5"),
            ("deepseek", "deepseek-v4-pro"),
            ("volcengine", "doubao-seed-2-1-pro"),
            ("zai", "glm-5.3"),
            ("zhipuai", "GLM-5.3"),
            ("moonshotai", "kimi-k3"),
            ("moonshotai-cn", "moonshot-v1-128k"),
            ("google", "gemini-3.8-flash"),
            ("mistral", "codestral-latest"),
            ("alibaba", "qwen3.8-plus"),
            ("alibaba-cn", "qwq-32b"),
            ("xai", "grok-4.7"),
            ("minimax", "MiniMax-M3"),
            ("minimax-cn", "MiniMax-H3"),
            ("stepfun", "step-5-preview"),
            ("stepfun-ai", "stepaudio-2.5-tts"),
            ("meta", "muse-spark-1.3"),
            ("cohere", "command-a-03-2025"),
            ("ai21", "jamba-large"),
            ("tencent-tokenhub", "hy4-preview"),
            ("xiaomi", "mimo-v2.6-pro"),
        ] {
            let official = source_provider(id).unwrap();
            assert!(
                is_official_model(official, model, None, false),
                "{id}/{model}"
            );
            assert!(
                !is_official_model(official, model, None, true),
                "{id}/{model}"
            );
        }
    }

    #[test]
    fn original_vendor_endpoints_cannot_supply_other_authors_prices() {
        for (id, hosted) in [
            ("volcengine", "deepseek-v4-flash-ga-260731"),
            ("volcengine", "glm-5-3-flash-260828"),
            ("alibaba", "kimi-k3"),
            ("alibaba-cn", "MiniMax/MiniMax-M2.7"),
            ("alibaba-cn", "siliconflow/deepseek-v3.2"),
            ("mistral", "glm-5-2"),
            ("mistral", "zai-glm-5-3"),
            ("google", "claude-sonnet-5"),
            ("meta", "llama-4-scout-17b"),
            ("meta", "cerebras-llama-4-scout"),
            ("meta", "groq-llama-4-maverick"),
            ("openai", "openaicompatible-model"),
        ] {
            assert!(
                !is_official_model(source_provider(id).unwrap(), hosted, None, false),
                "{id}/{hosted}"
            );
        }
        assert!(!is_official_model(
            provider("openrouter", "openai", "global"),
            "gpt-6-sol",
            Some("openai/gpt-6-sol"),
            false
        ));
    }
}
