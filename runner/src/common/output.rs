//! Utilities for handling and tagging generated output.

use chrono::{offset::Local, DateTime};
use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

/// `OutputManager` manages all things regarding naming and tagging output with settings and
/// properties of its data.
///
/// Each experiment should create an `OutputManager` at the beginning with all of the settings for
/// the experiment. The `settings!` macro helper can be used to do this conveniently. The
/// `OutputManager` can then be used to generate filenames for output files and can generate a
/// `.params` file containing all of the settings.
///
/// The generated filenames will be unique by including a timestamp. They can also optionally
/// contain any settings marked as `important`.
#[derive(Debug, Clone)]
pub struct OutputManager {
    settings: std::collections::BTreeMap<String, String>,
    important: Vec<String>,
    timestamp: DateTime<Local>,
}

impl OutputManager {
    /// Create a new empty `OutputManager` containing now settings.
    pub fn new() -> Self {
        OutputManager {
            settings: std::collections::BTreeMap::new(),
            important: Vec::new(),
            timestamp: Local::now(),
        }
    }

    /// Register a new setting called `name` with value `value`. The boolean value `important`
    /// indicates whether or not the setting should be included in any generated filenames. All
    /// settings must be serializable.
    pub fn register<V: serde::Serialize + std::fmt::Debug>(
        &mut self,
        name: &str,
        value: &V,
        important: bool,
    ) {
        let value = serde_json::to_string(value).expect("unable to serialize");
        if let Some(prev) = self.settings.insert(name.into(), value) {
            panic!(
                "Setting {:?} previously registered with value {:?}",
                name, prev
            );
        }
        if important {
            self.important.push(name.into());
        }
    }

    /// Generate the primary output and params filenames, in that order.
    pub fn gen_standard_names(&self) -> (String, String, String, String) {
        const OUTPUT_SUFFIX: &str = "out";
        const PARAMS_SUFFIX: &str = "params";
        const TIMES_SUFFIX: &str = "time";
        const SIM_SUFFIX: &str = "sim";

        (
            self.gen_file_name(OUTPUT_SUFFIX),
            self.gen_file_name(PARAMS_SUFFIX),
            self.gen_file_name(TIMES_SUFFIX),
            self.gen_file_name(SIM_SUFFIX),
        )
    }

    /// Generate a filename with the given extension. Only use this if you want to generate a file
    /// that is not a `.out` or a `.params` file. The parameter `ext` is the extension without the
    /// leading dot (e.g. `err`).
    pub fn gen_file_name(&self, ext: &str) -> String {
        let mut base = String::new();

        // prepend all important settings
        for (i, setting) in self.important.iter().enumerate() {
            if i > 0 {
                base.push_str("-");
            }
            self.append_setting(&mut base, setting);
        }

        // append the date
        base.push_str("-");
        base.push_str(&self.timestamp.format("%Y-%m-%d-%H-%M-%S").to_string());

        base.push_str(".");
        base.push_str(ext);

        base
    }

    /// Helper to add the given setting to the given string. Used to build file names. The caller
    /// should ensure that the setting is registered.
    fn append_setting(&self, string: &mut String, setting: &str) {
        let val = self
            .settings
            .get(setting)
            .expect("important setting not defined");

        // sanitize
        let val = val.trim();
        let val = val.replace(" ", "_");
        let val = val.replace("\"", "_");
        let val = val.replace("\'", "_");

        string.push_str(setting);
        string.push_str(&val);
    }

    /// Returns the value of setting `setting` deserialized to a `D`.
    ///
    /// # Panics
    ///
    /// - If `setting` is not registered at the time `get` is called.
    /// - If `setting`'s value cannot be deserialized to a `D`.
    pub fn get<'s, 'de, D: serde::Deserialize<'de>>(&'s self, setting: &str) -> D
    where
        's: 'de,
    {
        serde_json::from_str(self.settings.get(setting).expect("no such setting"))
            .expect("unable to deserialize")
    }
}

impl Serialize for OutputManager {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.settings.len()))?;
        for (k, v) in &self.settings {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for OutputManager {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let settings: std::collections::BTreeMap<String, String> =
            Deserialize::deserialize(deserializer)?;

        Ok(Self {
            settings,
            important: Vec::new(),
            timestamp: Local::now(),
        })
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __settings_helper {
    (manager ,) => {};
    ($manager:ident, $name:ident : $value:expr, $($tail:tt)*) => {{
        $manager.register(stringify!($name), &$value, false);
        $crate::__settings_helper!($manager, $($tail)*);
    }};
    ($manager:ident, * $name:ident : $value:expr, $($tail:tt)*) => {{
        $manager.register(stringify!($name), &$value, true);
        $crate::__settings_helper!($manager, $($tail)*);
    }};
    ($manager:ident, ($impt:expr) $name:ident : $value:expr, $($tail:tt)*) => {{
        $manager.register(stringify!($name), &$value, $impt);
        $crate::__settings_helper!($manager, $($tail)*);
    }};
}

/// A convenience macro for creating an `OutputManager` with the given settings. The syntax is `[*]
/// name: value` where `name` is the name of the setting, `value` is any expression that evaluates
/// to the value of the setting, and the `*` is an optional token that signifies that the setting is
/// important. Alternately, `(cond)` can be used to dynamically mark a setting as important
/// depending on their value (e.g. to call attention to a non-default value).
///
/// ```rust
/// let settings: OutputManager = settings! {
///     * workload: if pattern.is_some() { "time_mmap_touch" } else { "memcached_gen_data" },
///     exp: 00000,
///
///     * size: 100, // gb
///     pattern: "-z",
///     calibrated: false,
///
///     (!warmup) warmup: warmup, // `warmup` is marked important if `false`.
/// };
///
/// ```
#[macro_export]
macro_rules! settings {
    ($($tail:tt)*) => {{
        let mut manager = crate::common::output::OutputManager::new();

        $crate::__settings_helper!(manager, $($tail)*);

        manager
    }}
}
