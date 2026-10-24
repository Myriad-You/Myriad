//! Whether the text of a web page may be read, and why.
//!
//! A page is read only when it is known to be ordinary (a listed site, or a
//! documentation host) or to be other people's words (chat and mail). A
//! sensitive site, an account page on an ordinary site, a page showing a
//! password field, and any site these rules cannot place are not read: not
//! knowing is treated as sensitive.
//!
//! The path and the title are read here on the device and never appear in
//! an output; neither does the host.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::sources::{SourceKind, Sources};

/// What the page is, from the most closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    /// A password field is showing: a sign-in or password form.
    SignIn,
    /// A site whose being open says something private.
    Sensitive,
    /// Settings, security, password, billing or payment pages of any site.
    Account,
    /// Not placed by any rule.
    Unknown,
    /// Other people's words.
    Messaging,
    Ordinary,
}

impl PageKind {
    pub fn read(self) -> bool {
        matches!(self, Self::Messaging | Self::Ordinary)
    }
}

/// Which rule placed the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    PasswordField,
    SensitiveSite,
    AccountPath,
    AccountTitle,
    MessagingSite,
    /// Listed in the table's ordinary sites.
    OrdinarySite,
    /// A documentation host (`docs.`, `developer.`…).
    DocsHost,
    UnknownSite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub kind: PageKind,
    pub basis: Basis,
    /// The source kind, when the table has one.
    pub source: Option<SourceKind>,
    /// What an ordinary site is (`video`, `news`…).
    pub category: Option<String>,
}

impl Page {
    pub fn read(&self) -> bool {
        self.kind.read()
    }

    /// The page's text is other people's words.
    pub fn others_words(&self) -> bool {
        self.kind == PageKind::Messaging
    }
}

/// Path words (or whole segments with `-` and `_` taken out) that make a
/// page an account page. Softer words (`security`, `privacy`, `profile`)
/// do not count alone: `/docs/security` is read, `/settings/security` is not.
const ACCOUNT_WORDS: &[&str] = &[
    "settings",
    "setting",
    "account",
    "accounts",
    "myaccount",
    "mypage",
    "password",
    "passwords",
    "passwd",
    "billing",
    "payment",
    "payments",
    "checkout",
    "wallet",
    "orders",
    "signin",
    "login",
    "signup",
    "register",
    "2fa",
    "mfa",
    "totp",
    "twofactor",
    "passkey",
    "passkeys",
    "credentials",
    "apikeys",
];

/// Title words of account pages, in the languages the text rules cover.
static ACCOUNT_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        "账号设置|账户设置|帐号设置|安全设置|隐私设置|修改密码|支付方式|付款方式|账单|",
        "アカウント設定|セキュリティ設定|パスワード(?:の)?(?:変更|再設定)|お支払い方法|支払い方法|請求",
        r"|(?i:\b(?:account settings|security settings|privacy settings|change password|reset password|payment methods?|billing)\b)",
    ))
    .expect("account title pattern")
});

/// First labels of documentation hosts.
const DOCS_LABELS: &[&str] = &["docs", "doc", "developer", "developers", "devdocs"];

pub fn page(
    host: &str,
    path: Option<&str>,
    title: Option<&str>,
    password_field: bool,
    market: Option<&str>,
    sources: &Sources,
) -> Page {
    let placed = |kind, basis, source, category: Option<&str>| Page {
        kind,
        basis,
        source,
        category: category.map(str::to_string),
    };
    let source = sources.kind(None, Some(host), market);
    if password_field {
        return placed(PageKind::SignIn, Basis::PasswordField, source, None);
    }
    if let Some(kind) = source.filter(|kind| kind.sensitive()) {
        return placed(PageKind::Sensitive, Basis::SensitiveSite, Some(kind), None);
    }
    if path.is_some_and(account_path) {
        return placed(PageKind::Account, Basis::AccountPath, source, None);
    }
    if title.is_some_and(|title| ACCOUNT_TITLE.is_match(title)) {
        return placed(PageKind::Account, Basis::AccountTitle, source, None);
    }
    if source == Some(SourceKind::Messaging) {
        return placed(PageKind::Messaging, Basis::MessagingSite, source, None);
    }
    if let Some(category) = sources.ordinary(host) {
        return placed(
            PageKind::Ordinary,
            Basis::OrdinarySite,
            None,
            Some(category),
        );
    }
    let host = crate::sources::normalize(host);
    let first = host.split('.').next().unwrap_or("");
    if host.contains('.') && DOCS_LABELS.contains(&first) {
        return placed(PageKind::Ordinary, Basis::DocsHost, None, Some("reference"));
    }
    placed(PageKind::Unknown, Basis::UnknownSite, None, None)
}

/// A path with an account word in it.
pub fn account_path(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or("").to_lowercase();
    let word = |word: &str| ACCOUNT_WORDS.contains(&word);
    path.split(['/', '-', '_', '.', '+', '~']).any(word)
        || path
            .split('/')
            .any(|segment| word(&segment.replace(['-', '_'], "")))
}
