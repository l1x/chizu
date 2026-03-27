use std::fmt;
use std::io::Read;
use std::time::SystemTime;

use chizu_core::model::{EdgeKind, EntityKind};
use chizu_core::Store;

use crate::client::LlmClient;
use crate::config::SummarizeConfig;
use crate::error::{Result, SummarizeError};
use crate::prompt;

/// Stats returned after a summarization run.
pub struct SummarizeStats {
    pub source_units_summarized: usize,
    pub source_units_skipped: usize,
    pub components_summarized: usize,
    pub errors: usize,
}

impl fmt::Display for SummarizeStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "source_units: {} summarized, {} skipped\ncomponents: {} summarized\nerrors: {}",
            self.source_units_summarized,
            self.source_units_skipped,
            self.components_summarized,
            self.errors,
        )
    }
}

/// Options controlling which entities to summarize.
pub struct SummarizeOptions {
    /// Only summarize entities belonging to this component.
    pub component: Option<String>,
    /// Force re-summarization even if up to date.
    pub force: bool,
    /// Workspace root for resolving relative file paths.
    pub workspace_root: Option<std::path::PathBuf>,
}

/// Summarize source units and components in the graph.
pub fn summarize_graph(
    store: &Store,
    config: &SummarizeConfig,
    options: &SummarizeOptions,
) -> Result<SummarizeStats> {
    let client = LlmClient::new(config)?;
    let mut stats = SummarizeStats {
        source_units_summarized: 0,
        source_units_skipped: 0,
        components_summarized: 0,
        errors: 0,
    };

    // Gather components to process
    let components = match &options.component {
        Some(comp_id) => {
            let entity = store.get_entity(comp_id)?;
            vec![entity]
        }
        None => {
            let all = store.list_entities()?;
            all.into_iter()
                .filter(|e| e.kind == EntityKind::Component)
                .collect()
        }
    };

    for component in &components {
        // Pass 1: Summarize source units for this component
        let source_units: Vec<_> = store
            .list_entities_by_component(&component.id)?
            .into_iter()
            .filter(|e| e.kind == EntityKind::SourceUnit)
            .collect();

        for su in &source_units {
            let file_path = match &su.path {
                Some(p) => p.clone(),
                None => continue,
            };

            // Compute current file hash for staleness detection
            let abs_path = match &options.workspace_root {
                Some(root) => root.join(&file_path).display().to_string(),
                None => file_path.clone(),
            };
            let current_hash = compute_file_hash(&abs_path).ok();

            // Check staleness
            if !options.force {
                if let Ok(existing) = store.get_summary(&su.id) {
                    if existing.source_hash.is_some() && existing.source_hash == current_hash {
                        eprintln!("summarizing {}... skipped", su.id);
                        stats.source_units_skipped += 1;
                        continue;
                    }
                }
            }

            eprint!("summarizing {}... ", su.id);

            // Gather symbols defined in this source unit
            let symbols = gather_symbols(store, &su.id);

            let user_prompt = prompt::source_unit_prompt(
                &file_path,
                &component.name,
                su.language.as_deref(),
                &symbols,
            );

            match call_and_store(
                &client,
                store,
                &su.id,
                &user_prompt,
                current_hash.as_deref(),
            ) {
                Ok(()) => {
                    eprintln!("done");
                    stats.source_units_summarized += 1;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    stats.errors += 1;
                }
            }
        }

        // Pass 2: Component roll-up summary
        eprint!("summarizing {}... ", component.id);

        // Collect short summaries of this component's source units
        let su_summaries: Vec<(String, String)> = source_units
            .iter()
            .filter_map(|su| {
                let path = su.path.as_deref().unwrap_or(&su.id);
                store
                    .get_summary(&su.id)
                    .ok()
                    .map(|s| (path.to_string(), s.short_summary))
            })
            .collect();

        // Gather dependency names from DependsOn edges
        let dep_names = gather_dependency_names(store, &component.id);

        let user_prompt = prompt::component_prompt(
            &component.name,
            component.path.as_deref(),
            &dep_names,
            &su_summaries,
        );

        match call_and_store(&client, store, &component.id, &user_prompt, None) {
            Ok(()) => {
                eprintln!("done");
                stats.components_summarized += 1;
            }
            Err(e) => {
                eprintln!("error: {e}");
                stats.errors += 1;
            }
        }
    }

    Ok(stats)
}

fn call_and_store(
    client: &LlmClient,
    store: &Store,
    entity_id: &str,
    user_prompt: &str,
    source_hash: Option<&str>,
) -> Result<()> {
    let raw = client.chat(prompt::SYSTEM_PROMPT, user_prompt)?;
    let parsed = prompt::parse_llm_response(&raw)?;

    let summary = chizu_core::model::Summary {
        entity_id: entity_id.to_string(),
        short_summary: parsed.short,
        detailed_summary: Some(parsed.detailed),
        keywords: parsed.keywords,
        updated_at: now_iso8601(),
        source_hash: source_hash.map(|s| s.to_string()),
    };

    store
        .upsert_summary(&summary)
        .map_err(SummarizeError::Store)
}

fn gather_symbols(store: &Store, source_unit_id: &str) -> Vec<prompt::SymbolInfo> {
    let edges = store.edges_from(source_unit_id).unwrap_or_default();
    let mut symbols = Vec::new();
    for edge in &edges {
        if edge.rel == EdgeKind::Defines || edge.rel == EdgeKind::Contains {
            if let Ok(entity) = store.get_entity(&edge.dst_id) {
                if entity.kind == EntityKind::Symbol {
                    symbols.push(prompt::SymbolInfo {
                        name: entity.name.clone(),
                        kind: entity.kind.to_string(),
                        line_start: entity.line_start,
                    });
                }
            }
        }
    }
    symbols
}

fn gather_dependency_names(store: &Store, component_id: &str) -> Vec<String> {
    let edges = store.edges_from(component_id).unwrap_or_default();
    let mut names = Vec::new();
    for edge in &edges {
        if edge.rel == EdgeKind::DependsOn {
            if let Ok(entity) = store.get_entity(&edge.dst_id) {
                names.push(entity.name.clone());
            }
        }
    }
    names
}

fn compute_file_hash(path: &str) -> std::result::Result<String, std::io::Error> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn now_iso8601() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Manual UTC formatting — avoids chrono dependency
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to Y-M-D (simplified Gregorian)
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's chrono-compatible date library
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_iso8601_format() {
        let ts = now_iso8601();
        // Should match pattern YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        // 1970-01-01
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 = 19723 days since epoch
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }
}
