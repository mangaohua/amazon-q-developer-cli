use std::fmt::{Display, Formatter};

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::database::settings::Setting;
use crate::database::Database;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatProvider {
    AmazonQ,
    OpenAI,
    Custom(String),
}

impl Display for ChatProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatProvider::AmazonQ => write!(f, "amazon-q"),
            ChatProvider::OpenAI => write!(f, "openai"),
            ChatProvider::Custom(name) => write!(f, "{}", name),
        }
    }
}

impl From<&str> for ChatProvider {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "amazon-q" | "amazonq" | "q" => ChatProvider::AmazonQ,
            "openai" => ChatProvider::OpenAI,
            _ => ChatProvider::Custom(s.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub provider: ChatProvider,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            provider: ChatProvider::AmazonQ,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            model: "gpt-3.5-turbo".to_string(),
        }
    }
}

impl OpenAiConfig {
    pub async fn save_to_database(&self, database: &mut Database) -> Result<()> {
        database
            .settings
            .set(Setting::OpenAiProvider, self.provider.to_string())
            .await
            .wrap_err("Failed to save provider setting")?;

        database
            .settings
            .set(Setting::OpenAiApiBaseUrl, self.base_url.clone())
            .await
            .wrap_err("Failed to save base URL setting")?;

        if let Some(api_key) = &self.api_key {
            database
                .settings
                .set(Setting::OpenAiApiKey, api_key.clone())
                .await
                .wrap_err("Failed to save API key setting")?;
        }

        database
            .settings
            .set(Setting::OpenAiModel, self.model.clone())
            .await
            .wrap_err("Failed to save model setting")?;

        Ok(())
    }

    pub fn from_database(database: &Database) -> Self {
        let provider = database
            .settings
            .get_string(Setting::OpenAiProvider)
            .map(|s| ChatProvider::from(s.as_str()))
            .unwrap_or(ChatProvider::AmazonQ);

        let base_url = database
            .settings
            .get_string(Setting::OpenAiApiBaseUrl)
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        let api_key = database.settings.get_string(Setting::OpenAiApiKey);

        let model = database
            .settings
            .get_string(Setting::OpenAiModel)
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());

        Self {
            provider,
            base_url,
            api_key,
            model,
        }
    }

    pub fn is_openai_compatible(&self) -> bool {
        !matches!(self.provider, ChatProvider::AmazonQ)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_provider_display() {
        assert_eq!(ChatProvider::AmazonQ.to_string(), "amazon-q");
        assert_eq!(ChatProvider::OpenAI.to_string(), "openai");
        assert_eq!(ChatProvider::Custom("claude".to_string()).to_string(), "claude");
    }

    #[test]
    fn test_chat_provider_from_str() {
        assert_eq!(ChatProvider::from("amazon-q"), ChatProvider::AmazonQ);
        assert_eq!(ChatProvider::from("amazonq"), ChatProvider::AmazonQ);
        assert_eq!(ChatProvider::from("q"), ChatProvider::AmazonQ);
        assert_eq!(ChatProvider::from("openai"), ChatProvider::OpenAI);
        assert_eq!(ChatProvider::from("claude"), ChatProvider::Custom("claude".to_string()));
    }

    #[test]
    fn test_openai_config_default() {
        let config = OpenAiConfig::default();
        assert_eq!(config.provider, ChatProvider::AmazonQ);
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.model, "gpt-3.5-turbo");
        assert!(config.api_key.is_none());
    }

    #[test]
    fn test_is_openai_compatible() {
        let amazon_q_config = OpenAiConfig {
            provider: ChatProvider::AmazonQ,
            ..Default::default()
        };
        assert!(!amazon_q_config.is_openai_compatible());

        let openai_config = OpenAiConfig {
            provider: ChatProvider::OpenAI,
            ..Default::default()
        };
        assert!(openai_config.is_openai_compatible());

        let custom_config = OpenAiConfig {
            provider: ChatProvider::Custom("claude".to_string()),
            ..Default::default()
        };
        assert!(custom_config.is_openai_compatible());
    }
}
