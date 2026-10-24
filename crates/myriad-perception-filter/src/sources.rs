//! Where a window comes from: an app or a site whose being open already
//! says something private (a bank, a clinic, a dating or job site, legal
//! advice, adult content, an account centre, a government identity
//! service), an app whose text is other people's words (chat and mail), a
//! password manager whose windows are never captured, or a site known to be
//! ordinary (see [`crate::page`]).
//!
//! The table is data, kept by market in `shared/perception_sources.json`.
//! Sites match by the listed domain and its subdomains; paths and queries
//! are never looked at, and the host never appears in an output.

use std::{collections::BTreeMap, sync::LazyLock};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Finance,
    Health,
    Dating,
    Job,
    Legal,
    Adult,
    /// An account centre or sign-in site as a whole.
    Account,
    /// A government identity service (My Number, Singpass…).
    GovernmentId,
    /// The text on screen is other people's words.
    Messaging,
}

impl SourceKind {
    /// Every kind but `messaging` puts the window in `present_only`.
    pub fn sensitive(self) -> bool {
        self != Self::Messaging
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    /// Bundle ids whose windows are never captured (password managers).
    #[serde(default)]
    pub capture_excluded: Vec<String>,
    #[serde(default)]
    pub bundles: BTreeMap<String, SourceKind>,
    /// Sites for every market.
    #[serde(default)]
    pub hosts: BTreeMap<String, SourceKind>,
    /// Sites by market (`cn`, `jp`, `sg`…).
    #[serde(default)]
    pub markets: BTreeMap<String, BTreeMap<String, SourceKind>>,
    /// Sites known to be ordinary, by what they are (`video`, `news`,
    /// `reference`, `dev`, `shopping`…). Never outranks a kind above.
    #[serde(default)]
    pub ordinary: BTreeMap<String, String>,
}

static BUILT_IN: LazyLock<Sources> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../shared/perception_sources.json"))
        .expect("shared/perception_sources.json")
});

impl Sources {
    /// The table every client uses.
    pub fn built_in() -> &'static Self {
        &BUILT_IN
    }

    pub fn capture_excluded(&self, bundle: &str) -> bool {
        self.capture_excluded.iter().any(|known| known == bundle)
    }

    /// The site first, then the app. With no market (or one the table does
    /// not know) every market's sites count: unknown reads as more private.
    pub fn kind(
        &self,
        bundle: Option<&str>,
        host: Option<&str>,
        market: Option<&str>,
    ) -> Option<SourceKind> {
        let site = host.map(normalize).and_then(|host| {
            let markets: Vec<&BTreeMap<String, SourceKind>> =
                match market.and_then(|market| self.markets.get(market)) {
                    Some(one) => vec![one],
                    None => self.markets.values().collect(),
                };
            std::iter::once(&self.hosts)
                .chain(markets)
                .filter_map(|table| lookup(table, &host))
                // The most private kind wins when two tables disagree.
                .min()
        });
        site.or_else(|| bundle.and_then(|bundle| self.bundles.get(bundle).copied()))
    }
}

impl Sources {
    /// What an ordinary site is, when the table lists it or a domain above it.
    pub fn ordinary(&self, host: &str) -> Option<&str> {
        let host = normalize(host);
        let mut at = host.as_str();
        loop {
            if let Some(category) = self.ordinary.get(at) {
                return Some(category);
            }
            at = at.split_once('.')?.1;
        }
    }
}

pub(crate) fn normalize(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// The host itself, or the nearest listed domain it sits under.
fn lookup(table: &BTreeMap<String, SourceKind>, host: &str) -> Option<SourceKind> {
    let mut at = host;
    loop {
        if let Some(kind) = table.get(at) {
            return Some(*kind);
        }
        at = at.split_once('.')?.1;
    }
}
