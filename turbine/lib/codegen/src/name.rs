use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{Display, Formatter},
};

use heck::{ToPascalCase, ToSnekCase};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Url;
use type_system::url::VersionedUrl;

use crate::{analysis::DependencyAnalyzer, AnyType};

#[derive(Debug, Copy, Clone)]
pub(crate) enum ModuleFlavor {
    ModRs,
    ModuleRs,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Directory(String);

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct File(String);

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Path(Vec<Directory>, File);

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Name {
    name: String,
    alias: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct Location {
    path: Path,
    name: Name,

    alias: Option<String>,
}

// TODO: what if we create regex masks for this sort of thing with replacements in overrides?
//  like a blockprotocol mask, hash mask, custom mask, to extract the type, with a default mask that
//  simply calls heck
//  custom simply chooses a flat name with heck

// BP: https://blockprotocol.org/@blockprotocol/types/data-type/text/v/1
// HASH: http://localhost:3000/@alice/types/property-type/cbrsUuid/v/1
// I'VE LIVED A LIE FOR MONTHS

/// Pattern matching mode
///
/// We only match path and host/protocol, everything else is stripped
#[derive(Debug, Copy, Clone)]
pub(crate) enum Mode {
    MatchPath,
    MatchAll,
}

impl Mode {
    /// Verification step that panics as this will lead to corruption either way
    ///
    /// Will verify that all named groups required by the [`NameResolver`] are present depending on
    /// the name.
    ///
    /// ## Panics
    ///
    /// If the regex pattern is incomplete or does not have the required capture groups
    fn verify_pattern(self, regex: &Regex) {
        match self {
            Self::MatchPath => {
                // we do not check for extra groups, as they might be used, this is mostly just to
                // encourage future checks
                let mut optional: HashSet<_> = std::iter::once("namespace").collect();
                let mut required: HashSet<_> = ["kind", "id"].into_iter().collect();

                for name in regex.capture_names().flatten() {
                    required.remove(name);
                    optional.remove(name);
                }

                assert!(
                    required.is_empty(),
                    "match path pattern requires `kind` and `id` named groups"
                );
            }
            Self::MatchAll => {
                let mut optional: HashSet<_> = std::iter::once("namespace").collect();
                let mut required: HashSet<_> = ["origin", "kind", "id"].into_iter().collect();

                for name in regex.capture_names().flatten() {
                    required.remove(name);
                    optional.remove(name);
                }

                assert!(
                    required.is_empty(),
                    "match all pattern requires `origin`, `kind` and `id` named groups"
                );
            }
        }
    }
}

pub(crate) struct Flavor {
    name: &'static str,
    mode: Mode,
    pattern: Regex,
}

impl Flavor {
    pub(crate) fn new(name: &'static str, mode: Mode, pattern: Regex) -> Self {
        mode.verify_pattern(&pattern);

        Self {
            name,
            mode,
            pattern,
        }
    }
}

static BLOCKPROTOCOL_FLAVOR: Lazy<Flavor> = Lazy::new(|| {
    let pattern = Regex::new(
        r"/@(?P<namespace>[\w-]+)/types/(?P<kind>data|property|entity)-type/(?P<id>[\w\-_%]+)/",
    )
    .expect("valid pattern");

    Flavor::new("block-protocol", Mode::MatchPath, pattern)
});

static BUILTIN_FLAVORS: &[&Lazy<Flavor>] = &[&BLOCKPROTOCOL_FLAVOR];

#[derive(Debug, Copy, Clone)]
enum Kind {
    Data,
    Property,
    Entity,
}

impl Display for Kind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data => f.write_str("data"),
            Self::Property => f.write_str("property"),
            Self::Entity => f.write_str("entity"),
        }
    }
}

struct DeconstructedUrl<'a> {
    origin: String,
    namespace: Option<&'a str>,
    kind: Kind,
    id: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct OverrideAction {
    matches: String,
    replacement: String,
}

impl OverrideAction {
    pub(crate) fn new(replace: impl Into<String>, with: impl Into<String>) -> Self {
        Self {
            matches: replace.into(),
            replacement: with.into(),
        }
    }
}

pub(crate) struct Override {
    origin: Option<OverrideAction>,
}

impl Override {
    pub(crate) const fn new() -> Self {
        Self { origin: None }
    }

    #[allow(clippy::missing_const_for_fn)]
    pub(crate) fn with_origin(mut self, host: OverrideAction) -> Self {
        self.origin = Some(host);

        self
    }

    fn apply(&self, url: &mut DeconstructedUrl) {
        if let Some(origin) = &self.origin {
            if url.origin == origin.matches {
                url.origin = origin.replacement.clone();
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PropertyName {
    name: String,
}

pub(crate) struct NameResolver<'a> {
    lookup: &'a HashMap<VersionedUrl, AnyType>,
    analyzer: &'a DependencyAnalyzer<'a>,

    overrides: Vec<Override>,
    module: ModuleFlavor,
    flavors: Vec<Flavor>,
}

impl<'a> NameResolver<'a> {
    pub(crate) const fn new(
        lookup: &'a HashMap<VersionedUrl, AnyType>,
        analyzer: &'a DependencyAnalyzer<'a>,
    ) -> Self {
        Self {
            lookup,
            analyzer,

            overrides: Vec::new(),
            module: ModuleFlavor::ModRs,
            flavors: Vec::new(),
        }
    }

    pub(crate) fn with_override(&mut self, value: Override) {
        self.overrides.push(value);
    }

    pub(crate) fn with_module_flavor(&mut self, flavor: ModuleFlavor) {
        self.module = flavor;
    }

    pub(crate) fn with_flavor(&mut self, flavor: Flavor) {
        self.flavors.push(flavor);
    }

    fn deconstruct_url<'b>(&self, url: &'b Url) -> Option<DeconstructedUrl<'b>> {
        let flavors = BUILTIN_FLAVORS
            .iter()
            .map(|flavor| &***flavor)
            .chain(self.flavors.iter());

        for flavor in flavors {
            let target = match flavor.mode {
                Mode::MatchPath => url.path(),
                Mode::MatchAll => url.as_str(),
            };

            let Some(captures)= flavor.pattern.captures(target) else { continue; };

            let origin = match flavor.mode {
                Mode::MatchPath => url.origin().ascii_serialization(),
                Mode::MatchAll => captures
                    .name("origin")
                    .expect("infallible; checked by constructor")
                    .as_str()
                    .to_owned(),
            };

            let namespace = captures.name("namespace").map(|m| m.as_str());

            let kind = captures
                .name("kind")
                .map(|m| m.as_str())
                .expect("infallible; checked by constructor");

            let kind = match kind {
                "data" => Kind::Data,
                "property" => Kind::Property,
                "entity" => Kind::Entity,
                _ => unimplemented!(),
            };

            let id = captures
                .name("id")
                .map(|m| m.as_str())
                .expect("infallible; checked by constructor");

            let mut url = DeconstructedUrl {
                origin,
                namespace,
                kind,
                id,
            };

            for r#override in &self.overrides {
                r#override.apply(&mut url);
            }

            return Some(url);
        }

        None
    }

    /// Return the module location for the structure or enum for the specified URL
    ///
    /// We need to resolve the name and if there are multiple versions we need to make sure that
    /// those are in the correct file! (`mod.rs` vs `module.rs`)
    pub(crate) fn location(&self, id: &VersionedUrl) -> Location {
        let versions: BTreeMap<_, _> = self
            .lookup
            .iter()
            .filter(|(key, _)| key.base_url == id.base_url)
            .filter(|(key, _)| **key != *id)
            .map(|(key, value)| (key.version, value))
            .collect();

        let base_url = id.base_url.to_url();

        let url = self.deconstruct_url(&base_url);

        let mut path = match &url {
            // we don't know the URL, so the file is simply called the snake_case version of the
            // URL
            None => Path(Vec::new(), File(base_url.as_str().to_snek_case())),
            Some(DeconstructedUrl {
                origin,
                namespace,
                kind,
                id,
            }) => {
                let mut directories = vec![Directory(origin.to_snek_case())];

                if let Some(namespace) = namespace {
                    directories.push(Directory(namespace.to_snek_case()));
                }

                directories.push(Directory(kind.to_string()));

                Path(directories, File(id.to_snek_case()))
            }
        };

        let mut name = match url {
            None => self.lookup[id].title().to_pascal_case(),
            Some(DeconstructedUrl { id, .. }) => id.to_pascal_case(),
        };

        // TODO: import vX versions
        // Default handling, if we're the newest version (very often the case), then we also export
        // a versioned identifier to the "default" one.
        let mut alias = Some(format!("{name}V{}", id.version));

        // we need to handle multiple versions, the latest version is always in the `mod.rs`,
        // `module.rs`, while all other files are in `v<N>` files.
        // in the case that there are no other versions, we can just continue and use the name
        // provided earlier.
        if let Some((&other_latest, _)) = versions.last_key_value() {
            if other_latest > id.version {
                // we're an older version, therefore we need to be in a folder

                let File(old) = path.1;
                path.0.push(Directory(old));
                path.1 = File(format!("v{}", id.version));

                // we also need to suffix the version number to the type name to stay consistent
                name = format!("{name}V{}", id.version);

                // the name is the actual alias, so we don't need to export it multiple times
                alias = None;
            } else {
                // we're the newest version, hoist it up to the `module.rs` or `mod.rs` file,
                // depending on flavor.
                match self.module {
                    ModuleFlavor::ModRs => {
                        let File(old) = path.1;
                        path.0.push(Directory(old));
                        path.1 = File("mod".to_owned());
                    }
                    // no change necessary
                    ModuleFlavor::ModuleRs => {}
                }
            }
        }

        Location {
            path,
            name: Name { name, alias },
            alias: None,
        }
    }

    // TODO: pub use previous versions in mod.rs if multiple files

    /// Same as [`Self::location`], but is aware of name clashes and will resolve those properly
    pub(crate) fn locations<'b>(ids: &[&'b VersionedUrl]) -> HashMap<&'b VersionedUrl, Location> {
        todo!()
    }

    /// Return the name of the structure or enum for the specified URL, if there are multiple
    /// versions, older versions will have `V<n>` appended to their name
    // TODO: type alias for current version!
    pub(crate) fn name(id: &VersionedUrl) -> Name {
        todo!()
    }

    // TODO: we need to generate the code for `mod` also!

    // TODO: inner (cannot by done by the name resolver)

    /// Returns the name for the accessor or property for the specified URL
    pub(crate) fn property_name(id: &VersionedUrl) -> PropertyName {
        todo!()
    }

    /// Same as [`Self::property_name`], but is aware of name clashes and will resolve those
    pub(crate) fn property_names<'b>(
        id: &[&'b VersionedUrl],
    ) -> HashMap<&'b VersionedUrl, PropertyName> {
        todo!()
    }
}
