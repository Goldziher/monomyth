//! Micro-tier taxonomy: the content-atom roots.
//!
//! - [`MotifClass`] — Thompson's twenty-three top-level motif classes.

use std::sync::LazyLock;

use crate::info::{MotifClassInfo, parse_items};
use crate::macros::framework_enum;

static MOTIF_CLASSES: LazyLock<Vec<MotifClassInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/thompson_motif_classes.json"
    ))
});

framework_enum! {
    /// Thompson's twenty-three lettered motif classes, the micro-tier roots.
    ///
    /// Fine-grained motifs (e.g. `D1361.4`) hang off these classes and are loaded
    /// separately from the derived datasets.
    MotifClass : MotifClassInfo = MOTIF_CLASSES;
    MythologicalMotifs = 1,
    Animals = 2,
    Tabu = 3,
    Magic = 4,
    TheDead = 5,
    Marvels = 6,
    Ogres = 7,
    Tests = 8,
    TheWiseAndTheFoolish = 9,
    Deceptions = 10,
    ReversalOfFortune = 11,
    OrdainingTheFuture = 12,
    ChanceAndFate = 13,
    Society = 14,
    RewardsAndPunishments = 15,
    CaptivesAndFugitives = 16,
    UnnaturalCruelty = 17,
    Sex = 18,
    TheNatureOfLife = 19,
    Religion = 20,
    TraitsOfCharacter = 21,
    Humor = 22,
    MiscellaneousGroupsOfMotifs = 23,
}
