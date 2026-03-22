pub fn repo_id(name: &str) -> String {
    format!("repo::{name}")
}

pub fn component_id(crate_name: &str) -> String {
    format!("component::{crate_name}")
}

pub fn source_unit_id(crate_name: &str, path: &str) -> String {
    format!("source_unit::{crate_name}::{path}")
}

pub fn symbol_id(crate_name: &str, symbol_name: &str) -> String {
    format!("symbol::{crate_name}::{symbol_name}")
}

pub fn test_id(crate_name: &str, fn_name: &str) -> String {
    format!("test::{crate_name}::{fn_name}")
}

pub fn bench_id(crate_name: &str, fn_name: &str) -> String {
    format!("bench::{crate_name}::{fn_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_formats() {
        assert_eq!(repo_id("grakno"), "repo::grakno");
        assert_eq!(component_id("grakno-core"), "component::grakno-core");
        assert_eq!(
            source_unit_id("grakno-core", "crates/grakno-core/src/lib.rs"),
            "source_unit::grakno-core::crates/grakno-core/src/lib.rs"
        );
        assert_eq!(
            symbol_id("grakno-core", "Store"),
            "symbol::grakno-core::Store"
        );
        assert_eq!(
            test_id("grakno-core", "test_foo"),
            "test::grakno-core::test_foo"
        );
        assert_eq!(
            bench_id("grakno-core", "bench_bar"),
            "bench::grakno-core::bench_bar"
        );
    }
}
