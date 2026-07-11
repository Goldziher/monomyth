//! Crosswalks binding the tiers together: typed accessors resolved from the
//! crosswalk artifacts at load time.
//!
//! - [`arc_functions`] — Campbell stage → the Propp functions that realize it.
//! - [`arc_functions_weighted`] — the same binding, paired with each function's
//!   crosswalk weight (ADR-0022).
//! - [`plot_situations`] — Booker plot → its representative Polti situations.
//! - [`plot_situations_weighted`] — the same binding, paired with each situation's
//!   crosswalk weight (ADR-0022).
//! - [`role_alignment`] / [`role_actants`] / [`role_archetypes`] — Propp role →
//!   its typical Greimas actants and story archetypes.
//! - [`role_archetypes_weighted`] — the same archetype binding, paired with each
//!   archetype's crosswalk weight (ADR-0022).
//! - [`stage_motifs_weighted`] — Campbell stage → its Thompson motif-class
//!   candidates, paired with each candidate's crosswalk weight (ADR-0022).
//!
//! All referenced ids are resolved to typed enum values while loading, so a
//! dangling id in an artifact fails fast (and is independently caught by the
//! referential-integrity tests). Maps use `BTreeMap` for deterministic iteration.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::character::{Archetype, GreimasActant, ProppRole};
use crate::macro_tier::{BookerPlot, MonomythStage};
use crate::meso_tier::{PoltiSituation, ProppFunction};
use crate::micro_tier::MotifClass;

/// An empty slice used when a key has no crosswalk entry (e.g. a psychological
/// stage with no direct Propp counterpart).
const NO_FUNCTIONS: &[ProppFunction] = &[];
const NO_WEIGHTED_FUNCTIONS: &[(ProppFunction, u16)] = &[];
const NO_SITUATIONS: &[PoltiSituation] = &[];
const NO_WEIGHTED_SITUATIONS: &[(PoltiSituation, u16)] = &[];
const NO_ACTANTS: &[GreimasActant] = &[];
const NO_ARCHETYPES: &[Archetype] = &[];
const NO_WEIGHTED_ARCHETYPES: &[(Archetype, u16)] = &[];
const NO_WEIGHTED_MOTIFS: &[(MotifClass, u16)] = &[];

/// Resolve a list of artifact ids to typed variants.
///
/// Only called from `LazyLock` initializers over embedded, schema-valid
/// crosswalk artifacts; a dangling id is a build-time invariant violation
/// (covered by the referential-integrity tests), hence the `expect`.
fn resolve<T>(ids: &[u16], from_id: impl Fn(u16) -> Option<T>) -> Vec<T> {
    ids.iter()
        .map(|&id| from_id(id).expect("crosswalk id resolves to a known variant"))
        .collect()
}

#[derive(Debug, Deserialize)]
struct CrosswalkFile<Item> {
    items: Vec<Item>,
}

fn parse_crosswalk<Item>(json: &str) -> Vec<Item>
where
    Item: for<'de> Deserialize<'de>,
{
    let file: CrosswalkFile<Item> =
        serde_json::from_str(json).expect("embedded crosswalk artifact is schema-valid JSON");
    file.items
}

#[derive(Debug, Deserialize)]
struct ArcItem {
    campbell_stage_id: u16,
    propp_function_ids: Vec<u16>,
    propp_function_weights: Vec<u16>,
}

#[derive(Debug, Deserialize)]
struct PlotItem {
    booker_plot_id: u16,
    polti_situation_ids: Vec<u16>,
    polti_situation_weights: Vec<u16>,
}

#[allow(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
struct CharacterItem {
    propp_role_ids: Vec<u16>,
    greimas_actant_ids: Vec<u16>,
    archetype_ids: Vec<u16>,
    archetype_weights: Vec<u16>,
}

#[derive(Debug, Deserialize)]
struct MotifItem {
    campbell_stage_id: u16,
    motif_class_ids: Vec<u16>,
    motif_class_weights: Vec<u16>,
}

static ARC_MAP: LazyLock<BTreeMap<u16, Vec<ProppFunction>>> = LazyLock::new(|| {
    let items: Vec<ArcItem> = parse_crosswalk(include_str!(
        "../../../artifacts/frameworks/arc_crosswalk.json"
    ));
    items
        .into_iter()
        .map(|item| {
            (
                item.campbell_stage_id,
                resolve(&item.propp_function_ids, ProppFunction::from_id),
            )
        })
        .collect()
});

/// The same binding as [`ARC_MAP`], paired with each function's crosswalk weight
/// (permille, `0..=1000`), in the artifact's listed order.
static ARC_WEIGHTED_MAP: LazyLock<BTreeMap<u16, Vec<(ProppFunction, u16)>>> = LazyLock::new(|| {
    let items: Vec<ArcItem> = parse_crosswalk(include_str!(
        "../../../artifacts/frameworks/arc_crosswalk.json"
    ));
    items
        .into_iter()
        .map(|item| {
            let functions = resolve(&item.propp_function_ids, ProppFunction::from_id);
            let paired = functions
                .into_iter()
                .zip(item.propp_function_weights.iter().copied())
                .collect();
            (item.campbell_stage_id, paired)
        })
        .collect()
});

static PLOT_MAP: LazyLock<BTreeMap<u16, Vec<PoltiSituation>>> = LazyLock::new(|| {
    let items: Vec<PlotItem> = parse_crosswalk(include_str!(
        "../../../artifacts/frameworks/plot_crosswalk.json"
    ));
    items
        .into_iter()
        .map(|item| {
            (
                item.booker_plot_id,
                resolve(&item.polti_situation_ids, PoltiSituation::from_id),
            )
        })
        .collect()
});

/// The same binding as [`PLOT_MAP`], paired with each situation's crosswalk weight
/// (permille, `0..=1000`), in the artifact's listed order.
static PLOT_WEIGHTED_MAP: LazyLock<BTreeMap<u16, Vec<(PoltiSituation, u16)>>> =
    LazyLock::new(|| {
        let items: Vec<PlotItem> = parse_crosswalk(include_str!(
            "../../../artifacts/frameworks/plot_crosswalk.json"
        ));
        items
            .into_iter()
            .map(|item| {
                let situations = resolve(&item.polti_situation_ids, PoltiSituation::from_id);
                let paired = situations
                    .into_iter()
                    .zip(item.polti_situation_weights.iter().copied())
                    .collect();
                (item.booker_plot_id, paired)
            })
            .collect()
    });

/// The Campbell stage → Thompson motif-class candidates binding, paired with each
/// candidate's crosswalk weight (permille, `0..=1000`), in the artifact's listed
/// order.
static MOTIF_WEIGHTED_MAP: LazyLock<BTreeMap<u16, Vec<(MotifClass, u16)>>> = LazyLock::new(|| {
    let items: Vec<MotifItem> = parse_crosswalk(include_str!(
        "../../../artifacts/frameworks/motif_crosswalk.json"
    ));
    items
        .into_iter()
        .map(|item| {
            let motifs = resolve(&item.motif_class_ids, MotifClass::from_id);
            let paired = motifs
                .into_iter()
                .zip(item.motif_class_weights.iter().copied())
                .collect();
            (item.campbell_stage_id, paired)
        })
        .collect()
});

/// A Propp role's typical alignment across the two other character facets.
///
/// These are priors, not laws — a generated character selects one alignment as
/// its spine, then may bend facets for twists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterAlignment {
    /// The Greimas actants the role typically fills.
    pub actants: Vec<GreimasActant>,
    /// The story archetypes the role typically wears.
    pub archetypes: Vec<Archetype>,
}

static CHARACTER_MAP: LazyLock<BTreeMap<ProppRole, CharacterAlignment>> = LazyLock::new(|| {
    let items: Vec<CharacterItem> = parse_crosswalk(include_str!(
        "../../../artifacts/frameworks/character_crosswalk.json"
    ));
    let mut map = BTreeMap::new();
    for item in items {
        let alignment = CharacterAlignment {
            actants: resolve(&item.greimas_actant_ids, GreimasActant::from_id),
            archetypes: resolve(&item.archetype_ids, Archetype::from_id),
        };
        for role in resolve(&item.propp_role_ids, ProppRole::from_id) {
            map.insert(role, alignment.clone());
        }
    }
    map
});

/// The Propp role → archetype binding (one row per role, mirroring
/// [`CHARACTER_MAP`]'s expand-per-role construction), paired with each
/// archetype's crosswalk weight (permille, `0..=1000`), in the artifact's listed
/// order.
static CHARACTER_WEIGHTED_MAP: LazyLock<BTreeMap<ProppRole, Vec<(Archetype, u16)>>> =
    LazyLock::new(|| {
        let items: Vec<CharacterItem> = parse_crosswalk(include_str!(
            "../../../artifacts/frameworks/character_crosswalk.json"
        ));
        let mut map = BTreeMap::new();
        for item in items {
            let archetypes = resolve(&item.archetype_ids, Archetype::from_id);
            let paired: Vec<(Archetype, u16)> = archetypes
                .into_iter()
                .zip(item.archetype_weights.iter().copied())
                .collect();
            for role in resolve(&item.propp_role_ids, ProppRole::from_id) {
                map.insert(role, paired.clone());
            }
        }
        map
    });

/// The Propp functions that typically realize a Campbell stage.
///
/// Returns an empty slice for a psychological/transitional stage with no direct
/// Propp counterpart (e.g. [`MonomythStage::RefusalOfTheCall`]).
///
/// ```
/// use monomyth_frameworks::{arc_functions, MonomythStage, ProppFunction};
///
/// assert_eq!(
///     arc_functions(MonomythStage::CallToAdventure),
///     &[ProppFunction::VillainyOrLack, ProppFunction::Mediation],
/// );
/// assert!(arc_functions(MonomythStage::RefusalOfTheCall).is_empty());
/// ```
#[must_use]
pub fn arc_functions(stage: MonomythStage) -> &'static [ProppFunction] {
    ARC_MAP.get(&stage.id()).map_or(NO_FUNCTIONS, Vec::as_slice)
}

/// The Propp functions that typically realize a Campbell stage, paired with each
/// function's crosswalk weight (permille, `0..=1000`).
///
/// The pairing preserves the artifact's listed order — the same order
/// [`arc_functions`] returns, since both are drawn from the same underlying list.
/// Returns an empty slice under the same conditions as [`arc_functions`].
///
/// ```
/// use monomyth_frameworks::{arc_functions_weighted, MonomythStage, ProppFunction};
///
/// assert_eq!(
///     arc_functions_weighted(MonomythStage::CallToAdventure),
///     &[(ProppFunction::VillainyOrLack, 1000), (ProppFunction::Mediation, 1000)],
/// );
/// assert!(arc_functions_weighted(MonomythStage::RefusalOfTheCall).is_empty());
/// ```
#[must_use]
pub fn arc_functions_weighted(stage: MonomythStage) -> &'static [(ProppFunction, u16)] {
    ARC_WEIGHTED_MAP
        .get(&stage.id())
        .map_or(NO_WEIGHTED_FUNCTIONS, Vec::as_slice)
}

/// The Polti situations that typically instantiate a Booker plot.
///
/// ```
/// use monomyth_frameworks::{plot_situations, BookerPlot, PoltiSituation};
///
/// assert_eq!(
///     plot_situations(BookerPlot::RagsToRiches),
///     &[
///         PoltiSituation::Obtaining,
///         PoltiSituation::RivalryOfSuperiorAndInferior,
///         PoltiSituation::Ambition,
///     ],
/// );
/// ```
#[must_use]
pub fn plot_situations(plot: BookerPlot) -> &'static [PoltiSituation] {
    PLOT_MAP
        .get(&plot.id())
        .map_or(NO_SITUATIONS, Vec::as_slice)
}

/// The Polti situations that typically instantiate a Booker plot, paired with
/// each situation's crosswalk weight (permille, `0..=1000`).
///
/// The pairing preserves the artifact's listed order — the same order
/// [`plot_situations`] returns, since both are drawn from the same underlying
/// list. Returns an empty slice under the same conditions as [`plot_situations`].
///
/// ```
/// use monomyth_frameworks::{plot_situations_weighted, BookerPlot, PoltiSituation};
///
/// assert_eq!(
///     plot_situations_weighted(BookerPlot::TheQuest),
///     &[
///         (PoltiSituation::DaringEnterprise, 1000),
///         (PoltiSituation::TheEnigma, 1000),
///         (PoltiSituation::RecoveryOfALostOne, 1000),
///     ],
/// );
/// ```
#[must_use]
pub fn plot_situations_weighted(plot: BookerPlot) -> &'static [(PoltiSituation, u16)] {
    PLOT_WEIGHTED_MAP
        .get(&plot.id())
        .map_or(NO_WEIGHTED_SITUATIONS, Vec::as_slice)
}

/// The Thompson motif classes a beat on a Campbell stage may plausibly realize,
/// paired with each candidate's crosswalk weight (permille, `0..=1000`).
///
/// Sourced from `artifacts/frameworks/motif_crosswalk.json`, a hand-authored
/// pairing grounded in Thompson's own class glosses (no crosswalk artifact binds
/// Campbell stages to Thompson motif classes upstream). Every one of the
/// seventeen [`MonomythStage`] variants maps to at least one candidate.
///
/// ```
/// use monomyth_frameworks::{stage_motifs_weighted, MonomythStage, MotifClass};
///
/// assert_eq!(
///     stage_motifs_weighted(MonomythStage::TheRoadOfTrials),
///     &[(MotifClass::Tests, 1000)],
/// );
/// ```
#[must_use]
pub fn stage_motifs_weighted(stage: MonomythStage) -> &'static [(MotifClass, u16)] {
    MOTIF_WEIGHTED_MAP
        .get(&stage.id())
        .map_or(NO_WEIGHTED_MOTIFS, Vec::as_slice)
}

/// The typical character alignment (actants + archetypes) for a Propp role.
///
/// Returns `None` only for a role that no crosswalk row assigns (every Propp
/// role is currently assigned, so this is always `Some` today).
///
/// ```
/// use monomyth_frameworks::{role_alignment, GreimasActant, Archetype, ProppRole};
///
/// let hero = role_alignment(ProppRole::Hero).expect("hero is assigned");
/// assert_eq!(hero.actants, vec![GreimasActant::Subject, GreimasActant::Receiver]);
/// assert_eq!(hero.archetypes, vec![Archetype::Hero]);
/// ```
#[must_use]
pub fn role_alignment(role: ProppRole) -> Option<&'static CharacterAlignment> {
    CHARACTER_MAP.get(&role)
}

/// The Greimas actants a Propp role typically fills (empty if unassigned).
#[must_use]
pub fn role_actants(role: ProppRole) -> &'static [GreimasActant] {
    CHARACTER_MAP
        .get(&role)
        .map_or(NO_ACTANTS, |alignment| alignment.actants.as_slice())
}

/// The story archetypes a Propp role typically wears (empty if unassigned).
#[must_use]
pub fn role_archetypes(role: ProppRole) -> &'static [Archetype] {
    CHARACTER_MAP
        .get(&role)
        .map_or(NO_ARCHETYPES, |alignment| alignment.archetypes.as_slice())
}

/// The story archetypes a Propp role typically wears, paired with each
/// archetype's crosswalk weight (permille, `0..=1000`).
///
/// The pairing preserves the artifact's listed order — the same order
/// [`role_archetypes`] returns, since both are drawn from the same underlying
/// list. Returns an empty slice under the same conditions as [`role_archetypes`].
///
/// ```
/// use monomyth_frameworks::{role_archetypes_weighted, Archetype, ProppRole};
///
/// assert_eq!(
///     role_archetypes_weighted(ProppRole::Villain),
///     &[(Archetype::Shadow, 1000)],
/// );
/// ```
#[must_use]
pub fn role_archetypes_weighted(role: ProppRole) -> &'static [(Archetype, u16)] {
    CHARACTER_WEIGHTED_MAP
        .get(&role)
        .map_or(NO_WEIGHTED_ARCHETYPES, Vec::as_slice)
}
