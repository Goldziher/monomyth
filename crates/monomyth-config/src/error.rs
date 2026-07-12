//! [`ConfigError`]: the failures reading or parsing a configuration file.

use std::path::PathBuf;

use thiserror::Error;

/// A failure while discovering, reading, or parsing a configuration file.
///
/// A *missing* file is never an error (an absent layer simply does not override);
/// these variants cover a file that exists but cannot be read or does not parse,
/// so a typo in `monomyth.toml` fails loudly rather than silently reverting to
/// defaults.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The file exists but could not be read.
    #[error("reading config file {path}")]
    Read {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file was read but is not valid TOML for the config schema (e.g. a
    /// misspelled or unknown key — the schema uses `deny_unknown_fields`).
    #[error("parsing config file {path}")]
    Parse {
        /// The file that failed to parse.
        path: PathBuf,
        /// The underlying TOML deserialization error.
        #[source]
        source: toml::de::Error,
    },
}
