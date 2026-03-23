use argh::FromArgs;

/// Grakno — a code knowledge graph
#[derive(FromArgs)]
pub struct TopLevel {
    /// path to the database (default: grakno.db)
    #[argh(option, default = "String::from(\"grakno.db\")")]
    pub db: String,

    /// storage backend: sqlite or grafeo (default: sqlite)
    #[argh(option, default = "String::from(\"sqlite\")")]
    pub backend: String,

    #[argh(subcommand)]
    pub command: Command,
}

#[derive(FromArgs)]
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
}

/// index a Rust workspace into the graph
#[derive(FromArgs)]
#[argh(subcommand, name = "index")]
pub struct IndexCmd {
    /// path to the workspace root (default: current directory)
    #[argh(positional, default = "String::from(\".\")")]
    pub path: String,
}

/// query the graph
#[derive(FromArgs)]
#[argh(subcommand, name = "query")]
pub struct QueryCmd {
    #[argh(subcommand)]
    pub sub: QuerySub,
}

#[derive(FromArgs)]
#[argh(subcommand)]
pub enum QuerySub {
    Entity(QueryEntityCmd),
    Entities(QueryEntitiesCmd),
    Routes(QueryRoutesCmd),
}

/// look up a single entity by id
#[derive(FromArgs)]
#[argh(subcommand, name = "entity")]
pub struct QueryEntityCmd {
    /// the entity id
    #[argh(positional)]
    pub id: String,
}

/// list entities
#[derive(FromArgs)]
#[argh(subcommand, name = "entities")]
pub struct QueryEntitiesCmd {
    /// filter by component id
    #[argh(option)]
    pub component: Option<String>,
}

/// list task routes
#[derive(FromArgs)]
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
#[derive(FromArgs)]
#[argh(subcommand, name = "inspect")]
pub struct InspectCmd {
    /// entity id to inspect in detail
    #[argh(positional)]
    pub entity_id: Option<String>,
}

/// summarize entities using an LLM
#[derive(FromArgs)]
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
#[derive(FromArgs)]
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
#[derive(FromArgs)]
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
#[derive(FromArgs)]
#[argh(subcommand, name = "plan")]
pub struct PlanCmd {
    /// the query to plan for
    #[argh(positional)]
    pub query: String,

    /// maximum results to return (default: 15)
    #[argh(option, default = "15")]
    pub limit: usize,

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
#[derive(FromArgs)]
#[argh(subcommand, name = "watch")]
pub struct WatchCmd {
    /// path to the workspace root (default: current directory)
    #[argh(positional, default = "String::from(\".\")")]
    pub path: String,

    /// debounce interval in milliseconds (default: 500)
    #[argh(option, default = "500")]
    pub debounce_ms: u64,
}
