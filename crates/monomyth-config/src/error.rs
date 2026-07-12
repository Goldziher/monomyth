//! [`ConfigError`]: the failures reading, parsing, or validating a configuration
//! file.

use std::path::PathBuf;

use thiserror::Error;

/// A failure while discovering, reading, parsing, or validating a configuration
/// file.
///
/// A *missing* file is never an error (an absent layer simply does not override);
/// these variants cover a file that exists but cannot be read, does not parse, or
/// resolves to an out-of-range value — so a mistake in `monomyth.toml` fails loudly
/// (and *informatively*: the underlying cause is part of the `Display` string)
/// rather than silently reverting to defaults.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file exists but could not be read.
    #[error("reading config file {path}: {source}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file was read but is not valid TOML for the config schema (e.g. a
    /// misspelled or unknown key — the schema uses `deny_unknown_fields`).
    #[error("parsing config file {path}: {source}")]
    Parse {
        /// The file that failed to parse.
        path: PathBuf,
        /// The underlying TOML deserialization error.
        #[source]
        source: toml::de::Error,
    },
    /// The file parsed, but a resolved value is out of range (e.g. a permille over
    /// `1000`, or an inverted `min`/`max` bound). Caught before the value can reach
    /// a generation pass, where it could panic or silently empty an RNG draw.
    #[error("invalid configuration: {field}: {reason}")]
    Invalid {
        /// The offending field, in `section.key` form (e.g. `generation.rooms_max`).
        field: String,
        /// Why the value is rejected.
        reason: String,
    },
}
