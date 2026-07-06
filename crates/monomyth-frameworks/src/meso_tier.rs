//! Meso-tier taxonomies: the generative grammar of beats and conflicts.
//!
//! - [`ProppFunction`] — Propp's thirty-one functions of dramatis personae.
//! - [`PoltiSituation`] — Polti's thirty-six dramatic situations.
//! - [`DundesMotifeme`] — Dundes's eight paired slot/filler motifemes.

use std::sync::LazyLock;

use crate::info::{DundesMotifemeInfo, PoltiSituationInfo, ProppFunctionInfo, parse_items};
use crate::macros::framework_enum;

static PROPP_FUNCTIONS: LazyLock<Vec<ProppFunctionInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/propp_functions.json"
    ))
});

static POLTI_SITUATIONS: LazyLock<Vec<PoltiSituationInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/polti_situations.json"
    ))
});

static DUNDES_MOTIFEMES: LazyLock<Vec<DundesMotifemeInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/dundes_motifemes.json"
    ))
});

framework_enum! {
    /// Propp's thirty-one functions — the engine's core generative grammar (meso tier).
    ///
    /// Functions occur in a fixed relative order; any tale selects a subset.
    ProppFunction : ProppFunctionInfo = PROPP_FUNCTIONS;
    Absentation = 1,
    Interdiction = 2,
    Violation = 3,
    Reconnaissance = 4,
    Delivery = 5,
    Trickery = 6,
    Complicity = 7,
    VillainyOrLack = 8,
    Mediation = 9,
    BeginningCounteraction = 10,
    Departure = 11,
    FirstFunctionOfTheDonor = 12,
    HerosReaction = 13,
    ReceiptOfAMagicalAgent = 14,
    Guidance = 15,
    Struggle = 16,
    Branding = 17,
    Victory = 18,
    Liquidation = 19,
    Return = 20,
    Pursuit = 21,
    Rescue = 22,
    UnrecognizedArrival = 23,
    UnfoundedClaims = 24,
    DifficultTask = 25,
    Solution = 26,
    Recognition = 27,
    Exposure = 28,
    Transfiguration = 29,
    Punishment = 30,
    Wedding = 31,
}

framework_enum! {
    /// Polti's thirty-six dramatic situations, the conflict/encounter enum (meso tier).
    PoltiSituation : PoltiSituationInfo = POLTI_SITUATIONS;
    Supplication = 1,
    Deliverance = 2,
    CrimePursuedByVengeance = 3,
    VengeanceTakenForKinUponKin = 4,
    Pursuit = 5,
    Disaster = 6,
    FallingPreyToCrueltyOrMisfortune = 7,
    Revolt = 8,
    DaringEnterprise = 9,
    Abduction = 10,
    TheEnigma = 11,
    Obtaining = 12,
    EnmityOfKin = 13,
    RivalryOfKin = 14,
    MurderousAdultery = 15,
    Madness = 16,
    FatalImprudence = 17,
    InvoluntaryCrimesOfLove = 18,
    SlayingOfAKinsmanUnrecognized = 19,
    SelfSacrificeForAnIdeal = 20,
    SelfSacrificeForKindred = 21,
    AllSacrificedForAPassion = 22,
    NecessityOfSacrificingLovedOnes = 23,
    RivalryOfSuperiorAndInferior = 24,
    Adultery = 25,
    CrimesOfLove = 26,
    DiscoveryOfTheDishonorOfALovedOne = 27,
    ObstaclesToLove = 28,
    AnEnemyLoved = 29,
    Ambition = 30,
    ConflictWithAGod = 31,
    MistakenJealousy = 32,
    ErroneousJudgment = 33,
    Remorse = 34,
    RecoveryOfALostOne = 35,
    LossOfLovedOnes = 36,
}

framework_enum! {
    /// Dundes's eight paired motifemes — tension/resolution structural slots (meso tier).
    DundesMotifeme : DundesMotifemeInfo = DUNDES_MOTIFEMES;
    Interdiction = 1,
    Violation = 2,
    Lack = 3,
    LackLiquidated = 4,
    Task = 5,
    TaskAccomplished = 6,
    Deceit = 7,
    Deception = 8,
}
