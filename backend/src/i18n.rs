//! Compiled-in JSON catalogs for user-visible backend copy.

use std::collections::HashMap;
use std::sync::LazyLock;

type Catalog = HashMap<String, String>;

fn parse_catalog(raw: &'static str) -> Catalog {
    serde_json::from_str(raw).unwrap_or_else(|error| {
        panic!("invalid i18n catalog JSON: {error}");
    })
}

fn catalog_for(
    table: &'static LazyLock<HashMap<&'static str, Catalog>>,
    locale: &str,
) -> &'static Catalog {
    let loc = crate::api::reports::locale::normalize_report_locale(locale);
    table
        .get(loc)
        .or_else(|| table.get("en-US"))
        .expect("en-US catalog must exist")
}

static REPORTS: LazyLock<HashMap<&'static str, Catalog>> = LazyLock::new(|| {
    HashMap::from([
        (
            "en-US",
            parse_catalog(include_str!("../i18n/reports.en-US.json")),
        ),
        (
            "zh-CN",
            parse_catalog(include_str!("../i18n/reports.zh-CN.json")),
        ),
        (
            "zh-TW",
            parse_catalog(include_str!("../i18n/reports.zh-TW.json")),
        ),
        (
            "ja-JP",
            parse_catalog(include_str!("../i18n/reports.ja-JP.json")),
        ),
        (
            "ko-KR",
            parse_catalog(include_str!("../i18n/reports.ko-KR.json")),
        ),
        (
            "fr-FR",
            parse_catalog(include_str!("../i18n/reports.fr-FR.json")),
        ),
        (
            "de-DE",
            parse_catalog(include_str!("../i18n/reports.de-DE.json")),
        ),
    ])
});

static SEO: LazyLock<HashMap<&'static str, Catalog>> = LazyLock::new(|| {
    HashMap::from([
        (
            "en-US",
            parse_catalog(include_str!("../i18n/seo.en-US.json")),
        ),
        (
            "zh-CN",
            parse_catalog(include_str!("../i18n/seo.zh-CN.json")),
        ),
        (
            "zh-TW",
            parse_catalog(include_str!("../i18n/seo.zh-TW.json")),
        ),
        (
            "ja-JP",
            parse_catalog(include_str!("../i18n/seo.ja-JP.json")),
        ),
        (
            "ko-KR",
            parse_catalog(include_str!("../i18n/seo.ko-KR.json")),
        ),
        (
            "fr-FR",
            parse_catalog(include_str!("../i18n/seo.fr-FR.json")),
        ),
        (
            "de-DE",
            parse_catalog(include_str!("../i18n/seo.de-DE.json")),
        ),
    ])
});

fn lookup(
    table: &'static LazyLock<HashMap<&'static str, Catalog>>,
    locale: &str,
    key: &str,
) -> String {
    let catalog = catalog_for(table, locale);
    if let Some(value) = catalog.get(key) {
        return value.clone();
    }
    REPORTS
        .get("en-US")
        .and_then(|fallback| fallback.get(key))
        .cloned()
        .or_else(|| {
            SEO.get("en-US")
                .and_then(|fallback| fallback.get(key))
                .cloned()
        })
        .unwrap_or_else(|| key.to_string())
}

pub fn reports(locale: &str, key: &str) -> String {
    lookup(&REPORTS, locale, key)
}

pub fn seo(locale: &str, key: &str) -> String {
    lookup(&SEO, locale, key)
}

pub fn seo_f(locale: &str, key: &str, params: &[(&str, &str)]) -> String {
    let mut value = seo(locale, key);
    for (name, replacement) in params {
        value = value.replace(&format!("{{{name}}}"), replacement);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn reports_follow_locale_and_traditional() {
        assert!(reports("zh-CN", "missingPlatformData").contains("可用数据"));
        assert!(reports("zh-TW", "missingPlatformData").contains("可用資料"));
        assert!(reports("en-US", "generateNone").contains("Could not generate"));
        assert!(reports("ja-JP", "x.recentPost").contains("近投稿"));
        assert_eq!(reports("zh-CN", "bili.catMovie"), "电影");
        assert_eq!(reports("zh-TW", "bili.catMovie"), "電影");
    }

    #[test]
    fn seo_templates_fill_title() {
        let copy = seo_f("en-US", "descPlain", &[("title", "Myriad")]);
        assert!(copy.contains("Myriad"));
        assert!(seo("zh-TW", "keywords").contains("數位生活"));
    }

    #[test]
    fn catalogs_are_objects() {
        let raw: Value = serde_json::from_str(include_str!("../i18n/reports.en-US.json")).unwrap();
        assert!(raw.as_object().unwrap().contains_key("missingPlatformData"));
    }

    fn object_keys(raw: &str) -> Vec<String> {
        let value: Value = serde_json::from_str(raw).unwrap();
        let mut keys: Vec<String> = value
            .as_object()
            .expect("catalog must be an object")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn catalogs_share_keys() {
        for (name, en, zh, tw, ja, ko, fr, de) in [
            (
                "reports",
                include_str!("../i18n/reports.en-US.json"),
                include_str!("../i18n/reports.zh-CN.json"),
                include_str!("../i18n/reports.zh-TW.json"),
                include_str!("../i18n/reports.ja-JP.json"),
                include_str!("../i18n/reports.ko-KR.json"),
                include_str!("../i18n/reports.fr-FR.json"),
                include_str!("../i18n/reports.de-DE.json"),
            ),
            (
                "seo",
                include_str!("../i18n/seo.en-US.json"),
                include_str!("../i18n/seo.zh-CN.json"),
                include_str!("../i18n/seo.zh-TW.json"),
                include_str!("../i18n/seo.ja-JP.json"),
                include_str!("../i18n/seo.ko-KR.json"),
                include_str!("../i18n/seo.fr-FR.json"),
                include_str!("../i18n/seo.de-DE.json"),
            ),
        ] {
            let expected = object_keys(en);
            for (locale, raw) in [
                ("zh-CN", zh),
                ("zh-TW", tw),
                ("ja-JP", ja),
                ("ko-KR", ko),
                ("fr-FR", fr),
                ("de-DE", de),
            ] {
                assert_eq!(
                    object_keys(raw),
                    expected,
                    "{name} {locale} keys must match en-US"
                );
            }
        }
    }
}
