mod cli;
mod config;
mod observability;

use cli::{
    Command, ConfigInitCmd, ConfigSub, ConfigValidateCmd, EmbedCmd, PlanCmd, QuerySub, SearchCmd,
    SummarizeCmd, TopLevel,
};
use grakno_core::Store;
use observability::{record_store_stats, ObservabilityConfig};
use std::str::FromStr;

fn main() {
    let args: TopLevel = argh::from_env();

    // Initialize observability stack
    let obs_config = ObservabilityConfig {
        service_name: "grakno".into(),
        environment: std::env::var("GRAKNO_ENV").unwrap_or_else(|_| "development".into()),
        otlp_endpoint: args.otlp_endpoint,
        log_format: observability::LogFormat::from_str(&args.log_format)
            .unwrap_or(observability::LogFormat::Pretty),
        sampling_rate: args.sampling_rate,
    };

    let _telemetry_guard = observability::init_observability(&obs_config);

    // Guide doesn't need --repo; handle it before repo resolution
    if matches!(args.command, Command::Guide(_)) {
        cmd_guide();
        return;
    }

    // Require --repo for all commands except guide
    let repo_str = args.repo.as_deref().unwrap_or_else(|| {
        eprintln!("error: --repo is required for this command");
        std::process::exit(1);
    });

    // Canonicalize repo path
    let repo_path = std::path::Path::new(repo_str)
        .canonicalize()
        .unwrap_or_else(|e| {
            eprintln!("error: invalid --repo path '{repo_str}': {e}");
            std::process::exit(1);
        });

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        backend = %args.backend,
        repo = %repo_path.display(),
        "Grakno starting"
    );

    // Handle commands that don't need store
    if let Command::Config(ref cmd) = args.command {
        match &cmd.sub {
            ConfigSub::Init(init) => cmd_config_init(init, &repo_path),
            ConfigSub::Validate(val) => cmd_config_validate(val, &repo_path),
        }
        return;
    }

    // Load configuration file if present
    let _config = match config::Config::find_from(&repo_path) {
        Ok(Some((cfg, path))) => {
            tracing::info!(config_path = %path.display(), "loaded configuration");
            Some(cfg)
        }
        Ok(None) => {
            tracing::debug!("no .grakno.toml found, using defaults");
            None
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to load configuration");
            eprintln!("error: failed to load configuration: {e}");
            std::process::exit(1);
        }
    };

    // Database always lives at <repo>/.grakno/graph.db
    let db_path = repo_path.join(".grakno").join("graph.db");

    // Ensure .grakno directory exists for write commands
    if matches!(args.command, Command::Index(_) | Command::Watch(_)) {
        if let Some(parent) = db_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::error!(error = %e, "failed to create .grakno directory");
                eprintln!("error: failed to create .grakno directory: {e}");
                std::process::exit(1);
            }
        }
    }

    let store = open_store(&args.backend, &db_path.to_string_lossy());

    // Record initial store stats
    if let Ok(stats) = store.stats() {
        record_store_stats(&stats);
    }

    match args.command {
        Command::Index(cmd) => {
            let should_embed = cmd.embed
                || _config
                    .as_ref()
                    .map(|c| c.embedding.enabled)
                    .unwrap_or(false);
            cmd_index(&store, &repo_path, should_embed, _config.as_ref());
        }
        Command::Query(q) => match q.sub {
            QuerySub::Entity(cmd) => cmd_query_entity(&store, &cmd.id),
            QuerySub::Entities(cmd) => cmd_query_entities(&store, cmd.component.as_deref()),
            QuerySub::Routes(cmd) => {
                cmd_query_routes(&store, cmd.task.as_deref(), cmd.entity.as_deref())
            }
        },
        Command::Inspect(cmd) => match cmd.entity_id {
            Some(ref id) => cmd_inspect_entity(&store, id),
            None => cmd_inspect_overview(&store),
        },
        Command::Summarize(cmd) => cmd_summarize(&store, cmd, &repo_path),
        Command::Embed(cmd) => cmd_embed(&store, cmd),
        Command::Search(cmd) => cmd_search(&store, cmd),
        Command::Watch(cmd) => cmd_watch(&store, &repo_path, cmd.debounce_ms),
        Command::Plan(cmd) => cmd_plan(&store, cmd),
        Command::Config(_) => {
            // Already handled above
        }
        Command::Guide(_) => {
            // Already handled above
            unreachable!()
        }
    }
}

fn cmd_config_init(cmd: &ConfigInitCmd, repo_path: &std::path::Path) {
    let path = match cmd.path {
        Some(ref explicit) => {
            let p = std::path::PathBuf::from(explicit);
            if p.is_absolute() {
                p
            } else {
                repo_path.join(p)
            }
        }
        None => repo_path.join(".grakno.toml"),
    };
    let path = path.as_path();

    // Check if file already exists
    if path.exists() && !cmd.force {
        eprintln!("error: config file already exists at {}", path.display());
        eprintln!("       use --force to overwrite");
        std::process::exit(1);
    }

    // Generate default config with comments
    let content = config::Config::default_with_comments();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("error: failed to create directory: {e}");
            std::process::exit(1);
        }
    }

    // Write config file
    match std::fs::write(path, content) {
        Ok(_) => {
            println!("created configuration file: {}", path.display());
            println!("\nedit this file to customize grakno settings:");
            println!("  - index.parallel_workers: number of indexing threads");
            println!("  - query.default_limit: default result count for queries");
            println!("  - query.rerank_weights: scoring signal weights");
            println!("  - llm.default_model: model for summarization");
        }
        Err(e) => {
            eprintln!("error: failed to write config file: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_config_validate(cmd: &ConfigValidateCmd, repo_path: &std::path::Path) {
    let config_result = if let Some(ref path_str) = cmd.path {
        let p = std::path::PathBuf::from(path_str);
        let resolved = if p.is_absolute() { p } else { repo_path.join(p) };
        config::Config::load(&resolved)
    } else {
        config::Config::find_from(repo_path).map(|opt| opt.map(|(cfg, _)| cfg))
    };

    match config_result {
        Ok(Some(config)) => {
            // Config loaded and validated successfully
            println!("configuration is valid");
            println!("\n[index]");
            println!("  parallel_workers: {}", config.index.parallel_workers);
            println!(
                "  exclude_patterns: {} patterns",
                config.index.exclude_patterns.len()
            );
            println!("\n[query]");
            println!("  default_limit: {}", config.query.default_limit);
            println!("\n[query.rerank_weights]");
            println!(
                "  task_route: {:.2}",
                config.query.rerank_weights.task_route
            );
            println!("  keyword: {:.2}", config.query.rerank_weights.keyword);
            println!(
                "  name_match: {:.2}",
                config.query.rerank_weights.name_match
            );
            println!("  vector: {:.2}", config.query.rerank_weights.vector);
            println!(
                "  kind_preference: {:.2}",
                config.query.rerank_weights.kind_preference
            );
            println!("  exported: {:.2}", config.query.rerank_weights.exported);
            println!(
                "  path_match: {:.2}",
                config.query.rerank_weights.path_match
            );
            println!("\n[llm]");
            println!("  default_model: {}", config.llm.default_model);
            println!("  timeout_secs: {}", config.llm.timeout_secs);
            println!("  retry_attempts: {}", config.llm.retry_attempts);
            println!("  max_tokens: {}", config.llm.max_tokens);
            println!("  temperature: {}", config.llm.temperature);
        }
        Ok(None) => {
            eprintln!("error: no configuration file found");
            eprintln!("\nrun `grakno config init` to create a default configuration");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn open_store(backend: &str, db: &str) -> Store {
    match backend {
        #[cfg(feature = "sqlite_usearch")]
        "sqlite" => Store::open(db).unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to open sqlite store");
            eprintln!("error: failed to open sqlite store at {db}: {e}");
            std::process::exit(1);
        }),
        #[cfg(not(feature = "sqlite_usearch"))]
        "sqlite" => {
            tracing::error!("sqlite backend not available");
            eprintln!(
                "error: sqlite backend not available; rebuild with --features sqlite_usearch"
            );
            std::process::exit(1);
        }
        "grafeo" => {
            #[cfg(feature = "grafeo")]
            {
                Store::open_grafeo(db).unwrap_or_else(|e| {
                    tracing::error!(error = %e, "failed to open grafeo store");
                    eprintln!("error: failed to open grafeo store at {db}: {e}");
                    std::process::exit(1);
                })
            }
            #[cfg(not(feature = "grafeo"))]
            {
                tracing::error!("grafeo backend not available");
                eprintln!("error: grafeo backend not available; rebuild with --features grafeo");
                std::process::exit(1);
            }
        }
        other => {
            tracing::error!(backend = %other, "unknown backend");
            eprintln!("error: unknown backend '{other}'; expected 'sqlite' or 'grafeo'");
            std::process::exit(1);
        }
    }
}

#[tracing::instrument(skip(store), fields(path = %path.display()))]
fn cmd_index(
    store: &Store,
    path: &std::path::Path,
    should_embed: bool,
    config: Option<&config::Config>,
) {
    tracing::info!("starting index operation");
    let start = std::time::Instant::now();

    match grakno_index::index_project(store, path) {
        Ok(stats) => {
            let duration = start.elapsed().as_secs_f64();
            tracing::info!(
                duration_seconds = duration,
                crates = stats.crates_found,
                files = stats.files_indexed,
                symbols = stats.symbols_extracted,
                edges = stats.edges_created,
                "index completed successfully"
            );

            // Record metrics
            let m = observability::index_metrics();
            m.files_indexed
                .add(stats.files_indexed as u64, &[("result", "success")]);
            m.files_skipped.add(stats.files_skipped as u64, &[]);
            m.symbols_extracted.add(stats.symbols_extracted as u64, &[]);
            m.edges_created.add(stats.edges_created as u64, &[]);
            m.index_duration.observe(duration, &[("result", "success")]);

            // Update store gauges
            if let Ok(store_stats) = store.stats() {
                record_store_stats(&store_stats);
            }

            println!("indexed successfully:\n{stats}");

            // Generate embeddings if requested
            if should_embed {
                if let Some(cfg) = config {
                    if cfg.embedding.enabled || should_embed {
                        println!("\ngenerating embeddings...");
                        let embed_start = std::time::Instant::now();

                        let embed_config = grakno_summarize::SummarizeConfig::new(
                            cfg.embedding.base_url.clone(),
                            cfg.embedding.api_key.clone(),
                            cfg.embedding.model.clone(),
                        );

                        match grakno_summarize::EmbeddingClient::new(&embed_config) {
                            Ok(client) => {
                                let embed_options =
                                    grakno_summarize::SimpleEmbedOptions { force: false };
                                match grakno_summarize::embed_entities_simple(
                                    store,
                                    &client,
                                    &embed_options,
                                ) {
                                    Ok(embed_stats) => {
                                        let embed_duration = embed_start.elapsed().as_secs_f64();
                                        println!(
                                            "embeddings: {embed_stats} (took {:.2}s)",
                                            embed_duration
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("warning: embedding generation failed: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("warning: failed to create embedding client: {e}");
                            }
                        }
                    }
                } else {
                    eprintln!("warning: embeddings requested but no config found");
                    eprintln!("         create .grakno.toml with [embedding] section or use --embed with config");
                }
            }
        }
        Err(e) => {
            let duration = start.elapsed().as_secs_f64();
            tracing::error!(error = %e, duration_seconds = duration, "index failed");

            let m = observability::index_metrics();
            m.index_duration.observe(duration, &[("result", "error")]);

            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_summarize(store: &Store, cmd: SummarizeCmd, repo_path: &std::path::Path) {
    let config = grakno_summarize::SummarizeConfig::new(cmd.base_url, cmd.api_key, cmd.model);
    let config = grakno_summarize::SummarizeConfig {
        max_tokens: cmd.max_tokens,
        temperature: cmd.temperature,
        ..config
    };
    let options = grakno_summarize::summarizer::SummarizeOptions {
        component: cmd.component,
        force: cmd.force,
        workspace_root: Some(repo_path.to_path_buf()),
    };

    tracing::info!("starting summarization");
    let start = std::time::Instant::now();

    match grakno_summarize::summarize_graph(store, &config, &options) {
        Ok(stats) => {
            let duration = start.elapsed().as_secs_f64();
            tracing::info!(
                duration_seconds = duration,
                source_units = stats.source_units_summarized,
                components = stats.components_summarized,
                errors = stats.errors,
                "summarization completed"
            );
            println!("summarization complete:\n{stats}");
        }
        Err(e) => {
            tracing::error!(error = %e, "summarization failed");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_query_entity(store: &Store, id: &str) {
    match store.get_entity(id) {
        Ok(e) => {
            tracing::debug!(entity_id = %id, "entity found");
            println!("id:        {}", e.id);
            println!("kind:      {}", e.kind);
            println!("name:      {}", e.name);
            if let Some(ref c) = e.component_id {
                println!("component: {c}");
            }
            if let Some(ref p) = e.path {
                println!("path:      {p}");
            }
            if let Some(ref lang) = e.language {
                println!("language:  {lang}");
            }
            if let Some(start) = e.line_start {
                let end = e.line_end.map(|n| n.to_string()).unwrap_or_default();
                println!("lines:     {start}..{end}");
            }
            if let Some(ref vis) = e.visibility {
                println!("visibility: {vis}");
            }
            println!("exported:  {}", e.exported);
        }
        Err(e) => {
            tracing::warn!(entity_id = %id, error = %e, "entity not found");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_query_entities(store: &Store, component: Option<&str>) {
    let entities = match component {
        Some(c) => store.list_entities_by_component(c),
        None => store.list_entities(),
    };
    match entities {
        Ok(list) if list.is_empty() => println!("no entities found"),
        Ok(list) => {
            tracing::debug!(count = list.len(), "listed entities");
            for e in &list {
                println!("{:<12} {}", e.kind, e.id);
            }
            println!("\n{} entities", list.len());
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to list entities");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_query_routes(store: &Store, task: Option<&str>, entity: Option<&str>) {
    let routes = match (task, entity) {
        (Some(t), _) => store.routes_for_task(t),
        (_, Some(e)) => store.routes_for_entity(e),
        (None, None) => {
            eprintln!("error: provide --task or --entity");
            std::process::exit(1);
        }
    };
    match routes {
        Ok(list) if list.is_empty() => println!("no routes found"),
        Ok(list) => {
            tracing::debug!(count = list.len(), "listed routes");
            for r in &list {
                println!(
                    "task={:<16} entity={:<32} priority={}",
                    r.task_name, r.entity_id, r.priority
                );
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to list routes");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_inspect_overview(store: &Store) {
    let version = store.schema_version().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let stats = store.stats().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    match version {
        Some(v) => println!("grakno graph (schema v{v})"),
        None => println!("grakno graph (grafeo backend)"),
    }
    println!("  entities:    {}", stats.entities);
    println!("  edges:       {}", stats.edges);
    println!("  files:       {}", stats.files);
    println!("  summaries:   {}", stats.summaries);
    println!("  task_routes: {}", stats.task_routes);
    println!("  embeddings:  {}", stats.embeddings);

    // Record latest stats
    record_store_stats(&stats);
}

fn cmd_inspect_entity(store: &Store, id: &str) {
    let entity = match store.get_entity(id) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    println!("=== {} ({}) ===", entity.name, entity.kind);
    println!("id: {}", entity.id);
    if let Some(ref p) = entity.path {
        println!("path: {p}");
    }

    let outgoing = store.edges_from(id).unwrap_or_default();
    let incoming = store.edges_to(id).unwrap_or_default();

    if !outgoing.is_empty() {
        println!("\noutgoing edges ({}):", outgoing.len());
        for e in &outgoing {
            println!("  --[{}]--> {}", e.rel, e.dst_id);
        }
    }

    if !incoming.is_empty() {
        println!("\nincoming edges ({}):", incoming.len());
        for e in &incoming {
            println!("  <--[{}]-- {}", e.rel, e.src_id);
        }
    }

    if let Ok(s) = store.get_summary(id) {
        println!("\nsummary: {}", s.short_summary);
        if !s.keywords.is_empty() {
            println!("keywords: {}", s.keywords.join(", "));
        }
    }
}

fn cmd_embed(store: &Store, cmd: EmbedCmd) {
    let config = grakno_summarize::SummarizeConfig::new(cmd.base_url, cmd.api_key, cmd.model);
    let client = match grakno_summarize::EmbeddingClient::new(&config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let options = grakno_summarize::EmbedOptions {
        component: cmd.component,
        force: cmd.force,
    };

    tracing::info!("starting embedding generation");

    match grakno_summarize::embedding::embed_graph(store, &client, &options) {
        Ok(stats) => {
            tracing::info!("embedding completed");
            println!("embedding complete:\n{stats}");
        }
        Err(e) => {
            tracing::error!(error = %e, "embedding failed");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_watch(store: &Store, repo_path: &std::path::Path, debounce_ms: u64) {
    use notify::{Event, RecursiveMode, Watcher};
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let root = repo_path;

    // Initial index
    tracing::info!(path = %root.display(), "running initial index");
    println!("running initial index of {}…", root.display());

    let start = std::time::Instant::now();
    match grakno_index::index_project(store, root) {
        Ok(stats) => {
            let duration = start.elapsed().as_secs_f64();
            tracing::info!(
                duration_seconds = duration,
                files = stats.files_indexed,
                "initial index complete"
            );
            println!("initial index complete:\n{stats}");
        }
        Err(e) => {
            tracing::error!(error = %e, "initial index failed");
            eprintln!("error during initial index: {e}");
            std::process::exit(1);
        }
    }

    let debounce = Duration::from_millis(debounce_ms);
    let (tx, rx) = mpsc::channel::<Event>();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .unwrap_or_else(|e| {
        eprintln!("error: failed to create watcher: {e}");
        std::process::exit(1);
    });

    watcher
        .watch(root, RecursiveMode::Recursive)
        .unwrap_or_else(|e| {
            eprintln!("error: failed to watch '{}': {e}", root.display());
            std::process::exit(1);
        });

    tracing::info!(
        path = %root.display(),
        debounce_ms = debounce_ms,
        "watch mode started"
    );
    println!(
        "watching {} (debounce {}ms, Ctrl+C to stop)",
        root.display(),
        debounce_ms
    );

    let relevant_ext = |p: &Path| -> bool {
        matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("rs" | "toml" | "md" | "tf" | "tla" | "astro" | "sql" | "yml" | "yaml" | "html")
        ) || p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains("Dockerfile"))
    };

    let ignored = |p: &Path| -> bool {
        for component in p.components() {
            let s = component.as_os_str().to_string_lossy();
            if s == "target" || s == ".git" || s == "node_modules" {
                return true;
            }
        }
        matches!(p.extension().and_then(|e| e.to_str()), Some("db"))
    };

    loop {
        // Block until we get the first relevant event
        loop {
            match rx.recv() {
                Ok(event) => {
                    tracing::trace!(?event, "file system event");
                    if event.paths.iter().any(|p| !ignored(p) && relevant_ext(p)) {
                        break;
                    }
                }
                Err(_) => {
                    // Channel closed, watcher dropped
                    return;
                }
            }
        }

        // Debounce: drain events for the debounce window
        let deadline = Instant::now() + debounce;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(_) => {} // coalesce
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }

        tracing::info!("change detected, re-indexing");
        println!("\nchange detected, re-indexing…");

        let start = std::time::Instant::now();
        match grakno_index::index_project(store, root) {
            Ok(stats) => {
                let duration = start.elapsed().as_secs_f64();
                tracing::info!(
                    duration_seconds = duration,
                    files = stats.files_indexed,
                    skipped = stats.files_skipped,
                    "re-index complete"
                );
                println!("re-index complete:\n{stats}");
            }
            Err(e) => {
                tracing::error!(error = %e, "re-index failed");
                eprintln!("re-index error: {e}");
            }
        }
    }
}

#[tracing::instrument(skip(store), fields(query = %cmd.query))]
fn cmd_plan(store: &Store, cmd: PlanCmd) {
    let category_override = cmd.category.as_ref().map(|c| {
        c.parse::<grakno_query::TaskCategory>().unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(1);
        })
    });

    let config = grakno_query::PipelineConfig {
        limit: cmd.limit,
        category_override,
        ..Default::default()
    };

    tracing::info!("starting query plan");
    let start = std::time::Instant::now();

    // Optionally embed the query if all three embedding options are provided
    let query_embedding = match (&cmd.base_url, &cmd.api_key, &cmd.model) {
        (Some(base_url), Some(api_key), Some(model)) => {
            let embed_config = grakno_summarize::SummarizeConfig::new(
                base_url.clone(),
                api_key.clone(),
                model.clone(),
            );
            match grakno_summarize::EmbeddingClient::new(&embed_config) {
                Ok(client) => match client.embed(&[&cmd.query]) {
                    Ok(mut vecs) if !vecs.is_empty() => {
                        tracing::debug!("query embedding successful");
                        Some(vecs.remove(0))
                    }
                    Ok(_) => {
                        tracing::warn!("embedding returned no vectors");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "embedding failed");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create embedding client");
                    None
                }
            }
        }
        _ => None,
    };

    let used_vector_search = query_embedding.is_some();
    let embedding_ref = query_embedding.as_deref();

    match grakno_query::QueryPipeline::run(store, &cmd.query, embedding_ref, &config) {
        Ok(plan) => {
            let duration = start.elapsed().as_secs_f64();

            // Record metrics
            let m = observability::query_metrics();
            m.queries_total.add(
                1,
                &[
                    ("category", plan.category.as_str()),
                    (
                        "used_vector",
                        if used_vector_search { "true" } else { "false" },
                    ),
                ],
            );
            m.query_duration
                .observe(duration, &[("category", plan.category.as_str())]);
            m.candidates_considered
                .observe(plan.candidates_considered as f64, &[]);
            if used_vector_search {
                m.vector_searches.add(1, &[]);
            }

            tracing::info!(
                duration_seconds = duration,
                category = %plan.category,
                candidates = plan.candidates_considered,
                results = plan.items.len(),
                used_vector_search,
                "query completed"
            );

            match cmd.format.as_str() {
                "json" => {
                    let json = serde_json::to_string_pretty(&plan).unwrap_or_else(|e| {
                        eprintln!("error serializing plan: {e}");
                        std::process::exit(1);
                    });
                    println!("{json}");
                }
                _ => {
                    print!("{}", plan.display());
                }
            }
        }
        Err(e) => {
            let duration = start.elapsed().as_secs_f64();
            tracing::error!(error = %e, duration_seconds = duration, "query failed");

            let m = observability::query_metrics();
            m.query_duration.observe(duration, &[("result", "error")]);

            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn cmd_search(store: &Store, cmd: SearchCmd) {
    let config = grakno_summarize::SummarizeConfig::new(cmd.base_url, cmd.api_key, cmd.model);
    let client = match grakno_summarize::EmbeddingClient::new(&config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    tracing::info!(query = %cmd.query, k = cmd.k, "starting semantic search");
    let start = std::time::Instant::now();

    match grakno_summarize::embedding::search(store, &client, &cmd.query, cmd.k) {
        Ok(results) if results.is_empty() => {
            tracing::info!("search returned no results");
            println!("no results found");
        }
        Ok(results) => {
            let duration = start.elapsed().as_secs_f64();
            tracing::info!(
                duration_seconds = duration,
                count = results.len(),
                "search completed"
            );

            for (i, r) in results.iter().enumerate() {
                println!("{}. {}  (distance: {:.3})", i + 1, r.entity_id, r.distance);
                let location = match (&r.path, r.line_start) {
                    (Some(p), Some(l)) => format!("{p}:{l}"),
                    (Some(p), None) => p.clone(),
                    _ => String::new(),
                };
                if !location.is_empty() {
                    println!("   [{}] {}", r.entity_kind, location);
                } else {
                    println!("   [{}]", r.entity_kind);
                }
                if !r.short_summary.is_empty() {
                    println!("   {}", r.short_summary);
                }
                println!();
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "search failed");
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}


fn cmd_guide() {
    println!(r#"
╔══════════════════════════════════════════════════════════════════════════════╗
║                         GRAKNO AGENT GUIDE                                   ║
║             How to use grakno effectively in your workflow                   ║
╚══════════════════════════════════════════════════════════════════════════════╝

WHAT IS GRAKNO?
═══════════════
Grakno builds a knowledge graph from your codebase: symbols, docs, infra configs,
and their relationships. It helps you navigate large codebases by understanding
structure, not just text.

NOTE: Most commands require --repo <path> to specify the repository root.

┌──────────────────────────────────────────────────────────────────────────────┐
│  QUICK START (5 minutes)                                                     │
└──────────────────────────────────────────────────────────────────────────────┘

  1. INDEX your codebase (one-time setup):

     $ grakno --repo . index --embed

     This creates .grakno/graph.db with entities and relationships.
     The --embed flag generates vectors for semantic search.

  2. QUERY to understand:

     $ grakno --repo . plan "how does auth work"
     $ grakno --repo . search "error handling patterns"

  3. INSPECT to drill down:

     $ grakno --repo . inspect <entity-id>

  4. WATCH to stay updated (optional):

     $ grakno --repo . watch     # Auto-reindex on file changes

┌──────────────────────────────────────────────────────────────────────────────┐
│  PLAN vs SEARCH: When to use which                                           │
└──────────────────────────────────────────────────────────────────────────────┘

  ┌─────────────┬─────────────────────────────────────────────────────────────┐
  │ PLAN        │ SEARCH                                                      │
  ├─────────────┼─────────────────────────────────────────────────────────────┤
  │ Use for:    │ Use for:                                                    │
  │ • Finding   │ • Finding by meaning                                        │
  │   relevant  │ • "Similar to X"                                            │
  │   files for │ • Exploring patterns                                        │
  │   a task    │ • Requires embeddings                                       │
  ├─────────────┼─────────────────────────────────────────────────────────────┤
  │ grakno      │ grakno --repo . search "how errors propagate"               │
  │  --repo .   │                                                             │
  │  plan       │                                                             │
  │  "refactor  │                                                             │
  │   the API"  │                                                             │
  ├─────────────┼─────────────────────────────────────────────────────────────┤
  │ Returns:    │ Returns:                                                    │
  │ Structured  │ Ranked list by semantic                                     │
  │ reading list│ similarity                                                  │
  │ with scores │                                                             │
  │ & reasons   │                                                             │
  └─────────────┴─────────────────────────────────────────────────────────────┘

  KEY INSIGHT:
  • PLAN combines multiple signals: keywords, names, task routes, and vectors
  • SEARCH is pure semantic similarity over embeddings
  • PLAN is better for task-oriented exploration
  • SEARCH is better for "find similar things"

┌──────────────────────────────────────────────────────────────────────────────┐
│  TASK ROUTES: How plan knows what to look for                                │
└──────────────────────────────────────────────────────────────────────────────┘

  Task routes are heuristic mappings from intent keywords to entity types.
  They help PLAN prioritize the right kinds of entities.

  Intent keyword    →  Prioritized entity types
  ─────────────────────────────────────────────────
  understand, learn → Symbol, Doc, SourceUnit
  deploy, release   → InfraRoot, Containerized, Task
  test, verify      → Test, SourceUnit
  fix, debug        → Symbol, SourceUnit, Doc
  refactor          → Symbol, SourceUnit
  optimize          → Symbol, SourceUnit

  VIEW ROUTES:
    $ grakno --repo . query routes               # All task routes
    $ grakno --repo . query routes --task deploy  # Routes for "deploy" intent

┌──────────────────────────────────────────────────────────────────────────────┐
│  EFFECTIVE QUERY PATTERNS                                                    │
└──────────────────────────────────────────────────────────────────────────────┘

  PLAN QUERIES (task-oriented):
  ─────────────────────────────
  "how does the auth system work"
  "where is the database connection pool configured"
  "find all API endpoints related to users"
  "what needs to change to add rate limiting"
  "how do I deploy this service"

  SEARCH QUERIES (semantic):
  ───────────────────────────
  "error handling patterns"
  "database connection retry logic"
  "configuration validation"
  "async task queue implementation"

  INSPECT QUERIES (deep dive):
  ─────────────────────────────
  $ grakno --repo . inspect symbol:my_function    # Function details
  $ grakno --repo . inspect doc:README            # Document content
  $ grakno --repo . inspect                       # Graph overview

┌──────────────────────────────────────────────────────────────────────────────┐
│  DAILY DEVELOPMENT WORKFLOW                                                  │
└──────────────────────────────────────────────────────────────────────────────┘

  NEW TASK:
  ─────────
  1. Start with PLAN to get oriented:
     $ grakno --repo . plan "implement feature X"

  2. INSPECT the most relevant entities:
     $ grakno --repo . inspect <entity-id>

  3. Make changes, then verify with SEARCH:
     $ grakno --repo . search "similar implementations"

  4. Run tests to validate

  ONGOING WORK:
  ─────────────
  • Keep grakno watch running in a terminal:
    $ grakno --repo . watch

  • Before major changes, use PLAN to identify impact:
    $ grakno --repo . plan "refactor the database layer"

  • Use SUMMARIZE for high-level overviews:
    $ grakno --repo . summarize --component api

┌──────────────────────────────────────────────────────────────────────────────┐
│  WATCH MODE: Automatic updates                                               │
└──────────────────────────────────────────────────────────────────────────────┘

  Watch mode monitors your filesystem and re-indexes changed files:

    $ grakno --repo . watch                  # Start watching
    $ grakno --repo . watch --debounce 1000  # 1 second debounce

  Best practices:
  • Run in a dedicated terminal/tab
  • Uses 500ms debounce by default (configurable)
  • Only re-indexes changed files, not full rebuild
  • Press Ctrl+C to stop

┌──────────────────────────────────────────────────────────────────────────────┐
│  CONFIGURATION                                                               │
└──────────────────────────────────────────────────────────────────────────────┘

  Create a config file:

    $ grakno --repo . config init

  Key settings in .grakno.toml:
  ─────────────────────────────
  [embedding]
  provider = "ollama"           # or "openai"
  model = "nomic-embed-text-v2-moe"

  [query]
  default_limit = 20            # Default result count

  [llm]
  provider = "ollama"           # For summarize command
  model = "llama3.2"

┌──────────────────────────────────────────────────────────────────────────────┐
│  EMBEDDINGS: When you need them                                              │
└──────────────────────────────────────────────────────────────────────────────┘

  • SEARCH requires embeddings (index with --embed)
  • PLAN uses embeddings if available, works without them
  • Embeddings enable semantic similarity matching
  • Without embeddings, PLAN relies on text/structure signals

  To add embeddings to existing index:
    $ grakno --repo . embed

┌──────────────────────────────────────────────────────────────────────────────┐
│  TROUBLESHOOTING                                                             │
└──────────────────────────────────────────────────────────────────────────────┘

  "No results found"
  → Check if you've indexed: grakno --repo . index --embed
  → Try broader search terms

  "No embeddings found"
  → Index with --embed flag
  → Or run: grakno --repo . embed

  "Database not found"
  → Run grakno --repo . index first
  → Check that --repo points to the correct directory

  Results not relevant
  → Try PLAN instead of SEARCH for task-oriented queries
  → Check task routes: grakno --repo . query routes

┌──────────────────────────────────────────────────────────────────────────────┐
│  COMMAND REFERENCE                                                           │
└──────────────────────────────────────────────────────────────────────────────┘

  grakno --repo <path> index [--embed]      Index codebase, optionally with embeddings
  grakno --repo <path> plan "query"         Get structured reading plan
  grakno --repo <path> search "query"       Semantic search (needs embeddings)
  grakno --repo <path> inspect [entity-id]  Inspect entity or show overview
  grakno --repo <path> query entities       List all entities
  grakno --repo <path> query routes         Show task routes
  grakno --repo <path> summarize            Generate summary
  grakno --repo <path> embed                Generate embeddings for existing index
  grakno --repo <path> watch                Auto-reindex on changes
  grakno --repo <path> config init          Create config file

══════════════════════════════════════════════════════════════════════════════

Remember: grakno.plan is for task-oriented discovery, grakno.search is for
semantic similarity. Start with plan, drill down with inspect.

For more details: grakno --help
"#);
}
