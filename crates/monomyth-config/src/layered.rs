//! [`Layered<T>`]: a resolved configuration value tagged with the
//! [`LayerSource`] that last set it.
//!
//! The layering is the whole mechanism. A value starts as a compiled-in
//! [`LayerSource::SystemDefault`] (the *only* place a default is authored) and is
//! then overridden, in precedence order, by deployment/project/user config files
//! and finally by runtime (CLI) flags. Carrying the source alongside the value is
//! what lets a `--flag` deterministically beat a project file, lets a test assert
//! *where* a value came from, and lets provenance be logged later.

/// Where a resolved configuration value came from, ordered lowest to highest
/// precedence.
///
/// The variant declaration order *is* the precedence order: `Ord` is derived from
/// it, and [`Layered::override_with`] admits an incoming value only when its source
/// is greater than or equal to the current one. So a later, higher layer always
/// wins, and a `RuntimeOverride` (a CLI flag) beats every file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LayerSource {
    /// Compiled-in default. Equals the historical `const`; never read from a file.
    SystemDefault,
    /// A deployment-shipped config file (e.g. `$MONOMYTH_CONFIG_DIR/monomyth.toml`).
    DeploymentDefault,
    /// The project file `./monomyth.toml`.
    ProjectOverride,
    /// The per-user file under the OS config dir (e.g. `~/.config/monomyth/…`).
    UserOverride,
    /// A runtime override, i.e. a CLI flag or a programmatic override. Always wins.
    RuntimeOverride,
}

/// A configuration value together with the highest-precedence [`LayerSource`] that
/// has set it.
///
/// Construct the seed with [`system_default`](Self::system_default), then fold
/// higher layers in with [`override_with`](Self::override_with).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layered<T> {
    value: T,
    source: LayerSource,
}

impl<T> Layered<T> {
    /// The [`LayerSource::SystemDefault`] seed carrying `value`.
    ///
    /// This is the single authoring point for a default: every resolved value
    /// begins here and can only move to a higher layer.
    #[must_use]
    pub const fn system_default(value: T) -> Self {
        Self {
            value,
            source: LayerSource::SystemDefault,
        }
    }

    /// The resolved value.
    #[must_use]
    pub fn get(&self) -> &T {
        &self.value
    }

    /// Consume the wrapper and return the resolved value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }

    /// The layer that last set the resolved value.
    #[must_use]
    pub fn source(&self) -> LayerSource {
        self.source
    }

    /// Apply `incoming` from layer `from`, but only if `incoming` is present and
    /// `from` is at least as high-precedence as the current source.
    ///
    /// Ties (`from == self.source`) go to the incoming value, so re-applying the
    /// same layer overwrites — which is what a repeated runtime override should do.
    /// A `None` incoming is a no-op: an absent key never clears a lower layer.
    pub fn override_with(&mut self, incoming: Option<T>, from: LayerSource) {
        if let Some(value) = incoming
            && from >= self.source
        {
            self.value = value;
            self.source = from;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LayerSource, Layered};

    #[test]
    fn system_default_carries_the_value_and_lowest_source() {
        let layered = Layered::system_default(500u16);
        assert_eq!(*layered.get(), 500);
        assert_eq!(layered.source(), LayerSource::SystemDefault);
    }

    #[test]
    fn a_higher_layer_overrides_a_lower_one() {
        let mut layered = Layered::system_default(1u16);
        layered.override_with(Some(2), LayerSource::ProjectOverride);
        assert_eq!(*layered.get(), 2);
        assert_eq!(layered.source(), LayerSource::ProjectOverride);
    }

    #[test]
    fn a_lower_layer_does_not_override_a_higher_one() {
        let mut layered = Layered::system_default(1u16);
        layered.override_with(Some(2), LayerSource::UserOverride);
        layered.override_with(Some(3), LayerSource::SystemDefault);
        assert_eq!(*layered.get(), 2);
        assert_eq!(layered.source(), LayerSource::UserOverride);
    }

    #[test]
    fn a_runtime_override_beats_every_file_layer() {
        let mut layered = Layered::system_default(1u16);
        layered.override_with(Some(2), LayerSource::ProjectOverride);
        layered.override_with(Some(3), LayerSource::UserOverride);
        layered.override_with(Some(4), LayerSource::RuntimeOverride);
        assert_eq!(*layered.get(), 4);
        assert_eq!(layered.source(), LayerSource::RuntimeOverride);
    }

    #[test]
    fn a_repeated_layer_overwrites_on_the_tie() {
        let mut layered = Layered::system_default(1u16);
        layered.override_with(Some(4), LayerSource::RuntimeOverride);
        layered.override_with(Some(5), LayerSource::RuntimeOverride);
        assert_eq!(*layered.get(), 5, "an equal-precedence layer overwrites");
    }

    #[test]
    fn an_absent_incoming_value_is_a_no_op() {
        let mut layered = Layered::system_default(7u16);
        layered.override_with(None, LayerSource::RuntimeOverride);
        assert_eq!(*layered.get(), 7);
        assert_eq!(layered.source(), LayerSource::SystemDefault);
    }
}
