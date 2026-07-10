//! One handler function per subcommand, plus the small pure helpers they share.
//!
//! Handlers own IO and the async provider/store calls; the pure helpers
//! ([`generate_world`], [`load_play_world`], [`resolve_text`]) are unit-testable in
//! isolation. The binary boundary wraps every fallible call with [`anyhow`]
//! context.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use monomyth_core::{EditOutcome, NarrativeEdit, World};
use monomyth_gen::{ContentContext, Generator};
use monomyth_knowledge::{IngestInput, Knowledge, KnowledgeQuery};
use monomyth_llm::Llm;
use monomyth_text::{render_intro, render_location, render_structure};

/// Generate a world's structure from `seed`.
///
/// Pure and deterministic: the same seed reproduces a byte-identical serialized
/// world. Shared by `gen` and the seed path of `play`.
///
/// # Errors
///
/// Propagates [`monomyth_gen::GenError`] if the procedural pipeline fails.
pub(crate) fn generate_world(seed: u64) -> Result<World> {
    Generator::with_default_passes()
        .generate_structure(seed)
        .context("generating world structure")
}

/// Handle `gen`: build structure, optionally fill content, render, and serialize.
///
/// The serialized world always leaves this command recoverable by `play`: it is
/// written to `out` when given, otherwise printed to stdout after the human
/// render.
///
/// # Errors
///
/// Fails if generation, the LLM/knowledge setup, content filling, serialization,
/// or writing the output file fails.
pub(crate) async fn run_gen(
    seed: u64,
    fill: bool,
    out: Option<PathBuf>,
    model: &str,
    db: &Path,
) -> Result<()> {
    let generator = Generator::with_default_passes();
    let mut world = generator
        .generate_structure(seed)
        .context("generating world structure")?;

    if fill {
        let llm = Llm::from_env(model).context("initializing the LLM from the environment")?;
        let knowledge = Knowledge::open(db)
            .await
            .context("opening the knowledge store")?;
        let context = ContentContext {
            llm: &llm,
            knowledge: &knowledge,
            model,
        };
        generator
            .fill_content(&mut world, &context)
            .await
            .context("filling world content")?;
    }

    println!("{}\n", render_intro(&world));
    println!("{}\n", render_location(&world));
    println!("{}", render_structure(&world));

    let serialized = serde_json::to_string_pretty(&world).context("serializing the world")?;
    match out {
        Some(path) => {
            std::fs::write(&path, &serialized)
                .with_context(|| format!("writing world to {}", path.display()))?;
            println!("\nWorld written to {}", path.display());
        }
        None => println!("\n{serialized}"),
    }
    Ok(())
}

/// Apply a batch of narrative edits to `world`'s structure, transactionally.
///
/// Pure and testable: it delegates to the core transactional
/// [`apply_edits`](monomyth_core::NarrativeStructure::apply_edits), so the whole
/// batch either lands and re-validates or leaves the structure untouched. Returns
/// the per-edit outcomes so the caller can report the ids of any created nodes.
///
/// # Errors
///
/// Fails if any edit's precondition is unmet or the edited structure is invalid;
/// on failure `world` is unchanged.
pub(crate) fn apply_edit_script(
    world: &mut World,
    edits: &[NarrativeEdit],
) -> Result<Vec<EditOutcome>> {
    world
        .story
        .structure
        .apply_edits(edits)
        .context("applying the narrative edit script")
}

/// Handle `edit`: load a world, apply a JSON edit script to its narrative
/// structure, re-validate, and emit the edited world.
///
/// The script is a JSON array of [`NarrativeEdit`] operations referencing existing
/// node ids (copy them from a `gen --out` world file). The edited world is written
/// to `out` when given, otherwise printed to stdout; a human summary goes to stderr
/// so stdout stays a clean serialized world.
///
/// # Errors
///
/// Fails if the world or script cannot be read or parsed, if an edit is rejected
/// (leaving the world untouched), or if serialization or writing the output fails.
pub(crate) fn run_edit(world_path: &Path, script_path: &Path, out: Option<PathBuf>) -> Result<()> {
    let world_json = std::fs::read_to_string(world_path)
        .with_context(|| format!("reading world file {}", world_path.display()))?;
    let mut world = World::from_json_checked(&world_json)
        .with_context(|| format!("loading world from {}", world_path.display()))?;

    let script_json = std::fs::read_to_string(script_path)
        .with_context(|| format!("reading edit script {}", script_path.display()))?;
    let edits: Vec<NarrativeEdit> = serde_json::from_str(&script_json)
        .with_context(|| format!("parsing edit script {}", script_path.display()))?;

    let before = world.story.structure.nodes.len();
    let outcomes = apply_edit_script(&mut world, &edits)?;
    let after = world.story.structure.nodes.len();

    eprintln!(
        "Applied {} edit(s): {before} -> {after} nodes.",
        edits.len()
    );
    for outcome in &outcomes {
        if let EditOutcome::NodeAdded(id) = outcome {
            eprintln!("  added node {id:?}");
        }
    }
    eprintln!("\n{}", render_structure(&world));

    let serialized =
        serde_json::to_string_pretty(&world).context("serializing the edited world")?;
    match out {
        Some(path) => {
            std::fs::write(&path, &serialized)
                .with_context(|| format!("writing world to {}", path.display()))?;
            eprintln!("Edited world written to {}", path.display());
        }
        None => println!("{serialized}"),
    }
    Ok(())
}

/// Build the world a `play` session runs against: load from `world_path` or
/// generate from `seed`. Exactly one must be provided.
///
/// # Errors
///
/// Fails if neither or both inputs are given, if the file cannot be read, or if
/// the JSON cannot be deserialized into a [`World`].
pub(crate) fn load_play_world(world_path: Option<&Path>, seed: Option<u64>) -> Result<World> {
    match (world_path, seed) {
        (Some(path), None) => {
            let json = std::fs::read_to_string(path)
                .with_context(|| format!("reading world file {}", path.display()))?;
            World::from_json_checked(&json)
                .with_context(|| format!("loading world from {}", path.display()))
        }
        (None, Some(seed)) => generate_world(seed),
        (Some(_), Some(_)) => bail!("provide exactly one of --world or --seed, not both"),
        (None, None) => bail!("provide one of --world or --seed"),
    }
}

/// Resolve the text a `ingest` should store from its mutually exclusive inputs.
///
/// # Errors
///
/// Fails if neither or both of `text`/`file` are given, or if the file read fails.
pub(crate) fn resolve_text(text: Option<String>, file: Option<&Path>) -> Result<String> {
    match (text, file) {
        (Some(text), None) => Ok(text),
        (None, Some(path)) => std::fs::read_to_string(path)
            .with_context(|| format!("reading ingest file {}", path.display())),
        (Some(_), Some(_)) => bail!("provide exactly one of --text or --file, not both"),
        (None, None) => bail!("provide one of --text or --file"),
    }
}

/// Handle `ingest`: store `text` for the declared `source`, printing the doc id.
///
/// # Errors
///
/// Fails if the store cannot be opened, or if the ingest gate refuses the source
/// (undeclared or non-ship namespace).
pub(crate) async fn run_ingest(source: &str, text: String, db: &Path) -> Result<()> {
    let knowledge = Knowledge::open(db)
        .await
        .context("opening the knowledge store")?;
    let document_id = knowledge
        .ingest(source, IngestInput::new(text))
        .await
        .with_context(|| format!("ingesting source {source:?}"))?;
    println!("Ingested {source} as document {}", document_id.0);
    Ok(())
}

/// Handle `retrieve`: print each surfaceable passage's source, score, and text.
///
/// # Errors
///
/// Fails if the store cannot be opened or the retrieval fails.
pub(crate) async fn run_retrieve(query: &str, top_k: u32, db: &Path) -> Result<()> {
    let knowledge = Knowledge::open(db)
        .await
        .context("opening the knowledge store")?;
    let passages = knowledge
        .retrieve(KnowledgeQuery::surfaceable(query, top_k))
        .await
        .with_context(|| format!("retrieving passages for {query:?}"))?;

    if passages.is_empty() {
        println!("No passages found.");
        return Ok(());
    }
    for passage in &passages {
        println!(
            "[{}] score={:.4}\n{}\n",
            passage.source_id, passage.score, passage.text
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use monomyth_core::NarrativeEdit;

    use super::{apply_edit_script, generate_world, load_play_world, resolve_text};

    #[test]
    fn should_apply_a_valid_edit_script_to_the_structure() {
        let mut world = generate_world(42).expect("generation succeeds");
        let root = world.story.structure.root();
        let outcomes = apply_edit_script(
            &mut world,
            &[NarrativeEdit::RelabelNode {
                node: root,
                label: "Opening".to_owned(),
            }],
        )
        .expect("a valid script applies");
        assert_eq!(outcomes.len(), 1);
        assert_eq!(
            world.story.structure.node(root).expect("root exists").label,
            "Opening"
        );
    }

    #[test]
    fn should_reject_an_invalid_edit_script_and_leave_the_world_untouched() {
        let mut world = generate_world(42).expect("generation succeeds");
        let root = world.story.structure.root();
        let before = serde_json::to_string(&world).expect("serialization succeeds");
        let result = apply_edit_script(&mut world, &[NarrativeEdit::RemoveBeat { node: root }]);
        assert!(result.is_err(), "removing the root must fail");
        let after = serde_json::to_string(&world).expect("serialization succeeds");
        assert_eq!(before, after, "a rejected script must not mutate the world");
    }

    #[test]
    fn should_generate_byte_identical_world_for_same_seed() {
        let first = serde_json::to_string(&generate_world(42).expect("generation succeeds"))
            .expect("serialization succeeds");
        let second = serde_json::to_string(&generate_world(42).expect("generation succeeds"))
            .expect("serialization succeeds");
        assert_eq!(first, second, "seed 42 must reproduce the same world");
    }

    #[test]
    fn should_error_when_neither_world_nor_seed_given() {
        let error = load_play_world(None, None).expect_err("neither input is an error");
        assert_eq!(error.to_string(), "provide one of --world or --seed");
    }

    #[test]
    fn should_error_when_both_world_and_seed_given() {
        let error =
            load_play_world(Some(Path::new("world.json")), Some(1)).expect_err("both is an error");
        assert_eq!(
            error.to_string(),
            "provide exactly one of --world or --seed, not both"
        );
    }

    #[test]
    fn should_return_inline_text_for_ingest() {
        let text = resolve_text(Some("hello".to_owned()), None).expect("inline text resolves");
        assert_eq!(text, "hello");
    }

    #[test]
    fn should_error_when_neither_text_nor_file_given() {
        let error = resolve_text(None, None).expect_err("neither input is an error");
        assert_eq!(error.to_string(), "provide one of --text or --file");
    }

    #[test]
    fn should_error_when_both_text_and_file_given() {
        let error = resolve_text(Some("x".to_owned()), Some(Path::new("in.txt")))
            .expect_err("both is an error");
        assert_eq!(
            error.to_string(),
            "provide exactly one of --text or --file, not both"
        );
    }
}
