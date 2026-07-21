//! [`GenreProfile`]: the config value that biases content-fill targeting.

use monomyth_config::GenreSettings;
use monomyth_gen::ContentConfig;
use serde::{Deserialize, Serialize};

/// The number of ship-safe passages retrieved to ground each content slot under
/// the myth genre, matching `monomyth-gen`'s `ContentConfig::default` so an
/// unconfigured run grounds identically with or without genre targeting.
const DEFAULT_MYTH_GROUNDING_TOP_K: u32 = 4;

/// The writing task handed to the model for a detective-genre world title.
const DETECTIVE_TITLE_INSTRUCTION: &str =
    "Write a short, moody title for a detective mystery, evoking a case rather than a place.";
/// The writing task handed to the model for a detective-genre location.
const DETECTIVE_LOCATION_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for a location in a detective \
     mystery — grounded, contemporary or noir, hinting at a clue or a secret it conceals.";
/// The writing task handed to the model for a detective-genre entity.
const DETECTIVE_ENTITY_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for a character in a detective \
     mystery, true to its dramatic role — a suspect, a witness, or the investigator.";
/// The writing task handed to the model for a detective-genre item.
const DETECTIVE_ITEM_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for a piece of evidence or a \
     personal effect in a detective mystery.";

/// The writing task handed to the model for a LitRPG-genre world title.
const LITRPG_TITLE_INSTRUCTION: &str =
    "Write a short, punchy title for a LitRPG adventure, evoking a quest or a dungeon tier.";
/// The writing task handed to the model for a LitRPG-genre location.
const LITRPG_LOCATION_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for a location in a LitRPG \
     world — a dungeon floor, a settlement, or a biome — noting any level or difficulty cues.";
/// The writing task handed to the model for a LitRPG-genre entity.
const LITRPG_ENTITY_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for a character or monster in a \
     LitRPG world, true to its dramatic role, with a nod to its class or stat archetype.";
/// The writing task handed to the model for a LitRPG-genre item.
const LITRPG_ITEM_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for an item or piece of loot in \
     a LitRPG world.";

/// A canonical genre identifier a [`GenreProfile`] resolves to.
///
/// `Default` is [`GenreKind::Myth`]: the flavor every content-fill pass had
/// before genre targeting existed, so an unconfigured run is unaffected. An
/// unrecognized `genre.name` string (see
/// [`GenreSettings::name`](monomyth_config::GenreSettings)) falls back to
/// [`GenreKind::Myth`] rather than failing resolution — genre selection is a
/// targeting hint, not a hard input-validation boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GenreKind {
    /// Comparative-mythology-grounded adventures — the original, and default,
    /// flavor.
    #[default]
    Myth,
    /// Detective/mystery fiction.
    Detective,
    /// `LitRPG` (game-system narrative conventions).
    LitRpg,
}

impl GenreKind {
    /// Parse a `genre.name` config string into a [`GenreKind`], case-insensitively.
    ///
    /// An unrecognized name resolves to [`GenreKind::Myth`]; see the type's docs
    /// for why this is a fallback rather than an error.
    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "detective" => Self::Detective,
            "litrpg" => Self::LitRpg,
            _ => Self::Myth,
        }
    }
}

/// The config value that biases generation's content-fill toward a genre's
/// conventions (ADR-0017).
///
/// `Default` reproduces the myth flavor — [`ContentConfig::default`] — so
/// constructing a [`GenreProfile::default`] and projecting it with
/// [`content_config`](Self::content_config) is a no-op migration for any run that
/// does not configure `[genre]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenreProfile {
    /// Which genre's conventions this profile targets.
    pub kind: GenreKind,
    /// Number of ship-safe passages retrieved to ground each content slot under
    /// this genre.
    pub grounding_top_k: u32,
}

impl Default for GenreProfile {
    fn default() -> Self {
        Self {
            kind: GenreKind::Myth,
            grounding_top_k: DEFAULT_MYTH_GROUNDING_TOP_K,
        }
    }
}

impl From<&GenreSettings> for GenreProfile {
    /// Project the resolved, layered `[genre]` config table into a
    /// [`GenreProfile`].
    ///
    /// This is the one place the layered `monomyth-config` types cross into
    /// `monomyth-genre`, mirroring how `monomyth-cli`'s `generation_config`
    /// projects `MonomythConfig` into `monomyth-gen`'s flat `GenerationConfig`.
    fn from(settings: &GenreSettings) -> Self {
        Self {
            kind: GenreKind::parse(settings.name.get()),
            grounding_top_k: *settings.grounding_top_k.get(),
        }
    }
}

impl GenreProfile {
    /// Project this profile into the [`ContentConfig`] a content-fill pass reads.
    ///
    /// The myth profile reuses [`ContentConfig::default`] verbatim (aside from the
    /// configurable `grounding_top_k`), which is what keeps an unconfigured run
    /// byte-identical to the pre-ADR-0017 behavior. A non-myth profile substitutes
    /// its own instruction strings, so the shift in generated content is
    /// measurable directly on the returned value.
    #[must_use]
    pub fn content_config(&self) -> ContentConfig {
        match self.kind {
            GenreKind::Myth => ContentConfig {
                grounding_top_k: self.grounding_top_k,
                ..ContentConfig::default()
            },
            GenreKind::Detective => ContentConfig {
                grounding_top_k: self.grounding_top_k,
                title_instruction: DETECTIVE_TITLE_INSTRUCTION.to_owned(),
                location_instruction: DETECTIVE_LOCATION_INSTRUCTION.to_owned(),
                entity_instruction: DETECTIVE_ENTITY_INSTRUCTION.to_owned(),
                item_instruction: DETECTIVE_ITEM_INSTRUCTION.to_owned(),
            },
            GenreKind::LitRpg => ContentConfig {
                grounding_top_k: self.grounding_top_k,
                title_instruction: LITRPG_TITLE_INSTRUCTION.to_owned(),
                location_instruction: LITRPG_LOCATION_INSTRUCTION.to_owned(),
                entity_instruction: LITRPG_ENTITY_INSTRUCTION.to_owned(),
                item_instruction: LITRPG_ITEM_INSTRUCTION.to_owned(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use monomyth_config::{ConfigResolver, LayerSource};
    use monomyth_gen::ContentConfig;

    use super::{GenreKind, GenreProfile};

    #[test]
    fn default_genre_profile_is_myth() {
        let profile = GenreProfile::default();
        assert_eq!(profile.kind, GenreKind::Myth);
        assert_eq!(profile.grounding_top_k, 4);
    }

    #[test]
    fn default_genre_profile_content_config_matches_the_myth_default() {
        let profile = GenreProfile::default();
        let content_config = profile.content_config();
        let expected = ContentConfig::default();
        assert_eq!(content_config.grounding_top_k, expected.grounding_top_k);
        assert_eq!(content_config.title_instruction, expected.title_instruction);
        assert_eq!(
            content_config.location_instruction,
            expected.location_instruction
        );
        assert_eq!(
            content_config.entity_instruction,
            expected.entity_instruction
        );
        assert_eq!(content_config.item_instruction, expected.item_instruction);
    }

    #[test]
    fn genre_kind_parses_known_names_case_insensitively() {
        assert_eq!(GenreKind::parse("Detective"), GenreKind::Detective);
        assert_eq!(GenreKind::parse("LITRPG"), GenreKind::LitRpg);
        assert_eq!(GenreKind::parse("myth"), GenreKind::Myth);
    }

    #[test]
    fn genre_kind_falls_back_to_myth_for_an_unrecognized_name() {
        assert_eq!(GenreKind::parse("space-opera"), GenreKind::Myth);
    }

    #[test]
    fn resolved_default_config_projects_to_the_default_profile() {
        let config = ConfigResolver::defaults()
            .resolve()
            .expect("defaults validate");
        let profile = GenreProfile::from(&config.genre);
        assert_eq!(profile, GenreProfile::default());
    }

    #[test]
    fn a_resolved_detective_config_projects_to_a_detective_profile() {
        let path = std::env::temp_dir().join("monomyth-genre-test-detective.toml");
        std::fs::write(
            &path,
            "[genre]\nname = \"detective\"\ngrounding_top_k = 6\n",
        )
        .expect("write temp config");
        let config = ConfigResolver::discover_from(None, None, Some(&path))
            .expect("valid toml")
            .resolve()
            .expect("in-range config validates");
        std::fs::remove_file(&path).ok();

        let profile = GenreProfile::from(&config.genre);
        assert_eq!(profile.kind, GenreKind::Detective);
        assert_eq!(profile.grounding_top_k, 6);
        assert_eq!(config.genre.name.source(), LayerSource::ProjectOverride);
    }

    #[test]
    fn a_non_myth_profile_yields_different_content_config_instructions() {
        let myth = GenreProfile::default();
        let detective = GenreProfile {
            kind: GenreKind::Detective,
            grounding_top_k: myth.grounding_top_k,
        };
        let myth_content = myth.content_config();
        let detective_content = detective.content_config();
        assert_ne!(
            myth_content.title_instruction,
            detective_content.title_instruction
        );
        assert_ne!(
            myth_content.location_instruction,
            detective_content.location_instruction
        );
        assert_ne!(
            myth_content.entity_instruction,
            detective_content.entity_instruction
        );
        assert_ne!(
            myth_content.item_instruction,
            detective_content.item_instruction
        );
    }
}
