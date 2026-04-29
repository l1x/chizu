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

pub(crate) async fn new_bedrock_runtime_client(
    config: &ProviderConfig,
) -> aws_sdk_bedrockruntime::Client {
    let mut loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(region) = is_non_empty(config.region.as_deref()) {
        loader = loader.region(Region::new(region.to_string()));
    }
    if let Some(profile) = is_non_empty(config.profile.as_deref()) {
        loader = loader.profile_name(profile);
    }

    let shared_config = loader.load().await;
    let mut builder = aws_sdk_bedrockruntime::config::Builder::from(&shared_config);
    if let Some(endpoint_url) = is_non_empty(config.endpoint_url.as_deref()) {
        builder = builder.endpoint_url(endpoint_url);
    }
    aws_sdk_bedrockruntime::Client::from_conf(builder.build())
}

pub(crate) async fn new_bedrock_agent_runtime_client(
    config: &ProviderConfig,
) -> aws_sdk_bedrockagentruntime::Client {
    let mut loader = aws_config::defaults(BehaviorVersion::latest());
    if let Some(region) = is_non_empty(config.region.as_deref()) {
        loader = loader.region(Region::new(region.to_string()));
    }
    if let Some(profile) = is_non_empty(config.profile.as_deref()) {
        loader = loader.profile_name(profile);
    }

    let shared_config = loader.load().await;
    let mut builder = aws_sdk_bedrockagentruntime::config::Builder::from(&shared_config);
    if let Some(endpoint_url) = is_non_empty(config.endpoint_url.as_deref()) {
        builder = builder.endpoint_url(endpoint_url);
    }
    aws_sdk_bedrockagentruntime::Client::from_conf(builder.build())
}
