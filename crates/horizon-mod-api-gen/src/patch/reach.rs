//! The game modules a patch crate mirrors beyond its copies' own: the prebuilt game widens the
//! items its modules define, never their `use`s, so a path that reaches a name through a module's
//! import (`super::minecarts::CartKind`, which `minecarts` takes from `horizon_gameplay` with a
//! `pub(in crate::game) use`) is a private import in the prebuilt game (E0603). Mirroring that
//! module repeats its `use`s in the patch crate, where they resolve as they did in the game. The
//! paths read are the mirrored modules' `use`s and the copies' tokens, macro arguments included;
//! each newly mirrored module's `use`s are read in turn.
use super::{Source, Tree};
use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use quote::ToTokens;
use std::collections::{BTreeMap, BTreeSet};
use syn::Item;

/// How many imports a path is followed through before it is given up on.
const DEPTH: usize = 16;

/// Adds to `tree`, the mirror of crate `krate`, every module of it that the mirror's `use`s or
/// copies reach through an import, until none is added.
pub(super) fn extend(source: &mut Source, krate: &str, tree: &mut Tree) {
    let mut read = BTreeSet::new();
    loop {
        let mut mirrored = BTreeSet::new();
        nodes(tree, krate, &mut mirrored);
        let pending: Vec<String> = mirrored.difference(&read).cloned().collect();
        if pending.is_empty() {
            return;
        }
        let mut reach = Reach {
            source,
            mirrored,
            needed: BTreeSet::new(),
        };
        for module in pending {
            let Ok(items) = reach.source.module(&module).map(|m| m.items.clone()) else {
                read.insert(module);
                continue;
            };
            let mut paths = Vec::new();
            for item in &items {
                // A `use` under a `cfg` is left out: it may name a module the build does not have,
                // whose mirror would not compile. None of the game's modules has one that does.
                if let Item::Use(item) = item
                    && !item.attrs.iter().any(|attr| attr.path().is_ident("cfg"))
                    && item.leading_colon.is_none()
                {
                    let mut found = Vec::new();
                    leaves(&item.tree, &mut Vec::new(), &mut found);
                    paths.extend(
                        found
                            .into_iter()
                            .map(|leaf| (leaf.path, leaf.name.is_none())),
                    );
                }
            }
            let mut tokens = TokenStream::new();
            if let Some(node) = node(tree, krate, &module) {
                tokens.extend(node.copies.iter().cloned());
            }
            for mac in reach.source.scope_macros(&module) {
                mac.to_tokens(&mut tokens);
            }
            let mut copied = Scan::default();
            scan(tokens, 0, &mut copied);
            for (mut path, glob) in copied.paths {
                // A name the copy's own `use` binds (`use super::minecarts as carts;`).
                if let Some(bound) = copied.local.get(&path[0]) {
                    path.splice(0..1, bound.iter().cloned());
                }
                paths.push((path, glob));
            }
            for (path, glob) in paths {
                reach.resolve(&module, &path, glob, 0);
            }
            read.insert(module);
        }
        let Reach { needed, .. } = reach;
        for module in needed {
            let mut node = &mut *tree;
            for segment in module.split("::").skip(1) {
                node = node.children.entry(segment.to_owned()).or_default();
            }
        }
    }
}

struct Reach<'a> {
    source: &'a mut Source,
    mirrored: BTreeSet<String>,
    /// The modules found to need a mirror this round.
    needed: BTreeSet<String>,
}

impl Reach<'_> {
    fn mirrored(&self, module: &str) -> bool {
        self.mirrored.contains(module) || self.needed.contains(module)
    }

    fn child(&mut self, module: &str, name: &str) -> Option<String> {
        let child = format!("{module}::{name}");
        self.source.is_module(&child).then_some(child)
    }

    /// The module of the crate `path`, written in module `at`, names through modules alone.
    fn module_of(&mut self, at: &str, path: &[String]) -> Option<String> {
        let mut module = at.to_owned();
        for (index, segment) in path.iter().enumerate() {
            module = match segment.as_str() {
                "crate" if index == 0 => at.split("::").next().unwrap_or(at).to_owned(),
                "self" if index == 0 => module,
                "super" => module.rsplit_once("::")?.0.to_owned(),
                name => self.child(&module, name)?,
            };
        }
        Some(module)
    }

    /// Follows `path`, written in module `at`, through the crate's modules; `glob` for a
    /// `use path::*`. A module whose import the path passes through is marked to be mirrored.
    /// Returns whether the path ends on an import of a mirror: a public import that forwards to
    /// one (`pub use super::minecarts as carts;`) needs its own module mirrored too, or a path
    /// through it would reach the prebuilt game's module instead of the mirror.
    fn resolve(&mut self, at: &str, path: &[String], glob: bool, depth: usize) -> bool {
        if depth > DEPTH || path.is_empty() {
            return false;
        }
        let first = path[0].as_str();
        let (mut module, rest) = match first {
            "crate" => (at.split("::").next().unwrap_or(at).to_owned(), &path[1..]),
            "self" => (at.to_owned(), &path[1..]),
            "super" => {
                let ups = path.iter().take_while(|s| *s == "super").count();
                let mut module = at;
                for _ in 0..ups {
                    match module.rsplit_once("::") {
                        Some((parent, _)) => module = parent,
                        None => return false,
                    }
                }
                (module.to_owned(), &path[ups..])
            }
            name => {
                if let Some(child) = self.child(at, name) {
                    (child, &path[1..])
                } else {
                    // A name `at` imports: followed from where the import takes it.
                    let Ok(items) = self.source.module(at).map(|m| m.items.clone()) else {
                        return false;
                    };
                    let from = import_of(&items, name)
                        .map(|(import, _)| import)
                        .or_else(|| {
                            // Or a glob of a module of the crate that has it (`use super::*;`).
                            globs(&items).into_iter().find_map(|mut prefix| {
                                let module = self.module_of(at, &prefix)?;
                                let has = self.child(&module, name).is_some()
                                    || self.source.module(&module).is_ok_and(|m| {
                                        defines(&m.items, name)
                                            || import_of(&m.items, name).is_some()
                                    });
                                has.then(|| {
                                    prefix.push(name.to_owned());
                                    prefix
                                })
                            })
                        });
                    let Some(from) = from else {
                        return false;
                    };
                    let joined: Vec<String> =
                        from.into_iter().chain(path[1..].iter().cloned()).collect();
                    return self.resolve(at, &joined, glob, depth + 1);
                }
            }
        };
        for (index, segment) in rest.iter().enumerate() {
            if let Some(child) = self.child(&module, segment) {
                module = child;
                continue;
            }
            let Ok(items) = self.source.module(&module).map(|m| m.items.clone()) else {
                return false;
            };
            if defines(&items, segment) {
                return false;
            }
            let after = &rest[index + 1..];
            let mirrored = self.mirrored(&module);
            return match import_of(&items, segment) {
                // A mirror's own import resolves in the patch crate; the path goes on from its
                // source.
                Some((import, _)) if mirrored => {
                    let joined: Vec<String> =
                        import.into_iter().chain(after.iter().cloned()).collect();
                    self.resolve(&module, &joined, glob, depth + 1);
                    true
                }
                // A public import is public in the prebuilt game too, unless what it forwards to
                // ends on a mirror's import.
                Some((import, true)) => {
                    let joined: Vec<String> =
                        import.into_iter().chain(after.iter().cloned()).collect();
                    let through = self.resolve(&module, &joined, glob, depth + 1);
                    if through {
                        self.needed.insert(module);
                    }
                    through
                }
                Some(_) => {
                    self.needed.insert(module);
                    true
                }
                // Through a glob import, or not the module's at all.
                None if mirrored => true,
                None if private_globs(&items) => {
                    self.needed.insert(module);
                    true
                }
                None => false,
            };
        }
        // The whole of `module`: a glob of it takes its imports too, which a mirror repeats.
        let imports = glob
            && self.source.module(&module).is_ok_and(|m| {
                m.items.iter().any(
                    |item| matches!(item, Item::Use(u) if !test_only(&u.attrs) && !public(&u.vis)),
                )
            });
        if imports && !self.mirrored(&module) {
            self.needed.insert(module);
        }
        imports
    }
}

/// The paths of the modules in `tree`, the mirror of `path`.
fn nodes(tree: &Tree, path: &str, out: &mut BTreeSet<String>) {
    out.insert(path.to_owned());
    for (name, child) in &tree.children {
        nodes(child, &format!("{path}::{name}"), out);
    }
}

fn node<'a>(tree: &'a Tree, krate: &str, module: &str) -> Option<&'a Tree> {
    let rest = module.strip_prefix(krate)?;
    let mut node = tree;
    for segment in rest.split("::").filter(|s| !s.is_empty()) {
        node = node.children.get(segment)?;
    }
    Some(node)
}

fn public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg") && attr.to_token_stream().to_string().contains("test"))
    })
}

/// Whether `items` define `name` themselves: an item the prebuilt game makes public.
fn defines(items: &[Item], name: &str) -> bool {
    items.iter().any(|item| {
        let ident = match item {
            Item::Fn(item) => &item.sig.ident,
            Item::Struct(item) => &item.ident,
            Item::Enum(item) => &item.ident,
            Item::Union(item) => &item.ident,
            Item::Const(item) => &item.ident,
            Item::Static(item) => &item.ident,
            Item::Type(item) => &item.ident,
            Item::Trait(item) => &item.ident,
            Item::Mod(item) => &item.ident,
            _ => return false,
        };
        ident == name
    })
}

/// Where the explicit `use` of `items` that binds `name` takes it from, and whether it is public.
fn import_of(items: &[Item], name: &str) -> Option<(Vec<String>, bool)> {
    items.iter().find_map(|item| match item {
        Item::Use(item) if !test_only(&item.attrs) && item.leading_colon.is_none() => {
            let mut found = Vec::new();
            leaves(&item.tree, &mut Vec::new(), &mut found);
            found
                .into_iter()
                .find(|leaf| leaf.name.as_deref() == Some(name))
                .map(|leaf| (leaf.path, public(&item.vis)))
        }
        _ => None,
    })
}

/// The paths of the glob imports of `items` (`use super::*;` is `[super]`).
fn globs(items: &[Item]) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    for item in items {
        if let Item::Use(item) = item
            && !test_only(&item.attrs)
            && item.leading_colon.is_none()
        {
            leaves(&item.tree, &mut Vec::new(), &mut found);
        }
    }
    found
        .into_iter()
        .filter(|leaf| leaf.name.is_none())
        .map(|leaf| leaf.path)
        .collect()
}

/// Whether `items` have a glob import that is not public.
fn private_globs(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Use(item) if !test_only(&item.attrs) && !public(&item.vis) => {
            let mut found = Vec::new();
            leaves(&item.tree, &mut Vec::new(), &mut found);
            found.iter().any(|leaf| leaf.name.is_none())
        }
        _ => false,
    })
}

/// One leaf of a `use` tree: the path it imports and the name it binds, none for a glob
/// (`a::b::*` is `[a, b]`).
struct Leaf {
    path: Vec<String>,
    name: Option<String>,
}

fn leaves(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<Leaf>) {
    let leaf = |path: Vec<String>, name: Option<String>| Leaf { path, name };
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            leaves(&path.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(name) if name.ident == "self" => {
            out.push(leaf(prefix.clone(), prefix.last().cloned()));
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix.clone();
            path.push(name.ident.to_string());
            out.push(leaf(path, Some(name.ident.to_string())));
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            if rename.ident != "self" {
                path.push(rename.ident.to_string());
            }
            out.push(leaf(path, Some(rename.rename.to_string())));
        }
        syn::UseTree::Glob(_) => out.push(leaf(prefix.clone(), None)),
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                leaves(tree, prefix, out);
            }
        }
    }
}

/// What a copy's tokens name: each path of two or more segments (`a::b::C`), macro arguments
/// included, with whether it is a glob `use`, and the names the copy's own `use`s bind.
#[derive(Default)]
struct Scan {
    paths: Vec<(Vec<String>, bool)>,
    local: BTreeMap<String, Vec<String>>,
}

/// Reads `tokens`, `depth` inline modules (`mod helper { … }`) below the copy's module, into
/// `out`. A path is kept as it reads from the copy's module, and left out where it stays inside
/// the inline modules. An absolute path (`::std::…`) and a macro variable's (`$x::…`) are not
/// the crate's; `$crate::…` is `crate::…`.
fn scan(tokens: TokenStream, depth: usize, out: &mut Scan) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let punct =
        |at: usize, c: char| matches!(trees.get(at), Some(TokenTree::Punct(p)) if p.as_char() == c);
    let colons = |at: usize| {
        matches!(trees.get(at), Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Joint)
            && punct(at + 1, ':')
    };
    let mut index = 0;
    while index < trees.len() {
        let after_colons = index >= 2 && colons(index - 2);
        match &trees[index] {
            TokenTree::Group(group) => {
                scan(group.stream(), depth, out);
                index += 1;
            }
            TokenTree::Ident(ident)
                if ident == "mod"
                    && matches!(trees.get(index + 1), Some(TokenTree::Ident(_)))
                    && matches!(trees.get(index + 2), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace) =>
            {
                if let Some(TokenTree::Group(body)) = trees.get(index + 2) {
                    scan(body.stream(), depth + 1, out);
                }
                index += 3;
            }
            TokenTree::Ident(ident) if ident == "use" && !after_colons => {
                let end = (index..trees.len())
                    .find(|&at| punct(at, ';'))
                    .unwrap_or(trees.len() - 1);
                let statement: TokenStream = trees[index..=end].iter().cloned().collect();
                if let Ok(item) = syn::parse2::<syn::ItemUse>(statement)
                    && item.leading_colon.is_none()
                {
                    let mut found = Vec::new();
                    leaves(&item.tree, &mut Vec::new(), &mut found);
                    for leaf in found {
                        let Some(path) = from_copy(leaf.path, depth) else {
                            continue;
                        };
                        if depth == 0
                            && let Some(name) = &leaf.name
                        {
                            out.local.insert(name.clone(), path.clone());
                        }
                        out.paths.push((path, leaf.name.is_none()));
                    }
                }
                index = end + 1;
            }
            TokenTree::Ident(ident) => {
                let after_dollar = index >= 1 && punct(index - 1, '$');
                let mut path = vec![ident.to_string()];
                let mut next = index + 1;
                while colons(next)
                    && let Some(TokenTree::Ident(segment)) = trees.get(next + 2)
                {
                    path.push(segment.to_string());
                    next += 3;
                }
                if path.len() > 1
                    && !after_colons
                    && (!after_dollar || path[0] == "crate")
                    && let Some(path) = from_copy(path, depth)
                {
                    out.paths.push((path, false));
                }
                index = next;
            }
            _ => index += 1,
        }
    }
}

/// `path`, written `depth` inline modules below the copy's module, as it reads from that module;
/// none where it stays inside the inline modules.
fn from_copy(path: Vec<String>, depth: usize) -> Option<Vec<String>> {
    if depth == 0 || path[0] == "crate" {
        return Some(path);
    }
    let ups = path.iter().take_while(|s| *s == "super").count();
    match ups.cmp(&depth) {
        std::cmp::Ordering::Less => None,
        std::cmp::Ordering::Equal if path.len() > ups => Some(
            std::iter::once("self".to_owned())
                .chain(path[ups..].iter().cloned())
                .collect(),
        ),
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Greater => Some(path[depth..].to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Cache, crate_mirror};
    use super::*;
    use quote::quote;
    use std::path::Path;
    use std::process::Command;

    /// The lower crate the moved items are in.
    const GAMEPLAY: &str = "
        pub mod api_values {
            pub trait TryIntoApi { fn api(&self) -> u32; }
            impl TryIntoApi for u8 { fn api(&self) -> u32 { u32::from(*self) } }
        }
        pub mod minecarts {
            #[derive(PartialEq)] pub enum CartKind { Rideable, Tnt }
            pub const SEAT: u32 = 2;
        }
        pub mod execution_role { #[derive(PartialEq)] pub enum ExecutionRole { Client } }
        pub mod rails { pub const GAUGE: u32 = 3; }
        pub mod depots { pub const DEPTH: u32 = 4; }
        pub mod tracks { pub const WIDTH: u32 = 5; }
        pub mod lanes { pub const LANE: u32 = 6; }
    ";

    /// The copied function, as written in the game: it reaches each moved item through an import
    /// the prebuilt game keeps private, from a game module's `use`, a body path (in a macro's
    /// arguments too), a name a glob brings, a glob and an alias of its own, and a public
    /// forwarding module.
    const RIDE: &str = "pub fn ride(value: u8) -> u32 {
        use super::TryIntoApi;
        use super::rails::*;
        use super::depots as yard;
        let kind = super::minecarts::CartKind::Rideable;
        let role = execution_role::ExecutionRole::Client;
        if matches!(kind, super::minecarts::CartKind::Rideable)
            && role == execution_role::ExecutionRole::Client
        {
            value.api() + super::minecarts::SEAT + GAUGE + yard::DEPTH
                + super::facade::carts::WIDTH + helper::lane()
        } else {
            0
        }
    }";

    /// A support module beside the copy: its paths leave it with `super`.
    const HELPER: &str = "pub mod helper { pub fn lane() -> u32 { super::super::lanes::LANE } }";

    /// The game crate as the staging pass leaves it, items public and imports as written, into
    /// `dir`; and the patch crate's mirror of it with `RIDE` and `HELPER` copied.
    fn mirrored(dir: &Path) -> TokenStream {
        let game = dir.join("lib.rs");
        std::fs::write(
            &game,
            format!(
                "pub mod game {{
                    use self::api_values::TryIntoApi;
                    #[cfg(any())]
                    use self::quiet::LANE;
                    pub mod api_values {{ pub(crate) use gameplay::api_values::TryIntoApi; }}
                    pub mod minecarts {{ pub(in crate::game) use gameplay::minecarts::*; }}
                    pub mod execution_role {{
                        pub(super) use gameplay::execution_role::ExecutionRole;
                    }}
                    pub mod rails {{ pub(in crate::game) use gameplay::rails::*; }}
                    pub mod depots {{ pub(crate) use gameplay::depots::DEPTH; }}
                    pub mod tracks {{ pub(crate) use gameplay::tracks::WIDTH; }}
                    pub mod facade {{ pub use super::tracks as carts; }}
                    pub mod lanes {{ pub(crate) use gameplay::lanes::LANE; }}
                    pub mod quiet {{ pub(crate) use gameplay::lanes::LANE; }}
                    pub mod riding {{ use super::*; {RIDE} {HELPER} }}
                }}"
            ),
        )
        .unwrap();
        let mut source = Source {
            root: dir.to_owned(),
            loaded: BTreeSet::new(),
            files: BTreeMap::from([("horizon_client".to_owned(), vec![game])]),
            parsed: BTreeMap::new(),
            local_macros: BTreeSet::new(),
            libraries: BTreeMap::new(),
            owners: BTreeMap::new(),
            cache: Cache::default(),
            hashes: BTreeMap::new(),
            stale: None,
        };
        let mut tree = Tree::default();
        let riding = tree
            .children
            .entry("game".into())
            .or_default()
            .children
            .entry("riding".into())
            .or_default();
        riding.copies.push(RIDE.parse().unwrap());
        riding.copies.push(HELPER.parse().unwrap());
        riding.own.extend(["ride".to_owned(), "helper".to_owned()]);
        crate_mirror(&mut source, "horizon_client", &mut tree, false, None).unwrap()
    }

    /// The modules of `items` that are mirrored (`pub mod x { … }`), by path below `prefix`.
    fn modules(items: &[Item], prefix: &str, out: &mut BTreeSet<String>) {
        for item in items {
            if let Item::Mod(module) = item
                && let Some((_, inner)) = &module.content
            {
                let path = format!("{prefix}::{}", module.ident);
                modules(inner, &path, out);
                out.insert(path);
            }
        }
    }

    #[test]
    fn a_copy_mirrors_the_modules_whose_imports_it_reaches() {
        let dir = tempfile::tempdir().unwrap();
        let file: syn::File = syn::parse2(mirrored(dir.path())).unwrap();
        let mut found = BTreeSet::new();
        modules(&file.items, "crate", &mut found);
        // `quiet` is reached only by a `use` its `cfg` leaves out of the build.
        let expected = [
            "game",
            "game::api_values",
            "game::depots",
            "game::execution_role",
            "game::facade",
            "game::lanes",
            "game::minecarts",
            "game::rails",
            "game::riding",
            "game::riding::helper",
            "game::tracks",
        ]
        .map(|m| format!("crate::{m}"));
        assert_eq!(found, BTreeSet::from(expected));
    }

    /// The mirror compiles against a prebuilt game whose imports are private.
    #[test]
    #[ignore = "daily"]
    fn the_patch_crate_compiles_against_the_games_private_imports() {
        let dir = tempfile::tempdir().unwrap();
        let rustc = |args: &[&str]| {
            let output = Command::new("rustc")
                .args(["--edition=2024", "--crate-type=rlib", "-L"])
                .arg(dir.path())
                .arg("--out-dir")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        let lower = dir.path().join("gameplay.rs");
        std::fs::write(&lower, GAMEPLAY).unwrap();
        rustc(&[lower.to_str().unwrap()]);
        let body = mirrored(dir.path());
        let game = dir.path().join("lib.rs");
        rustc(&[
            "--crate-name=horizon_client",
            game.to_str().unwrap(),
            "--extern",
            "gameplay",
        ]);
        let patch = dir.path().join("patch.rs");
        std::fs::write(
            &patch,
            quote!(#![allow(unused_imports, dead_code)] #body).to_string(),
        )
        .unwrap();
        rustc(&[
            "--crate-name=horizon_patched",
            patch.to_str().unwrap(),
            "--extern",
            "horizon_client",
            "--extern",
            "gameplay",
        ]);
    }

    #[test]
    fn copied_paths_are_read_as_their_module_reads_them() {
        let tokens: TokenStream = syn::parse_quote! {
            fn f() {
                use super::minecarts as carts;
                use super::rails::*;
                let a = super::minecarts::CartKind::Rideable;
                matches!(k, self::m::K::A);
                ::std::mem::drop(a);
                x.y::<u8>();
                $crate::game::f();
                $v::g();
                Vec::<u8>::new();
            }
            mod helper {
                fn h() { super::super::lanes::LANE; super::sibling::X; self::inner::Y; }
            }
        };
        let mut found = Scan::default();
        scan(tokens, 0, &mut found);
        let paths: Vec<(String, bool)> = found
            .paths
            .iter()
            .map(|(path, glob)| (path.join("::"), *glob))
            .collect();
        let expect = |path: &str, glob| (path.to_owned(), glob);
        assert_eq!(
            paths,
            [
                expect("super::minecarts", false),
                expect("super::rails", true),
                expect("super::minecarts::CartKind::Rideable", false),
                expect("self::m::K::A", false),
                expect("crate::game::f", false),
                expect("super::lanes::LANE", false),
                expect("self::sibling::X", false),
            ]
        );
        assert_eq!(
            found.local,
            BTreeMap::from([(
                "carts".to_owned(),
                vec!["super".to_owned(), "minecarts".to_owned()]
            )])
        );
    }
}
