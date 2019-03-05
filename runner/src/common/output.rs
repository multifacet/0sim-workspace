//! Utilities for handling generated output.

// TODO: documentation

use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone)]
pub struct OutputManager {
    settings: std::collections::BTreeMap<String, String>,
    important: Vec<String>,
}

impl OutputManager {
    pub fn new() -> Self {
        OutputManager {
            settings: std::collections::BTreeMap::new(),
            important: Vec::new(),
        }
    }

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

    pub fn gen_file_names(&self) -> (String, String) {
        const OUTPUT_SUFFIX: &str = "out";
        const PARAMS_SUFFIX: &str = "params";

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
        base.push_str(
            &chrono::offset::Local::now()
                .format("%Y-%m-%d-%H-%M-%S")
                .to_string(),
        );

        base.push_str(".");

        let base_clone = base.clone();
        (base + OUTPUT_SUFFIX, base_clone + PARAMS_SUFFIX)
    }

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
}

#[macro_export]
macro_rules! settings {
    ($($tail:tt)*) => {{
        let mut manager = crate::common::output::OutputManager::new();

        $crate::__settings_helper!(manager, $($tail)*);

        manager
    }}
}

#[cfg(test)]
mod test {
    #[test]
    fn foo() {
        let settings = settings! {
            git_hash: { 0 + 1},
            workload: "name",

            setting1: false,
            * setting2: String::new(),
            setting3: 3.1E3,
            setting4: (2, 1),
        };

        let (output_file, params_file) = settings.gen_file_names();
        let params_json = serde_json::to_string(settings);
        let git_hash = settings.get::<usize>(GIT_HASH);
    }
}
