use std::process::Command;

fn chizu_bin() -> &'static str {
    env!("CARGO_BIN_EXE_chizu")
}

fn create_fixture_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let repo = temp_dir.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn auth_handler() {}\n\n#[test]\nfn test_auth() {}\n",
    )
    .unwrap();
    std::fs::write(
        repo.join("Cargo.toml"),
        r#"[package]
name = "fixture"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();
    (temp_dir, repo)
}

#[test]
fn cli_index_creates_graph_db() {
    let (_temp, repo) = create_fixture_repo();

    let output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "index failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(repo.join(".chizu/graph.db").exists());
}

#[test]
fn cli_search_returns_valid_json() {
    let (_temp, repo) = create_fixture_repo();

    let index_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();
    assert!(index_output.status.success());

    let search_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("search")
        .arg("auth")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    assert!(
        search_output.status.success(),
        "search failed: {}",
        String::from_utf8_lossy(&search_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&search_output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("search output should be valid JSON");
    assert!(json.get("entries").is_some());
}

#[test]
fn cli_entity_shows_details() {
    let (_temp, repo) = create_fixture_repo();

    let index_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();
    assert!(index_output.status.success());

    let entity_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("entity")
        .arg("symbol::src/lib.rs::auth_handler")
        .output()
        .unwrap();

    assert!(
        entity_output.status.success(),
        "entity failed: {}",
        String::from_utf8_lossy(&entity_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&entity_output.stdout);
    assert!(stdout.contains("auth_handler"));
    assert!(stdout.contains("symbol"));
}

#[test]
fn cli_entities_filters_by_kind() {
    let (_temp, repo) = create_fixture_repo();

    let index_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();
    assert!(index_output.status.success());

    let entities_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("entities")
        .arg("--kind")
        .arg("test")
        .output()
        .unwrap();

    assert!(
        entities_output.status.success(),
        "entities failed: {}",
        String::from_utf8_lossy(&entities_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&entities_output.stdout);
    assert!(stdout.contains("test"));
    assert!(stdout.contains("test_auth"));
}

#[test]
fn cli_routes_filters_by_task() {
    let (_temp, repo) = create_fixture_repo();

    let index_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();
    assert!(index_output.status.success());

    let routes_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("routes")
        .arg("--task")
        .arg("test")
        .output()
        .unwrap();

    assert!(
        routes_output.status.success(),
        "routes failed: {}",
        String::from_utf8_lossy(&routes_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&routes_output.stdout);
    assert!(stdout.contains("test"));
    assert!(stdout.contains("test_auth"));
}

#[test]
fn cli_edges_filters_by_rel() {
    let (_temp, repo) = create_fixture_repo();

    let index_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();
    assert!(index_output.status.success());

    let edges_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("edges")
        .arg("--rel")
        .arg("defines")
        .output()
        .unwrap();

    assert!(
        edges_output.status.success(),
        "edges failed: {}",
        String::from_utf8_lossy(&edges_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&edges_output.stdout);
    assert!(stdout.contains("defines"));
}

#[test]
fn cli_config_init_and_validate() {
    let (_temp, repo) = create_fixture_repo();

    let init_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("config")
        .arg("init")
        .output()
        .unwrap();

    assert!(
        init_output.status.success(),
        "config init failed: {}",
        String::from_utf8_lossy(&init_output.stderr)
    );
    assert!(repo.join(".chizu.toml").exists());

    let validate_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("config")
        .arg("validate")
        .output()
        .unwrap();

    assert!(
        validate_output.status.success(),
        "config validate failed: {}",
        String::from_utf8_lossy(&validate_output.stderr)
    );
}

#[test]
fn cli_visualize_outputs_svg() {
    let (_temp, repo) = create_fixture_repo();

    let index_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("index")
        .output()
        .unwrap();
    assert!(index_output.status.success());

    let output_path = repo.join("graph.svg");
    let viz_output = Command::new(chizu_bin())
        .arg("--repo")
        .arg(&repo)
        .arg("visualize")
        .arg("--output")
        .arg(&output_path)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&viz_output.stderr);

    if viz_output.status.success() {
        assert!(output_path.exists());
        let svg = std::fs::read_to_string(&output_path).unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
    } else if stderr.contains("Graphviz not found") {
        // Acceptable if dot is not installed; test passes.
    } else {
        panic!("visualize failed unexpectedly: {}", stderr);
    }
}
