//! What the probe groups (`kjk sim probes`) share.

/// A group's checks, each by name: passed with what it saw, or failed with
/// it.
pub type Checks = Vec<(&'static str, Result<String, String>)>;

/// What a check saw, passed when `ok`.
pub fn check(ok: bool, what: String) -> Result<String, String> {
    if ok { Ok(what) } else { Err(what) }
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
