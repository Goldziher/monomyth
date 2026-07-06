//! Macro-tier taxonomies: the high-level arc and genre skeletons.
//!
//! - [`MonomythStage`] — Campbell's seventeen hero's-journey stages.
//! - [`BookerPlot`] — Booker's seven basic plots.
//! - [`AtuCategory`] — the seven top-level ATU tale-type categories.

use std::sync::LazyLock;

use crate::info::{AtuCategoryInfo, BookerPlotInfo, MonomythStageInfo, parse_items};
use crate::macros::framework_enum;

static MONOMYTH_STAGES: LazyLock<Vec<MonomythStageInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/campbell_monomyth.json"
    ))
});

static BOOKER_PLOTS: LazyLock<Vec<BookerPlotInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/booker_plots.json"
    ))
});

static ATU_CATEGORIES: LazyLock<Vec<AtuCategoryInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/atu_categories.json"
    ))
});

framework_enum! {
    /// Campbell's seventeen monomyth stages, the story spine (macro tier).
    ///
    /// ```
    /// use monomyth_frameworks::MonomythStage;
    ///
    /// assert_eq!(MonomythStage::from_id(1), Some(MonomythStage::CallToAdventure));
    /// assert_eq!(MonomythStage::CallToAdventure.id(), 1);
    /// assert_eq!(MonomythStage::all().len(), 17);
    /// ```
    MonomythStage : MonomythStageInfo = MONOMYTH_STAGES;
    CallToAdventure = 1,
    RefusalOfTheCall = 2,
    SupernaturalAid = 3,
    CrossingTheFirstThreshold = 4,
    BellyOfTheWhale = 5,
    TheRoadOfTrials = 6,
    TheMeetingWithTheGoddess = 7,
    WomanAsTemptress = 8,
    AtonementWithTheFather = 9,
    Apotheosis = 10,
    TheUltimateBoon = 11,
    RefusalOfTheReturn = 12,
    TheMagicFlight = 13,
    RescueFromWithout = 14,
    TheCrossingOfTheReturnThreshold = 15,
    MasterOfTheTwoWorlds = 16,
    FreedomToLive = 17,
}

framework_enum! {
    /// Booker's seven basic plots, the high-level genre enum (macro tier).
    BookerPlot : BookerPlotInfo = BOOKER_PLOTS;
    OvercomingTheMonster = 1,
    RagsToRiches = 2,
    TheQuest = 3,
    VoyageAndReturn = 4,
    Comedy = 5,
    Tragedy = 6,
    Rebirth = 7,
}

framework_enum! {
    /// The seven top-level Aarne-Thompson-Uther tale-type categories (macro tier).
    AtuCategory : AtuCategoryInfo = ATU_CATEGORIES;
    AnimalTales = 1,
    TalesOfMagic = 2,
    ReligiousTales = 3,
    RealisticTalesNovelle = 4,
    TalesOfTheStupidOgreGiantDevil = 5,
    AnecdotesAndJokes = 6,
    FormulaTales = 7,
}
