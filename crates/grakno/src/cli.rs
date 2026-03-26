use argh::FromArgs;

/// Grakno — a code knowledge graph
#[derive(FromArgs)]
pub struct TopLevel {
    /// path to the repository root (required for all commands except guide)
    #[argh(option)]
    pub repo: Option<String>,

    /// storage backend: sqlite or grafeo (default: sqlite)
    #[argh(option, default = "String::from(\"sqlite\")")]
    pub backend: String,

    /// log format: pretty or json (default: pretty)
    #[argh(option, default = "String::from(\"pretty\")")]
    pub log_format: String,

    /// OTLP endpoint for traces/metrics/logs (optional)
    #[argh(option)]
    pub otlp_endpoint: Option<String>,

    /// trace sampling rate 0.0-1.0 (default: 1.0)
    #[argh(option)]
    pub sampling_rate: Option<f64>,

    #[argh(subcommand)]
    pub command: Command,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
pub enum Command {
    Index(IndexCmd),
    Query(QueryCmd),
    Inspect(InspectCmd),
    Summarize(SummarizeCmd),
    Embed(EmbedCmd),
    Search(SearchCmd),
    Watch(WatchCmd),
    Plan(PlanCmd),
    Guide(GuideCmd),
    Config(ConfigCmd),
}

/// index a codebase into the graph
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "index")]
pub struct IndexCmd {
    /// generate embeddings for vector search (requires embedding config)
    #[argh(switch, short = 'e')]
    pub embed: bool,
}

/// query the graph
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "query")]
pub struct QueryCmd {
    #[argh(subcommand)]
    pub sub: QuerySub,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
pub enum QuerySub {
    Entity(QueryEntityCmd),
    Entities(QueryEntitiesCmd),
    Routes(QueryRoutesCmd),
}

/// look up a single entity by id
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "entity")]
pub struct QueryEntityCmd {
    /// the entity id
    #[argh(positional)]
    pub id: String,
}

/// list entities
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "entities")]
pub struct QueryEntitiesCmd {
    /// filter by component id
    #[argh(option)]
    pub component: Option<String>,
}

/// list task routes
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "routes")]
pub struct QueryRoutesCmd {
    /// routes for a task name
    #[argh(option)]
    pub task: Option<String>,

    /// routes for an entity id
    #[argh(option)]
    pub entity: Option<String>,
}

/// inspect the graph
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "inspect")]
pub struct InspectCmd {
    /// entity id to inspect in detail
    #[argh(positional)]
    pub entity_id: Option<String>,
}

/// summarize entities using an LLM
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "summarize")]
pub struct SummarizeCmd {
    /// base URL for the OpenAI-compatible API
    #[argh(option)]
    pub base_url: String,

    /// API key for authentication
    #[argh(option)]
    pub api_key: String,

    /// model identifier (e.g. gpt-4o-mini)
    #[argh(option)]
    pub model: String,

    /// maximum tokens in the response (default: 512)
    #[argh(option, default = "512")]
    pub max_tokens: u32,

    /// sampling temperature (default: 0.2)
    #[argh(option, default = "0.2")]
    pub temperature: f32,

    /// only summarize this component
    #[argh(option)]
    pub component: Option<String>,

    /// re-summarize even if up to date
    #[argh(switch)]
    pub force: bool,
}

/// generate embeddings for entity summaries
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "embed")]
pub struct EmbedCmd {
    /// base URL for the OpenAI-compatible API
    #[argh(option)]
    pub base_url: String,

    /// API key for authentication
    #[argh(option)]
    pub api_key: String,

    /// embedding model identifier (e.g. text-embedding-3-small)
    #[argh(option)]
    pub model: String,

    /// only embed this component
    #[argh(option)]
    pub component: Option<String>,

    /// re-embed even if up to date
    #[argh(switch)]
    pub force: bool,
}

/// semantic search over entity embeddings
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "search")]
pub struct SearchCmd {
    /// base URL for the OpenAI-compatible API
    #[argh(option)]
    pub base_url: String,

    /// API key for authentication
    #[argh(option)]
    pub api_key: String,

    /// embedding model identifier (e.g. text-embedding-3-small)
    #[argh(option)]
    pub model: String,

    /// the search query
    #[argh(positional)]
    pub query: String,

    /// number of results to return (default: 10)
    #[argh(option, default = "10")]
    pub k: usize,
}

/// generate a reading plan for a query
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "plan")]
pub struct PlanCmd {
    /// the query to plan for
    #[argh(positional)]
    pub query: String,

    /// maximum results to return (default: 15)
    #[argh(option, default = "15")]
    pub limit: usize,

    /// override task category: understand, debug, build, test, deploy, configure, general
    #[argh(option)]
    pub category: Option<String>,

    /// output format: text or json (default: text)
    #[argh(option, default = "String::from(\"text\")")]
    pub format: String,

    /// base URL for the OpenAI-compatible API (optional, enables vector search)
    #[argh(option)]
    pub base_url: Option<String>,

    /// API key for authentication (optional)
    #[argh(option)]
    pub api_key: Option<String>,

    /// embedding model identifier (optional)
    #[argh(option)]
    pub model: Option<String>,
}

/// watch the workspace and re-index on file changes
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "watch")]
pub struct WatchCmd {
    /// debounce interval in milliseconds (default: 500)
    #[argh(option, default = "500")]
    pub debounce_ms: u64,
}

/// interactive guide for using grakno
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "guide")]
pub struct GuideCmd {}

/// configuration management
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "config")]
pub struct ConfigCmd {
    #[argh(subcommand)]
    pub sub: ConfigSub,
}

#[derive(FromArgs, Debug)]
#[argh(subcommand)]
pub enum ConfigSub {
    Init(ConfigInitCmd),
    Validate(ConfigValidateCmd),
}

/// initialize a new .grakno.toml config file
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "init")]
pub struct ConfigInitCmd {
    /// path to create config (default: <repo>/.grakno.toml)
    #[argh(option)]
    pub path: Option<String>,

    /// overwrite existing config file
    #[argh(switch)]
    pub force: bool,
}

/// validate existing .grakno.toml config file
#[derive(FromArgs, Debug)]
#[argh(subcommand, name = "validate")]
pub struct ConfigValidateCmd {
    /// path to config file (default: search upwards for .grakno.toml)
    #[argh(option)]
    pub path: Option<String>,
}
