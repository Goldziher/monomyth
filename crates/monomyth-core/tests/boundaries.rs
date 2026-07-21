//! Trait-first plane boundary guard (ADR-0013, ADR-0017).
//!
//! `monomyth-core` is the pure, serializable domain pivot: it must have no
//! dependency — direct or transitive — on the `monomyth-config`,
//! `monomyth-genre`, or `monomyth-contracts` planes. This mirrors the
//! `cargo tree` CI guard as a hermetic, offline test that reconstructs the
//! intra-workspace dependency graph straight from the crate manifests, so a
//! forbidden edge fails `cargo test` locally rather than only in CI.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

/// Planes the pivot may never reach. `monomyth-genre` does not exist yet; it is
/// listed so the guard bites the moment the crate is added (ADR-0017).
const FORBIDDEN: [&str; 3] = ["monomyth-config", "monomyth-contracts", "monomyth-genre"];

/// The workspace root, two directories above this crate's manifest
/// (`<root>/crates/monomyth-core`).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above the crate manifest")
        .to_path_buf()
}

/// Parse one crate manifest into its package name and the set of its normal
/// (non-dev, non-build) `monomyth-*` dependencies.
fn parse_manifest(manifest: &str) -> (String, BTreeSet<String>) {
    let parsed: toml::Value = toml::from_str(manifest).expect("crate manifest is valid TOML");
    let name = parsed
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .expect("crate manifest declares a package name")
        .to_owned();
    let mut edges = BTreeSet::new();
    if let Some(dependencies) = parsed.get("dependencies").and_then(toml::Value::as_table) {
        for dependency in dependencies.keys() {
            if dependency.starts_with("monomyth-") {
                edges.insert(dependency.clone());
            }
        }
    }
    (name, edges)
}

/// Build the directed graph of `monomyth-*` crates from every `crates/*/Cargo.toml`.
fn workspace_graph(root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let mut graph = BTreeMap::new();
    let crates_dir = root.join("crates");
    for entry in std::fs::read_dir(&crates_dir).expect("crates/ directory is readable") {
        let manifest_path = entry
            .expect("readable directory entry")
            .path()
            .join("Cargo.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = std::fs::read_to_string(&manifest_path).expect("crate manifest is readable");
        let (name, edges) = parse_manifest(&manifest);
        graph.insert(name, edges);
    }
    graph
}

/// The set of crates reachable from `start` following normal dependency edges.
fn reachable(graph: &BTreeMap<String, BTreeSet<String>>, start: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([start.to_owned()]);
    while let Some(current) = queue.pop_front() {
        if let Some(edges) = graph.get(&current) {
            for edge in edges {
                if seen.insert(edge.clone()) {
                    queue.push_back(edge.clone());
                }
            }
        }
    }
    seen
}

#[test]
fn core_does_not_depend_on_config_genre_or_contracts() {
    let root = workspace_root();
    let graph = workspace_graph(&root);

    assert!(
        graph.contains_key("monomyth-core"),
        "monomyth-core must be a workspace member"
    );
    // Guard against a vacuous pass: the two forbidden crates that exist today must ~keep
    // be present as graph nodes, proving the manifests parsed and were discovered. ~keep
    assert!(
        graph.contains_key("monomyth-config"),
        "monomyth-config manifest was not discovered"
    );
    assert!(
        graph.contains_key("monomyth-contracts"),
        "monomyth-contracts manifest was not discovered"
    );

    let core_deps = reachable(&graph, "monomyth-core");
    let violations: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|plane| core_deps.contains(*plane))
        .collect();

    assert!(
        violations.is_empty(),
        "ADR-0013/0017: monomyth-core must not depend on {violations:?} (the pivot depends on nothing)"
    );
}
