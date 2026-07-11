//! Parity tests: the Rust enums must mirror the JSON artifacts exactly. This is
//! the Rust replacement for the old Python `validate.py`.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::character::{Archetype, GreimasActant, ProppRole};
use crate::crosswalk::{arc_functions, plot_situations, role_actants, role_archetypes};
use crate::macro_tier::{AtuCategory, BookerPlot, MonomythStage};
use crate::meso_tier::{DundesMotifeme, PoltiSituation, ProppFunction};
use crate::micro_tier::MotifClass;

/// Mechanical `PascalCase` of an artifact `name`: split on whitespace, strip every
/// non-alphanumeric character from each word, uppercase its first character, and
/// concatenate. This is the exact rule the enum variant identifiers follow, so a
/// round-trip through it is a pure normalization check.
fn pascal_case(name: &str) -> String {
    let mut out = String::new();
    for word in name.split_whitespace() {
        let cleaned: String = word.chars().filter(char::is_ascii_alphanumeric).collect();
        let mut chars = cleaned.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// The serde-serialized identifier of a variant (quotes stripped).
fn variant_identifier<T: Serialize>(variant: &T) -> String {
    let serialized = serde_json::to_string(variant).expect("variant serializes");
    serialized.trim_matches('"').to_owned()
}

/// Shared parity assertions run against every base enum.
fn assert_parity<T>(
    all: &'static [T],
    count: usize,
    ids: impl Fn(T) -> u16,
    names: impl Fn(T) -> String,
) where
    T: Copy + PartialEq + core::fmt::Debug + Serialize + DeserializeOwned,
{
    assert_eq!(all.len(), count, "variant count must match the artifact");
    for (index, &variant) in all.iter().enumerate() {
        let expected_id = u16::try_from(index).expect("index fits u16") + 1;
        assert_eq!(
            ids(variant),
            expected_id,
            "ids must be contiguous 1..=count in order"
        );

        let identifier = variant_identifier(&variant);
        assert_eq!(
            pascal_case(&names(variant)),
            identifier,
            "variant identifier must be the PascalCase of the artifact name",
        );

        let round_tripped: T =
            serde_json::from_str(&format!("\"{identifier}\"")).expect("variant deserializes");
        assert_eq!(round_tripped, variant, "serde round-trip must be identity");
    }
}

macro_rules! parity_test {
    ($test_name:ident, $enum:ty, $count:expr) => {
        #[test]
        fn $test_name() {
            assert_parity::<$enum>(
                <$enum>::all(),
                $count,
                |variant| variant.id(),
                |variant| variant.info().name.clone(),
            );
            let count = u16::try_from($count).expect("artifact count fits u16");
            for id in 1..=count {
                assert_eq!(
                    <$enum>::from_id(id).map(|variant| variant.id()),
                    Some(id),
                    "from_id then id must be identity",
                );
            }
            assert_eq!(<$enum>::from_id(0), None, "id 0 has no variant");
            assert_eq!(
                <$enum>::from_id(count + 1),
                None,
                "id past the count has no variant",
            );
        }
    };
}

parity_test!(monomyth_stage_count_matches_json, MonomythStage, 17);
parity_test!(booker_plot_count_matches_json, BookerPlot, 7);
parity_test!(atu_category_count_matches_json, AtuCategory, 7);
parity_test!(propp_function_count_matches_json, ProppFunction, 31);
parity_test!(polti_situation_count_matches_json, PoltiSituation, 36);
parity_test!(dundes_motifeme_count_matches_json, DundesMotifeme, 8);
parity_test!(motif_class_count_matches_json, MotifClass, 23);
parity_test!(propp_role_count_matches_json, ProppRole, 7);
parity_test!(greimas_actant_count_matches_json, GreimasActant, 6);
parity_test!(archetype_count_matches_json, Archetype, 8);

/// Assert every id in a crosswalk `*_ids` field resolves to a real variant of the
/// target enum, reading the raw artifact directly (independent of the loaders).
fn assert_ids_resolve<T>(json: &str, field: &str, from_id: impl Fn(u16) -> Option<T>) {
    let value: serde_json::Value = serde_json::from_str(json).expect("crosswalk is valid JSON");
    let items = value["items"].as_array().expect("items array");
    for item in items {
        let ids = item[field].as_array().expect("id list is an array");
        for id in ids {
            let id = u16::try_from(id.as_u64().expect("id is an integer")).expect("id fits u16");
            assert!(from_id(id).is_some(), "dangling id {id} in field {field}");
        }
    }
}

/// Assert every scalar `*_id` key in a crosswalk resolves to a real variant.
fn assert_scalar_id_resolves<T>(json: &str, field: &str, from_id: impl Fn(u16) -> Option<T>) {
    let value: serde_json::Value = serde_json::from_str(json).expect("crosswalk is valid JSON");
    let items = value["items"].as_array().expect("items array");
    for item in items {
        let id =
            u16::try_from(item[field].as_u64().expect("id is an integer")).expect("id fits u16");
        assert!(
            from_id(id).is_some(),
            "dangling scalar id {id} in field {field}"
        );
    }
}

const ARC_JSON: &str = include_str!("../../../artifacts/frameworks/arc_crosswalk.json");
const PLOT_JSON: &str = include_str!("../../../artifacts/frameworks/plot_crosswalk.json");
const CHARACTER_JSON: &str = include_str!("../../../artifacts/frameworks/character_crosswalk.json");
const MOTIF_JSON: &str = include_str!("../../../artifacts/frameworks/motif_crosswalk.json");

/// Assert that `weights_field` is parallel to `ids_field` (same length) and every
/// weight is a valid permille value in `0..=1000`, mirroring
/// `arc_crosswalk_weights_are_well_formed`'s pattern for a new weighted crosswalk.
fn assert_weights_are_well_formed(json: &str, ids_field: &str, weights_field: &str) {
    let value: serde_json::Value = serde_json::from_str(json).expect("crosswalk is valid JSON");
    let items = value["items"].as_array().expect("items array");
    for item in items {
        let ids = item[ids_field].as_array().expect("ids field is an array");
        let weights = item[weights_field]
            .as_array()
            .expect("weights field is an array");
        assert_eq!(
            ids.len(),
            weights.len(),
            "{weights_field} must be parallel to {ids_field} for item {}",
            item["id"],
        );
        for weight in weights {
            let weight = weight.as_u64().expect("weight is an integer");
            assert!(
                weight <= 1000,
                "weight {weight} out of permille range 0..=1000 for item {}",
                item["id"],
            );
        }
    }
}

#[test]
fn arc_crosswalk_ids_resolve() {
    assert_scalar_id_resolves(ARC_JSON, "campbell_stage_id", MonomythStage::from_id);
    assert_ids_resolve(ARC_JSON, "propp_function_ids", ProppFunction::from_id);
}

/// ADR-0022 Phase S1: `propp_function_weights` must be parallel to
/// `propp_function_ids` (same length) and every weight must be a valid permille
/// value in `0..=1000`.
#[test]
fn arc_crosswalk_weights_are_well_formed() {
    assert_weights_are_well_formed(ARC_JSON, "propp_function_ids", "propp_function_weights");
}

/// ADR-0022 Phase S2: `polti_situation_weights` must be parallel to
/// `polti_situation_ids` and every weight must be a valid permille value in
/// `0..=1000`.
#[test]
fn plot_crosswalk_weights_are_well_formed() {
    assert_weights_are_well_formed(PLOT_JSON, "polti_situation_ids", "polti_situation_weights");
}

/// ADR-0022 Phase S2: `archetype_weights` must be parallel to `archetype_ids` and
/// every weight must be a valid permille value in `0..=1000`. Parity must hold
/// even for a role like "Gatekeeper" whose `archetype_ids` is a single-element
/// (non-empty) list, and there is no role with a fully empty `archetype_ids` in
/// the current artifact, but the check itself makes no such assumption.
#[test]
fn character_crosswalk_weights_are_well_formed() {
    assert_weights_are_well_formed(CHARACTER_JSON, "archetype_ids", "archetype_weights");
}

/// ADR-0022 Phase S2: `motif_class_weights` must be parallel to `motif_class_ids`
/// and every weight must be a valid permille value in `0..=1000`.
#[test]
fn motif_crosswalk_weights_are_well_formed() {
    assert_weights_are_well_formed(MOTIF_JSON, "motif_class_ids", "motif_class_weights");
}

#[test]
fn plot_crosswalk_ids_resolve() {
    assert_scalar_id_resolves(PLOT_JSON, "booker_plot_id", BookerPlot::from_id);
    assert_ids_resolve(PLOT_JSON, "polti_situation_ids", PoltiSituation::from_id);
}

#[test]
fn character_crosswalk_ids_resolve() {
    assert_ids_resolve(CHARACTER_JSON, "propp_role_ids", ProppRole::from_id);
    assert_ids_resolve(CHARACTER_JSON, "greimas_actant_ids", GreimasActant::from_id);
    assert_ids_resolve(CHARACTER_JSON, "archetype_ids", Archetype::from_id);
}

#[test]
fn motif_crosswalk_ids_resolve() {
    assert_scalar_id_resolves(MOTIF_JSON, "campbell_stage_id", MonomythStage::from_id);
    assert_ids_resolve(MOTIF_JSON, "motif_class_ids", MotifClass::from_id);
}

#[test]
fn arc_crosswalk_resolves_to_typed_functions() {
    assert_eq!(
        arc_functions(MonomythStage::CallToAdventure),
        &[ProppFunction::VillainyOrLack, ProppFunction::Mediation],
    );
    assert!(arc_functions(MonomythStage::RefusalOfTheCall).is_empty());
}

#[test]
fn plot_crosswalk_resolves_to_typed_situations() {
    assert_eq!(
        plot_situations(BookerPlot::TheQuest),
        &[
            PoltiSituation::DaringEnterprise,
            PoltiSituation::TheEnigma,
            PoltiSituation::RecoveryOfALostOne,
        ],
    );
}

#[test]
fn character_crosswalk_resolves_role_facets() {
    assert_eq!(role_actants(ProppRole::Villain), &[GreimasActant::Opponent],);
    assert_eq!(role_archetypes(ProppRole::Villain), &[Archetype::Shadow]);
    assert_eq!(
        ProppRole::all()
            .iter()
            .filter(|role| role_actants(**role).is_empty())
            .count(),
        0,
        "every Propp role is assigned an actant alignment",
    );
}

#[test]
fn info_exposes_framework_specific_fields() {
    assert_eq!(MonomythStage::RefusalOfTheCall.info().act, "Departure");
    assert!(MonomythStage::RefusalOfTheCall.info().optional);
    assert!(!MonomythStage::CallToAdventure.info().optional);
    assert_eq!(ProppFunction::VillainyOrLack.info().symbol, "A");
    assert_eq!(
        ProppFunction::VillainyOrLack.info().alt_symbol.as_deref(),
        Some("a")
    );
    assert_eq!(MotifClass::Magic.info().code, "D");
    assert_eq!(DundesMotifeme::Interdiction.info().pairs_with, 2);
    assert_eq!(AtuCategory::TalesOfMagic.info().range, "300-749");
}

#[test]
fn ord_follows_artifact_id_order() {
    assert!(MonomythStage::CallToAdventure < MonomythStage::FreedomToLive);
    let mut stages = vec![MonomythStage::FreedomToLive, MonomythStage::CallToAdventure];
    stages.sort();
    assert_eq!(
        stages,
        vec![MonomythStage::CallToAdventure, MonomythStage::FreedomToLive]
    );
}
