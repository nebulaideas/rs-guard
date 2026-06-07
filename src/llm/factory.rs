//! Provider factory for creating LLM provider instances by name.

use crate::error::DiffguardError;
use crate::llm::{deepseek::DeepSeekClient, Provider};

/// Creates an LLM provider instance based on the given provider name.
///
/// # Arguments
///
/// * `provider_name` — Provider identifier (e.g. `"deepseek"`).
/// * `api_key` — API key for authenticating with the provider.
///
/// # Errors
///
/// Returns [`DiffguardError::Config`] if the provider name is unknown
/// or if the API key contains invalid characters.
pub fn create_provider(provider_name: &str, api_key: &str) -> Result<Provider, DiffguardError> {
    match provider_name {
        "deepseek" => Ok(Provider::DeepSeek(DeepSeekClient::new(api_key)?)),
        other => Err(DiffguardError::Config(format!(
            "Unknown provider: {}. Supported: deepseek",
            other
        ))),
    }
}
