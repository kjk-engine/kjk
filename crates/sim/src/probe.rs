//! What the probe groups (`kjk sim probes`) share.

/// A group's checks, each by name: passed with what it saw, or failed with
/// it.
pub type Checks = Vec<(&'static str, Result<String, String>)>;

/// What a check saw, passed when `ok`.
pub fn check(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
}

/// The checks `names`, each failed with the same `why`.
pub fn failed<const N: usize>(names: [&'static str; N], why: String) -> Checks {
    names.map(|name| (name, Err(why.clone()))).into()
}

/// A script library for a probe: `catalog`'s routines (as `nwscript.nss`
/// declares them, each after its `// N`) and `scripts` by name, with no
/// talents.
pub(crate) fn library<N: Into<String>>(
    catalog: &str,
    scripts: impl IntoIterator<Item = (N, fmt_odyssey::ncs::Ncs)>,
) -> crate::script::Library {
    library_with(catalog, scripts, crate::script::Talents::default())
}

/// A library with no scripts holding the creature templates `creatures`
/// and the item templates `items`.
pub fn library_of(
    creatures: impl IntoIterator<Item = crate::module::ObjectData>,
    items: crate::inventory::Catalog,
) -> crate::script::Library {
    let creatures = creatures
        .into_iter()
        .map(|facts| (facts.template.clone(), std::sync::Arc::new(facts)))
        .collect();
    library("", std::iter::empty::<(String, fmt_odyssey::ncs::Ncs)>())
        .with_creatures(creatures)
        .with_items(items)
}

/// `library`, with `talents`.
pub(crate) fn library_with<N: Into<String>>(
    catalog: &str,
    scripts: impl IntoIterator<Item = (N, fmt_odyssey::ncs::Ncs)>,
    talents: crate::script::Talents,
) -> crate::script::Library {
    crate::script::Library::new(
        script_ncs::catalog::Catalog::parse(catalog).expect("the probe's catalog parses"),
        scripts
            .into_iter()
            .map(|(name, ncs)| (name.into(), ncs))
            .collect(),
        talents,
    )
}
