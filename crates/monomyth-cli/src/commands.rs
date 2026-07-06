//! One handler function per subcommand, plus the small pure helpers they share.
//!
//! Handlers own IO and the async provider/store calls; the pure helpers
//! ([`generate_world`], [`load_play_world`], [`resolve_text`]) are unit-testable in
//! isolation. The binary boundary wraps every fallible call with [`anyhow`]
//! context.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use monomyth_core::World;
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
            serde_json::from_str(&json)
                .with_context(|| format!("deserializing world from {}", path.display()))
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

    use super::{generate_world, load_play_world, resolve_text};

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
