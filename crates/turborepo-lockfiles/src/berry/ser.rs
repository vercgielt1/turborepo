use std::{borrow::Cow, fmt, sync::OnceLock};

use regex::Regex;

use super::{BerryPackage, DependencyMeta, LockfileData, Metadata};

fn simple_string() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"^[^-?:,\]\[{}#&*!|>'"%@` \t\r\n]([ \t]*[^,\]\[{}:# \t\r\n])*$"#).unwrap()
    })
}

const HEADER: &str = "# This file is generated by running \"yarn install\" inside your project.
# Manual changes might be lost - proceed with caution!
";

// We implement Display in order to produce a correctly serialized `yarn.lock`
// Since Berry is so particular about the contents we can't use the serde_yaml
// serializer without forking it and heavy modifications. Implementing Display
// is more honest than writing a Serializer implementation since the serializer
// would only support a single type.
impl fmt::Display for LockfileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{HEADER}\n{}\n", self.metadata)?;

        for (key, entry) in &self.packages {
            let wrapped_key = wrap_string(key);
            // Yaml 1.2 spec says that keys over 1024 characters need to be prefixed with ?
            // and the : goes in a new line
            let key_line = match wrapped_key.len() <= 1024 {
                true => format!("{wrapped_key}:"),
                false => format!("? {wrapped_key}\n:"),
            };
            write!(f, "\n{}\n{}\n", key_line, entry)?;
        }

        Ok(())
    }
}

impl fmt::Display for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "__metadata:\n  version: {}", self.version,)?;
        if let Some(cache_key) = &self.cache_key {
            write!(f, "\n  cacheKey: {}", wrap_string(cache_key))?;
        }
        Ok(())
    }
}

const SPACE: char = ' ';
const NEWLINE: char = '\n';

impl fmt::Display for BerryPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // we only want to write a newline there was something before
        let mut first = true;
        let mut write_line = |field: &str, whitespace: char, value: &str| -> fmt::Result {
            if !value.is_empty() {
                if !first {
                    writeln!(f)?;
                }
                write!(f, "  {field}:{whitespace}{}", value,)?;
                first = false;
            }
            Ok(())
        };

        write_line("version", SPACE, &wrap_string(self.version.as_ref()))?;
        write_line("resolution", SPACE, &wrap_string(&self.resolution))?;
        if let Some(deps) = &self.dependencies {
            write_line(
                "dependencies",
                NEWLINE,
                &stringify_dependencies(deps.iter()),
            )?;
        }
        if let Some(peer_deps) = &self.peer_dependencies {
            write_line(
                "peerDependencies",
                NEWLINE,
                &stringify_dependencies(peer_deps.iter()),
            )?;
        }
        if let Some(deps_meta) = &self.dependencies_meta {
            write_line(
                "dependenciesMeta",
                NEWLINE,
                &stringify_dependencies_meta(deps_meta.iter()),
            )?;
        }
        if let Some(peer_deps_meta) = &self.peer_dependencies_meta {
            write_line(
                "peerDependenciesMeta",
                NEWLINE,
                &stringify_dependencies_meta(peer_deps_meta.iter()),
            )?;
        }
        if let Some(bin) = &self.bin {
            write_line("bin", NEWLINE, &stringify_dependencies(bin.iter()))?;
        }

        if let Some(checksum) = &self.checksum {
            write_line("checksum", SPACE, &wrap_string(checksum))?;
        }
        if let Some(conditions) = &self.conditions {
            write_line("conditions", SPACE, &wrap_string(conditions))?;
        }
        if let Some(language_name) = &self.language_name {
            write_line("languageName", SPACE, &wrap_string(language_name))?;
        }
        if let Some(link_type) = &self.link_type {
            write_line("linkType", SPACE, &wrap_string(link_type))?;
        }

        Ok(())
    }
}

fn stringify_dependencies<I, S1, S2>(entries: I) -> String
where
    I: Iterator<Item = (S1, S2)>,
    S1: AsRef<str>,
    S2: AsRef<str>,
{
    let mut string = String::new();
    let mut first = true;
    for (key, value) in entries {
        let key = key.as_ref();
        let value = value.as_ref();

        if !first {
            string.push('\n');
        }
        string.push_str(&format!("    {}: {}", wrap_string(key), wrap_string(value)));
        first = false;
    }
    string
}

fn stringify_dependencies_meta<'a, I, S>(metadata: I) -> String
where
    I: Iterator<Item = (S, &'a DependencyMeta)>,
    S: AsRef<str>,
{
    let mut string = String::new();
    let mut first = true;

    let mut add_line = |dependency: &str, settings: &[(Option<bool>, &str)]| {
        if !first {
            string.push('\n');
        }

        string.push_str(&format!("    {}:\n", wrap_string(dependency)));

        for (i, (setting, field)) in settings.iter().enumerate() {
            if let Some(value) = setting {
                string.push_str(&format!("      {}: {}", wrap_string(field), value));
                if i < settings.len() - 1 {
                    string.push('\n');
                }
            }
        }

        first = false;
    };

    for (dependency, meta) in metadata {
        let dependency = dependency.as_ref();
        let settings = [
            (meta.built, "built"),
            (meta.optional, "optional"),
            (meta.unplugged, "unplugged"),
        ];
        if settings.iter().any(|&(setting, _)| setting.is_some()) {
            add_line(dependency, &settings);
        }
    }

    string
}

fn wrap_string(s: &str) -> Cow<str> {
    match simple_string().is_match(s) {
        // Simple strings require no wrapping
        true => Cow::from(s),
        // Complex strings require wrapping
        false => {
            Cow::from(serde_json::to_string(s).unwrap_or_else(|_| panic!("Unable to encode '{s}'")))
        }
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn test_metadata_display() {
        let metadata = Metadata {
            version: "6".into(),
            cache_key: Some("8c0".to_string()),
        };
        assert_eq!(
            metadata.to_string(),
            "__metadata:
  version: 6
  cacheKey: 8c0"
        );
    }

    #[test]
    fn test_wrap_string() {
        fn assert(input: &str, expected: &str) {
            assert_eq!(wrap_string(input), expected);
        }
        assert("debug@4.3.4", "debug@4.3.4");
        assert(
            "eslint-module-utils@npm:^2.7.3",
            "\"eslint-module-utils@npm:^2.7.3\"",
        );
        assert("@babel/core", "\"@babel/core\"");
    }

    #[test]
    fn test_long_key_gets_wrapped() {
        let long_key = "a".repeat(1025);
        let lockfile = LockfileData {
            metadata: Metadata {
                version: "6".into(),
                cache_key: Some("8".into()),
            },
            packages: [(
                long_key.clone(),
                BerryPackage {
                    version: "1.2.3".to_string(),
                    ..Default::default()
                },
            )]
            .iter()
            .cloned()
            .collect(),
        };
        let serailized = lockfile.to_string();
        assert!(serailized.contains(&format!("? {long_key}\n")));
    }
}
