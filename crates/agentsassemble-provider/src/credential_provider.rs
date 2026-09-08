#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ProviderCredentialId {
    DeepSeek,
    Cerebras,
    OpenRouter,
    Vercel,
    LlmGateway,
    TokenRouter,
    CustomApi,
}

impl ProviderCredentialId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::Cerebras => "cerebras",
            Self::OpenRouter => "openrouter",
            Self::Vercel => "vercel",
            Self::LlmGateway => "llmgateway",
            Self::TokenRouter => "tokenrouter",
            Self::CustomApi => "custom_api",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderCredentialId;

    #[test]
    fn secure_store_accounts_match_provider_ids() {
        assert_eq!(ProviderCredentialId::DeepSeek.as_str(), "deepseek");
        assert_eq!(ProviderCredentialId::Cerebras.as_str(), "cerebras");
        assert_eq!(ProviderCredentialId::OpenRouter.as_str(), "openrouter");
        assert_eq!(ProviderCredentialId::Vercel.as_str(), "vercel");
        assert_eq!(ProviderCredentialId::LlmGateway.as_str(), "llmgateway");
        assert_eq!(ProviderCredentialId::TokenRouter.as_str(), "tokenrouter");
        assert_eq!(ProviderCredentialId::CustomApi.as_str(), "custom_api");
    }
}
