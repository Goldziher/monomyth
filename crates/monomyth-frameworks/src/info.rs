//! Loaded-record structs mirroring the framework artifacts, plus the shared JSON
//! loading helper.
//!
//! Each `*Info` type captures the real fields of one artifact's items. Optional
//! fields use `#[serde(default)]` so items that omit them still parse. The types
//! are the return of every enum's `info()` accessor, so they are public.

use serde::{Deserialize, Serialize};

/// The common envelope of every `artifacts/frameworks/*.json` base-taxonomy file.
///
/// Only `items` is captured; the descriptive header fields (`title`, `license`,
/// `tier_note`, …) and any framework-specific top-level keys are intentionally
/// ignored, since serde skips unknown fields by default.
#[derive(Debug, Deserialize)]
pub(crate) struct FrameworkFile<Item> {
    pub(crate) items: Vec<Item>,
}

/// Parse the `items` array of an embedded framework artifact.
///
/// Only ever called from a `LazyLock` initializer over an `include_str!`-embedded,
/// schema-valid artifact, so a parse failure is a build-time program invariant
/// (covered by the parity test), not runtime input — hence the `expect`.
pub(crate) fn parse_items<Item>(json: &str) -> Vec<Item>
where
    Item: for<'de> Deserialize<'de>,
{
    let file: FrameworkFile<Item> =
        serde_json::from_str(json).expect("embedded framework artifact is schema-valid JSON");
    file.items
}

/// A Campbell monomyth stage record (`campbell_monomyth.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonomythStageInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical stage name.
    pub name: String,
    /// The act this stage belongs to (`Departure`, `Initiation`, `Return`).
    pub act: String,
    /// Authored description of the stage.
    pub description: String,
    /// Whether the stage is optional in a well-formed arc.
    #[serde(default)]
    pub optional: bool,
}

/// A Booker basic-plot record (`booker_plots.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookerPlotInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical plot name.
    pub name: String,
    /// Authored description of the plot.
    pub description: String,
}

/// An ATU top-level tale-type category record (`atu_categories.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtuCategoryInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical category name.
    pub name: String,
    /// The ATU type-number range this category covers (e.g. `"300-749"`).
    pub range: String,
    /// Authored description of the category.
    pub description: String,
}

/// A Propp function record (`propp_functions.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProppFunctionInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Propp's standard notation symbol for the function.
    pub symbol: String,
    /// Canonical function name.
    pub name: String,
    /// The sphere of action the function belongs to.
    pub sphere: String,
    /// Authored description of the function.
    pub description: String,
    /// Alternate symbol, where Propp gives one (e.g. `a` for a lack).
    #[serde(default)]
    pub alt_symbol: Option<String>,
}

/// A Polti dramatic-situation record (`polti_situations.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoltiSituationInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical situation name.
    pub name: String,
    /// The dynamic-element roles the situation requires.
    pub roles: Vec<String>,
    /// Authored description of the situation.
    pub description: String,
}

/// A Dundes motifeme record (`dundes_motifemes.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DundesMotifemeInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical motifeme name.
    pub name: String,
    /// Polarity of the slot (`tension` or `resolution`).
    pub polarity: String,
    /// The id of the motifeme this slot pairs with (opens/closes a tension).
    pub pairs_with: u16,
    /// Authored description of the motifeme.
    pub description: String,
}

/// A Thompson motif-class record (`thompson_motif_classes.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MotifClassInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Thompson's chapter letter (the index skips I, O, Y).
    pub code: String,
    /// Canonical class name.
    pub name: String,
    /// Authored description of the class.
    pub description: String,
}

/// A Propp dramatis-personae role record (`propp_roles.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProppRoleInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical role name.
    pub name: String,
    /// The spheres of action the role covers.
    pub sphere: String,
    /// Alternate names for the role.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Authored description of the role.
    pub description: String,
}

/// A Greimas actant record (`greimas_actants.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GreimasActantInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical actant name.
    pub name: String,
    /// The actantial axis (`desire`, `power`, `transmission`).
    pub axis: String,
    /// Authored description of the actant.
    pub description: String,
}

/// A story-archetype record (`archetypes.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchetypeInfo {
    /// Stable artifact id.
    pub id: u16,
    /// Canonical archetype name.
    pub name: String,
    /// Alternate names for the archetype.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Authored description of the archetype.
    pub description: String,
}
