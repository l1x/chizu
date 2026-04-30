use aws_config::{BehaviorVersion, Region};

use crate::config::ProviderConfig;

fn is_non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn configured_loader(config: &ProviderConfig) -> aws_config::ConfigLoader {
    let mut loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(region) = is_non_empty(config.region.as_deref()) {
        loader = loader.region(Region::new(region.to_string()));
    }
    if let Some(profile) = is_non_empty(config.profile.as_deref()) {
        loader = loader.profile_name(profile);
    }
    loader
}

fn endpoint_url(config: &ProviderConfig) -> Option<&str> {
    is_non_empty(config.endpoint_url.as_deref())
}

pub(crate) async fn new_bedrock_runtime_client(
    config: &ProviderConfig,
) -> aws_sdk_bedrockruntime::Client {
    let shared_config = configured_loader(config).load().await;
    let mut builder = aws_sdk_bedrockruntime::config::Builder::from(&shared_config);
    if let Some(endpoint_url) = endpoint_url(config) {
        builder = builder.endpoint_url(endpoint_url);
    }
    aws_sdk_bedrockruntime::Client::from_conf(builder.build())
}

pub(crate) async fn new_bedrock_agent_runtime_client(
    config: &ProviderConfig,
) -> aws_sdk_bedrockagentruntime::Client {
    let shared_config = configured_loader(config).load().await;
    let mut builder = aws_sdk_bedrockagentruntime::config::Builder::from(&shared_config);
    if let Some(endpoint_url) = endpoint_url(config) {
        builder = builder.endpoint_url(endpoint_url);
    }
    aws_sdk_bedrockagentruntime::Client::from_conf(builder.build())
}
