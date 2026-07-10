//! The `framework_enum!` macro that turns a framework artifact into a Rust enum.
//!
//! Why a macro: every base taxonomy shares the same shape — a fieldless enum whose
//! variants carry a stable numeric `id`, plus the four accessors (`id`, `from_id`,
//! `all`, `info`). Generating them mechanically keeps the enums and their JSON
//! artifacts in lockstep and makes the parity test a pure normalization check.

/// Generate a framework enum and its accessors from an artifact.
///
/// The caller supplies the enum name, its loaded-record type, the `LazyLock`
/// holding the parsed records (in `id` order), and the `Variant = id` list in
/// artifact `id` order so the derived `Ord` matches the artifact order.
///
/// Serialization uses serde's default derive: a variant is written as its own
/// identifier string (e.g. `"CallToAdventure"`), which is stable and
/// snapshot-friendly — the numeric ids are not self-describing, so they are not
/// used on the wire.
macro_rules! framework_enum {
    (
        $(#[$enum_meta:meta])*
        $name:ident : $info:ty = $loader:path;
        $( $variant:ident = $id:expr ),+ $(,)?
    ) => {
        $(#[$enum_meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
            serde::Serialize, serde::Deserialize,
        )]
        pub enum $name {
            $( $variant ),+
        }

        impl $name {
            /// All variants, in artifact `id` order.
            const ALL: &[Self] = &[ $( Self::$variant ),+ ];

            /// The stable numeric id of this variant, from the framework artifact.
            #[must_use]
            pub const fn id(self) -> u16 {
                match self {
                    $( Self::$variant => $id ),+
                }
            }

            /// Look up a variant by its artifact id, or `None` if no variant has it.
            #[must_use]
            pub fn from_id(id: u16) -> Option<Self> {
                Self::ALL.iter().copied().find(|variant| variant.id() == id)
            }

            /// All variants in artifact `id` order.
            #[must_use]
            pub fn all() -> &'static [Self] {
                Self::ALL
            }

            /// The loaded record for this variant (name, description, and any
            /// framework-specific fields).
            #[must_use]
            pub fn info(self) -> &'static $info {
                &$loader.as_slice()[self as usize]
            }
        }
    };
}

pub(crate) use framework_enum;
