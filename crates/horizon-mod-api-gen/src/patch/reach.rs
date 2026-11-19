//! The game modules a patch crate mirrors beyond its copies' own: the prebuilt game widens the
//! items its modules define, never their `use`s, so a path that reaches a name through a module's
//! import (`super::minecarts::CartKind`, which `minecarts` takes from `horizon_gameplay` with a
//! `pub(in crate::game) use`) is a private import in the prebuilt game (E0603). Mirroring that
//! module repeats its `use`s in the patch crate, where they resolve as they did in the game. The
//! paths read are the mirrored modules' `use`s and the copies' tokens, macro arguments included;
//! each newly mirrored module's `use`s are read in turn.
use super::{Source, Tree};
use proc_macro2::{Spacing, TokenStream, TokenTree};
use quote::ToTokens;
use std::collections::BTreeSet;
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
                if let Item::Use(item) = item
                    && !test_only(&item.attrs)
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
            paths_in(tokens, &mut |path| paths.push((path, false)));
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
    fn resolve(&mut self, at: &str, path: &[String], glob: bool, depth: usize) {
        if depth > DEPTH || path.is_empty() {
            return;
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
                        None => return,
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
                        return;
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
                    if let Some(from) = from {
                        let joined: Vec<String> =
                            from.into_iter().chain(path[1..].iter().cloned()).collect();
                        self.resolve(at, &joined, glob, depth + 1);
                    }
                    return;
                }
            }
        };
        for (index, segment) in rest.iter().enumerate() {
            if let Some(child) = self.child(&module, segment) {
                module = child;
                continue;
            }
            let Ok(items) = self.source.module(&module).map(|m| m.items.clone()) else {
                return;
            };
            if defines(&items, segment) {
                return;
            }
            let explicit = import_of(&items, segment);
            let after = &rest[index + 1..];
            match explicit {
                // A public import is public in the prebuilt game too; a mirror's own import
                // resolves in the patch crate. Either way the path goes on from its source.
                Some((import, public)) if public || self.mirrored(&module) => {
                    let joined: Vec<String> =
                        import.into_iter().chain(after.iter().cloned()).collect();
                    self.resolve(&module, &joined, glob, depth + 1);
                }
                Some(_) => {
                    self.needed.insert(module);
                }
                // Through a glob import that is not public, or not the module's at all.
                None if !self.mirrored(&module) && private_globs(&items) => {
                    self.needed.insert(module);
                }
                None => {}
            }
            return;
        }
        // The whole of `module`: a glob of it takes its imports too, which a mirror repeats.
        if glob
            && !self.mirrored(&module)
            && let Ok(items) = self.source.module(&module).map(|m| m.items.clone())
            && items
                .iter()
                .any(|item| matches!(item, Item::Use(u) if !test_only(&u.attrs) && !public(&u.vis)))
        {
            self.needed.insert(module);
        }
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

/// Every path of two or more segments in `tokens` (`a::b::C`), macro arguments included, that is
/// not absolute (`::std::…`) and not a macro variable's (`$x::…`); `$crate::…` is `crate::…`.
fn paths_in(tokens: TokenStream, out: &mut impl FnMut(Vec<String>)) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let colons = |at: usize| {
        matches!(&trees.get(at), Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Joint)
            && matches!(&trees.get(at + 1), Some(TokenTree::Punct(p)) if p.as_char() == ':')
    };
    let mut index = 0;
    while index < trees.len() {
        match &trees[index] {
            TokenTree::Group(group) => {
                paths_in(group.stream(), out);
                index += 1;
            }
            TokenTree::Ident(ident) => {
                let after_colons = index >= 2 && colons(index - 2);
                let after_dollar = index >= 1
                    && matches!(&trees[index - 1], TokenTree::Punct(p) if p.as_char() == '$');
                let mut path = vec![ident.to_string()];
                let mut next = index + 1;
                while colons(next)
                    && let Some(TokenTree::Ident(segment)) = trees.get(next + 2)
                {
                    path.push(segment.to_string());
                    next += 3;
                }
                if path.len() > 1 && !after_colons && (!after_dollar || path[0] == "crate") {
                    out(path);
                }
                index = next;
            }
            _ => index += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Cache, crate_mirror};
    use super::*;
    use quote::quote;
    use std::collections::BTreeMap;
    use std::process::Command;

    /// A prebuilt game whose modules take moved items from a lower crate by imports the
    /// prebuilt game keeps private: a `use` of the game module, restricted imports and a
    /// restricted glob, reached from a copied body by paths (in a macro's arguments too, and from
    /// a name a glob brings). The patch crate must compile against it.
    #[test]
    fn a_copy_reaches_moved_items_through_the_games_private_imports() {
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
        std::fs::write(
            &lower,
            "pub mod api_values { pub trait TryIntoApi { fn api(&self) -> u32; } \
             impl TryIntoApi for u8 { fn api(&self) -> u32 { u32::from(*self) } } }
             pub mod minecarts { #[derive(PartialEq)] pub enum CartKind { Rideable, Tnt } \
             pub const SEAT: u32 = 2; }
             pub mod execution_role { #[derive(PartialEq)] pub enum ExecutionRole { Client } }",
        )
        .unwrap();
        rustc(&[lower.to_str().unwrap()]);
        // The copied function, as written in the game.
        let ride = "pub fn ride(value: u8) -> u32 {
            use super::TryIntoApi;
            let kind = super::minecarts::CartKind::Rideable;
            let role = execution_role::ExecutionRole::Client;
            if matches!(kind, super::minecarts::CartKind::Rideable) && role == execution_role::ExecutionRole::Client {
                value.api() + super::minecarts::SEAT
            } else {
                0
            }
        }";
        // As the staging pass leaves it: items public, imports as written.
        let game = dir.path().join("lib.rs");
        std::fs::write(
            &game,
            format!(
                "pub mod game {{
                    use self::api_values::TryIntoApi;
                    pub mod api_values {{ pub(crate) use gameplay::api_values::TryIntoApi; }}
                    pub mod minecarts {{ pub(in crate::game) use gameplay::minecarts::*; }}
                    pub mod execution_role {{
                        pub(super) use gameplay::execution_role::ExecutionRole;
                    }}
                    pub mod riding {{ use super::*; {ride} }}
                }}"
            ),
        )
        .unwrap();
        rustc(&[
            "--crate-name=horizon_client",
            game.to_str().unwrap(),
            "--extern",
            "gameplay",
        ]);
        let mut source = Source {
            root: dir.path().to_owned(),
            loaded: BTreeSet::new(),
            files: BTreeMap::from([("horizon_client".to_owned(), vec![game.clone()])]),
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
        riding.copies.push(ride.parse().unwrap());
        riding.own.insert("ride".into());
        let body = crate_mirror(&mut source, "horizon_client", &mut tree, false, None).unwrap();
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
    fn paths_are_read_from_tokens_and_macro_arguments() {
        let tokens: TokenStream = syn::parse_quote! {
            fn f() {
                let a = super::minecarts::CartKind::Rideable;
                matches!(k, self::m::K::A);
                ::std::mem::drop(a);
                x.y::<u8>();
                $crate::game::f();
                $v::g();
                Vec::<u8>::new();
            }
        };
        let mut paths = Vec::new();
        paths_in(tokens, &mut |p| paths.push(p.join("::")));
        assert_eq!(
            paths,
            [
                "super::minecarts::CartKind::Rideable",
                "self::m::K::A",
                "crate::game::f",
            ]
        );
    }
}
