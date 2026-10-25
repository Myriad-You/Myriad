//! Each rule's own vectors: the ones published with the design, and the
//! native client's tests for the Swift rules these supersede.

mod synthetic;

use myriad_perception_filter::{
    Ask, Classify, Decision, Disposition, IdentifierKind, Leaves, PageKind, Policy, Private,
    PrivateKind, Reason, Relay, RelayCheck, SecretKind, Sources, Window, Withheld, check_relay,
    decide, decide_with, identifiers, page, secrets, text, text_hits, window,
};

fn seen(title: &str) -> Window {
    window(Some("com.example.editor"), title, Sources::built_in())
}

#[test]
fn a_key_file_window_is_not_read_at_all() {
    for title in [
        ".env — MyriadNative",
        "vim .env.local",
        "prod.env — repo",
        "server.pem - Preview",
        "credentials — ~/.aws",
        "id_ed25519 (~/.ssh) — Visual Studio Code",
        "deploy/secrets.yaml",
        "~/.netrc",
        "app_credentials.json — Xcode",
    ] {
        assert_eq!(seen(title), Window::Withheld(Withheld::KeyFile), "{title}");
    }
    for title in [
        "README.md — MyriadNative",
        "environment setup notes",
        "Inbox (3) — Mail",
        "PLACE.md — docs",
        "Meeting notes — Notes",
        "keynote talk draft",
        "foo.environment notes",
        "keyboard-shortcuts.md",
    ] {
        assert!(matches!(seen(title), Window::Seen(_)), "{title}");
    }
}

#[test]
fn password_manager_windows_are_withheld_whatever_the_title() {
    for bundle in ["com.apple.keychainaccess", "com.apple.Passwords"] {
        assert_eq!(
            window(Some(bundle), "login", Sources::built_in()),
            Window::Withheld(Withheld::PasswordApp)
        );
    }
}

#[test]
fn titles_are_scrubbed_flattened_and_capped() {
    let (_, key) = &synthetic::named()[0];
    let Window::Seen(title) = window(
        Some("com.apple.Terminal"),
        &format!("zsh — export OPENAI={key}\n第二行"),
        Sources::built_in(),
    ) else {
        panic!("withheld");
    };
    assert!(!title.contains(key.as_str()) && !title.contains('\n'));
    assert_eq!(
        title,
        format!("zsh — export OPENAI={} 第二行", secrets::MARK)
    );
    let Window::Seen(long) = window(None, &"长".repeat(400), Sources::built_in()) else {
        panic!("withheld");
    };
    assert_eq!(long.chars().count(), window::TITLE_LIMIT + 1);
    assert!(long.ends_with('…'));
}

#[test]
fn every_known_prefix_and_high_entropy_string_is_wiped() {
    for (name, key) in synthetic::named() {
        if name == "pem" {
            continue;
        }
        let text = format!("cat .env 输出了 {key} 然后继续");
        let scan = secrets::scan(&text);
        assert!(!scan.text.contains(key.as_str()), "{name}");
        assert!(
            scan.text.contains(secrets::MARK) && !scan.wiped.is_empty(),
            "{name}"
        );
        assert!(
            scan.text.starts_with("cat .env 输出了 ") && scan.text.ends_with(" 然后继续"),
            "{name}"
        );
        let kind = if matches!(name, "hex" | "opaque") {
            SecretKind::HighEntropy
        } else {
            SecretKind::VendorPrefix
        };
        assert_eq!(scan.wiped, [kind], "{name}");
    }
    let pem = synthetic::named().pop().unwrap().1;
    let scan = secrets::scan(&format!("before\n{pem}\nafter"));
    assert!(!scan.text.contains("PRIVATE KEY"));
    assert!(scan.text.starts_with("before\n") && scan.text.ends_with("\nafter"));
    assert_eq!(scan.wiped, [SecretKind::PrivateKey]);
}

#[test]
fn the_label_stays_and_only_the_value_goes() {
    let value = synthetic::random(18, "abcdefghijklmnop", 21);
    let mark = secrets::MARK;
    for (raw, expected, kind) in [
        (
            format!("OPENAI_API_KEY={value}"),
            format!("OPENAI_API_KEY={mark}"),
            SecretKind::Assignment,
        ),
        (
            format!("\"password\": \"{value}\""),
            format!("\"password\": \"{mark}\""),
            SecretKind::Assignment,
        ),
        (
            format!("Authorization: Bearer {value}"),
            format!("Authorization: Bearer {mark}"),
            SecretKind::Bearer,
        ),
        (
            format!("export DB_SECRET='{value}'"),
            format!("export DB_SECRET='{mark}'"),
            SecretKind::Assignment,
        ),
    ] {
        let scan = secrets::scan(&raw);
        assert_eq!(scan.text, expected);
        assert_eq!(scan.wiped, [kind], "{raw}");
    }
}

/// `myriad-error`'s redaction blanks these names after `=`; the assignment
/// rule already takes each of them.
#[test]
fn the_backend_redaction_key_names_are_assignments_too() {
    let value = synthetic::random(18, "abcdefghijklmnop", 22);
    for name in [
        "password",
        "token",
        "access_token",
        "refresh_token",
        "client_secret",
        "api_key",
        "apikey",
        "setup_secret",
        "notion_token",
        "openai_api_key",
        "anthropic_api_key",
        "gemini_api_key",
        "app_secret",
        "qq_bot_app_secret",
        "telegram_bot_token",
        "discord_bot_token",
        "feishu_bot_app_secret",
        "clientSecret",
    ] {
        let scan = secrets::scan(&format!("{name}={value}"));
        assert_eq!(scan.text, format!("{name}={}", secrets::MARK), "{name}");
    }
    let scan = secrets::scan(&format!("authorization={value}"));
    assert_eq!(scan.text, format!("authorization={}", secrets::MARK));
}

#[test]
fn ordinary_code_text_is_left_alone() {
    for text in [
        "Edit Sources/MyriadCore/AgentWatch/SecretScrub.swift at line 42",
        "session 6F9619FF-8B86-D011-B42D-00C04FC964FF finished at 2026-10-19T02:09:16.694Z",
        "commit e473772 on branch claude/agent-watch-603d78",
        "consciousnessProductionAttentionSuspendsReadyLineAndResumesWithoutSecondModel",
        "max_output_tokens=4096 and max_tokens: 8192",
        "test_case_2026_10_19_agent_watch passes",
        "她在看你开着的几个 agent，没有截屏。",
        "",
    ] {
        let scan = secrets::scan(text);
        assert_eq!(scan.text, text);
        assert!(scan.wiped.is_empty(), "误抹：{text}");
    }
}

#[test]
fn scanning_twice_wipes_nothing_more() {
    let all: Vec<String> = synthetic::named().into_iter().map(|(_, key)| key).collect();
    let once = secrets::scan(&all.join(" ")).text;
    let twice = secrets::scan(&once);
    assert_eq!(twice.text, once);
    assert!(twice.wiped.is_empty());
}

fn wiped_kinds(text: &str) -> Vec<IdentifierKind> {
    identifiers::scan(text).wiped
}

#[test]
fn identifiers_with_a_right_check_digit_are_wiped() {
    use IdentifierKind::*;
    assert_eq!(wiped_kinds("卡号 4111 1111 1111 1111 有效期 12/28"), [Card]);
    assert_eq!(wiped_kinds("card 4111-1111-1111-1111."), [Card]);
    assert_eq!(wiped_kinds("4111111111111111"), [Card]);
    for nric in ["S1234567D", "T1234567J", "F1234567N", "G1234567X"] {
        assert_eq!(wiped_kinds(&format!("NRIC: {nric}")), [SgNric], "{nric}");
    }
    assert_eq!(wiped_kinds("マイナンバー 1234 5678 9018"), [JpMyNumber]);
    assert_eq!(wiped_kinds("1234-5678-9018"), [JpMyNumber]);
    assert_eq!(wiped_kinds("個人番号：123456789018"), [JpMyNumber]);
    let scan = identifiers::scan("卡 4111 1111 1111 1111，NRIC S1234567D。");
    assert_eq!(
        scan.text,
        format!("卡 {m}，NRIC {m}。", m = identifiers::MARK)
    );
    for (_, value) in synthetic::identifiers() {
        assert_eq!(identifiers::scan(&value).wiped.len(), 1, "{value}");
    }
}

#[test]
fn a_well_formed_number_with_a_wrong_check_digit_stays() {
    for text in [
        "S1234567A",
        "4000 0012 3456 7890",
        "1234 5678 9012",
        // Twelve digits with the right check digit but no label and no grouping.
        "订单号 123456789018",
        "电话 138 0013 8000",
        "2026-10-19 12:00",
    ] {
        assert!(wiped_kinds(text).is_empty(), "{text}");
    }
}

#[test]
fn text_rules_hit_money_and_health_and_name_the_rule() {
    let rules = |text: &str| {
        text_hits(text)
            .iter()
            .map(|hit| hit.rule)
            .collect::<Vec<_>>()
    };
    assert_eq!(rules("可用余额：¥12,345.67"), [text::FINANCE_AMOUNT]);
    assert_eq!(rules("残高 1,234,567円"), [text::FINANCE_AMOUNT]);
    assert_eq!(
        rules("Statement balance S$1,204.50 due 3 Nov"),
        [text::FINANCE_AMOUNT]
    );
    assert_eq!(rules("HbA1c 6.4%（参考 4.6–6.2）"), [text::HEALTH_LAB]);
    assert_eq!(rules("LDL コレステロール 162 mg/dL"), [text::HEALTH_LAB]);
    assert_eq!(
        rules("処方：ロキソプロフェン 60mg 1日3回"),
        [text::HEALTH_PRESCRIPTION]
    );
    assert_eq!(rules("Diagnosis: mild asthma"), [text::HEALTH_FORM]);
    // v2: a holdings table with no currency, and an offer.
    assert_eq!(
        rules("份额 1,203.55 净值 1.2345 收益率 +3.21%"),
        [text::FINANCE_HOLDINGS]
    );
    assert_eq!(
        rules("Your base pay will be $8,500 per month"),
        [text::FINANCE_PAY]
    );
    assert_eq!(rules("内定のご連絡：年収 650万円"), [text::FINANCE_PAY]);
    // Deliberately narrow.
    for text in [
        "assert_eq!(account.balance(), 100);",
        "两万块的余额",
        "残高は30万くらい",
        "收益率 3.2%",
        "年収 650万円の求人",
    ] {
        assert!(text_hits(text).is_empty(), "{text}");
    }
}

#[test]
fn a_relay_that_carries_numbers_copies_wording_or_says_nothing_is_dropped() {
    let message = "下周三上午 10:30 去仁和医院复查，挂号单号 20261019 我已经发你了";
    assert_eq!(check_relay(message, "妈妈", "复查安排"), RelayCheck::Ok);
    assert_eq!(
        check_relay(message, "妈妈", "单号20261019"),
        RelayCheck::Digits
    );
    assert_eq!(
        check_relay(message, "妈妈", "去仁和医院复查"),
        RelayCheck::Copied
    );
    assert_eq!(check_relay(message, " ", "复查安排"), RelayCheck::Empty);
    assert_eq!(check_relay(message, "妈妈", ""), RelayCheck::Empty);
    // Full-width digits are digits.
    assert_eq!(
        check_relay("暗号は１２３４です", "Aki", "暗号1234"),
        RelayCheck::Digits
    );
    assert_eq!(
        check_relay(
            "Meet me at the old place on Friday night",
            "Sam",
            "meet at the old place"
        ),
        RelayCheck::Ok
    );
    assert_eq!(
        check_relay(
            "Meet me at the old place on Friday night",
            "Sam",
            "the old place on friday"
        ),
        RelayCheck::Copied
    );
}

fn open() -> Decision {
    Decision {
        capture: Some("seen".into()),
        window: Some("seen".into()),
        password_field: None,
        identifiers: Some(vec![]),
        source: Some(None),
        page: None,
        private: Some(Private::Known(false)),
        text_hits: Some(vec![]),
        model: Some("available".into()),
        language: Some("supported".into()),
        relay: Some(None),
        classify: Some(Some(Classify {
            general: Some("not_sensitive".into()),
            tagging: Some("not_sensitive".into()),
            kind: None,
        })),
    }
}

fn disposition(decision: &Decision) -> (Disposition, Reason) {
    let decided = decide(decision);
    (decided.disposition, decided.reason)
}

#[test]
fn the_published_boundaries() {
    use Disposition::*;
    assert_eq!(disposition(&open()), (Carry, Reason::Classify));
    // Nothing hit, model unavailable.
    let unavailable = Decision {
        model: Some("unavailable".into()),
        ..open()
    };
    assert_eq!(disposition(&unavailable), (Blind, Reason::ModelUnavailable));
    // A finance source needs no model.
    let finance = Decision {
        source: Some(Some("finance".into())),
        ..unavailable.clone()
    };
    assert_eq!(disposition(&finance), (PresentOnly, Reason::Source));
    // Messaging with no relay.
    let messaging = Decision {
        source: Some(Some("messaging".into())),
        ..open()
    };
    assert_eq!(disposition(&messaging), (Blind, Reason::Relay));
    // One clear answer and one error.
    let mut half = open();
    half.classify = Some(Some(Classify {
        general: Some("not_sensitive".into()),
        tagging: Some("error".into()),
        kind: None,
    }));
    assert_eq!(disposition(&half), (PresentOnly, Reason::Classify));
    // A private window the client cannot tell: the rules after it decide (a known gap).
    let unknown = Decision {
        private: Some(Private::Other("unknown".into())),
        ..open()
    };
    assert_eq!(disposition(&unknown), (Carry, Reason::Classify));
}

#[test]
fn earlier_steps_win() {
    use Disposition::*;
    let all = Decision {
        capture: Some("excluded".into()),
        window: Some("withheld".into()),
        identifiers: Some(vec!["card".into()]),
        source: Some(Some("health".into())),
        private: Some(Private::Known(true)),
        text_hits: Some(vec!["finance".into()]),
        ..open()
    };
    assert_eq!(disposition(&all), (Exclude, Reason::Capture));
    let all = Decision {
        capture: Some("seen".into()),
        ..all
    };
    assert_eq!(disposition(&all), (Exclude, Reason::Window));
    let all = Decision {
        window: Some("seen".into()),
        ..all
    };
    let decided = decide(&all);
    assert_eq!(
        (decided.disposition, decided.kind),
        (PresentOnly, Some(PrivateKind::Identity))
    );
    let all = Decision {
        identifiers: Some(vec![]),
        ..all
    };
    assert_eq!(decide(&all).kind, Some(PrivateKind::Health));
    let all = Decision {
        source: Some(None),
        ..all
    };
    assert_eq!(decide(&all).kind, Some(PrivateKind::PrivateBrowsing));
    let all = Decision {
        private: Some(Private::Known(false)),
        ..all
    };
    assert_eq!(decide(&all).kind, Some(PrivateKind::Finance));
    assert_eq!(decide(&all).reason, Reason::Text);
    // A host rule is never overruled by the model, even a clear one.
    assert_eq!(decide(&all).disposition, PresentOnly);
}

#[test]
fn other_peoples_words_leave_only_as_who_and_what() {
    let relayed = |result: &str, category: &str| {
        decide(&Decision {
            source: Some(Some("messaging".into())),
            relay: Some(Some(Relay {
                result: result.into(),
                category: Some(category.into()),
            })),
            classify: Some(None),
            ..open()
        })
    };
    let ok = relayed("ok", "plans");
    assert_eq!(
        (ok.disposition, ok.leaves),
        (Disposition::WhoWhat, Leaves::WhoWhat)
    );
    assert_eq!(ok.leaves_kind.as_deref(), Some("plans"));
    assert_eq!(
        relayed("ok", "astrology").leaves_kind.as_deref(),
        Some("other")
    );
    assert_eq!(relayed("rejected", "plans").disposition, Disposition::Blind);
    assert_eq!(relayed("error", "plans").disposition, Disposition::Blind);
    assert_eq!(relayed("ok", "health").kind, Some(PrivateKind::Health));
    assert_eq!(relayed("ok", "money").kind, Some(PrivateKind::Finance));
    assert_eq!(relayed("ok", "money").leaves, Leaves::PrivateMarker);
}

#[test]
fn closed_by_default() {
    use Disposition::*;
    // Every input the steps need, taken away one at a time.
    for strip in 0..10 {
        let mut d = open();
        let field = match strip {
            0 => {
                d.capture = None;
                "capture"
            }
            1 => {
                d.window = None;
                "window"
            }
            2 => {
                d.identifiers = None;
                "identifiers"
            }
            3 => {
                d.source = None;
                "source"
            }
            4 => {
                d.private = None;
                "private"
            }
            5 => {
                d.text_hits = None;
                "textHits"
            }
            6 => {
                d.model = None;
                "model"
            }
            7 => {
                d.language = None;
                "language"
            }
            8 => {
                d.classify = None;
                "classify"
            }
            _ => {
                d.classify = Some(None);
                "classify"
            }
        };
        let decided = decide(&d);
        assert_eq!(
            (decided.disposition, decided.missing),
            (Blind, Some(field)),
            "{field}"
        );
        assert_eq!(decided.leaves, Leaves::Nothing);
    }
    // Values this core does not know read as the closed one.
    assert_eq!(
        disposition(&Decision {
            capture: Some("partial".into()),
            ..open()
        })
        .0,
        Exclude
    );
    assert_eq!(
        disposition(&Decision {
            window: Some("blurred".into()),
            ..open()
        })
        .0,
        Exclude
    );
    assert_eq!(
        disposition(&Decision {
            source: Some(Some("gambling".into())),
            ..open()
        })
        .0,
        PresentOnly
    );
    assert_eq!(
        disposition(&Decision {
            private: Some(Private::Other("maybe".into())),
            ..open()
        })
        .0,
        PresentOnly
    );
    assert_eq!(
        disposition(&Decision {
            text_hits: Some(vec!["romance".into()]),
            ..open()
        })
        .0,
        PresentOnly
    );
    assert_eq!(
        disposition(&Decision {
            model: Some("downloading".into()),
            ..open()
        })
        .0,
        Blind
    );
    assert_eq!(
        disposition(&Decision {
            language: Some("und".into()),
            ..open()
        })
        .0,
        Blind
    );
    let mut unsure = open();
    unsure.classify = Some(Some(Classify {
        general: Some("probably_fine".into()),
        tagging: Some("not_sensitive".into()),
        kind: None,
    }));
    assert_eq!(disposition(&unsure).0, PresentOnly);
    assert_eq!(decide(&unsure).kind, Some(PrivateKind::Unknown));
}

/// The two open owner questions: today the more closed answer of each, and
/// either other answer is one constant.
#[test]
fn the_open_policies() {
    let mut once = open();
    once.classify = Some(Some(Classify {
        general: Some("not_sensitive".into()),
        tagging: None,
        kind: None,
    }));
    assert_eq!(decide(&once).disposition, Disposition::PresentOnly);
    let ask_once = Policy {
        classify_asks: &[Ask::General],
        ..Policy::CURRENT
    };
    assert_eq!(
        decide_with(&once, &ask_once).disposition,
        Disposition::Carry
    );

    // A health text rule: a sensitive source would leave nothing whatever the policy.
    let health = Decision {
        text_hits: Some(vec!["health".into()]),
        ..open()
    };
    assert_eq!(decide(&health).leaves_kind, None);
    let with_kind = Policy {
        present_only_leaves_kind: true,
        ..Policy::CURRENT
    };
    assert_eq!(
        decide_with(&health, &with_kind).leaves_kind.as_deref(),
        Some("health")
    );
}

#[test]
fn a_sensitive_source_or_page_leaves_nothing_whatever_the_policy() {
    let with_kind = Policy {
        present_only_leaves_kind: true,
        ..Policy::CURRENT
    };
    for decision in [
        Decision {
            source: Some(Some("health".into())),
            ..open()
        },
        Decision {
            source: Some(Some("adult".into())),
            ..open()
        },
        Decision {
            source: Some(Some("gambling".into())),
            ..open()
        },
        Decision {
            page: Some("account".into()),
            ..open()
        },
        Decision {
            page: Some("sensitive".into()),
            ..open()
        },
        // A value this core does not know reads as unknown.
        Decision {
            page: Some("forum".into()),
            ..open()
        },
        // Even when an identifier is what put it there.
        Decision {
            identifiers: Some(vec!["card".into()]),
            page: Some("sensitive".into()),
            ..open()
        },
    ] {
        for policy in [Policy::CURRENT, with_kind] {
            let decided = decide_with(&decision, &policy);
            assert_eq!(
                decided.disposition,
                Disposition::PresentOnly,
                "{decision:?}"
            );
            assert_eq!(
                (decided.leaves, decided.leaves_kind),
                (Leaves::Nothing, None),
                "{decision:?}"
            );
        }
    }
    // An ordinary page goes on to the model; a private window still leaves the marker.
    assert_eq!(
        disposition(&Decision {
            page: Some("ordinary".into()),
            ..open()
        })
        .0,
        Disposition::Carry
    );
    // A page no rule places is not held for that alone (a block list): it goes on like an ordinary
    // one, and what its text holds is still held by the rules after the page step.
    assert_eq!(
        disposition(&Decision {
            page: Some("unknown".into()),
            ..open()
        })
        .0,
        Disposition::Carry
    );
    let card = decide(&Decision {
        identifiers: Some(vec!["card".into()]),
        page: Some("unknown".into()),
        ..open()
    });
    assert_eq!(card.disposition, Disposition::PresentOnly);
    assert_eq!(
        card.leaves,
        Leaves::PrivateMarker,
        "only a sensitive page says nothing at all"
    );
    let private = decide(&Decision {
        private: Some(Private::Known(true)),
        ..open()
    });
    assert_eq!(private.leaves, Leaves::PrivateMarker);
}

#[test]
fn a_showing_password_field_is_not_read() {
    let field = decide(&Decision {
        password_field: Some(true),
        ..open()
    });
    assert_eq!(
        (field.disposition, field.reason),
        (Disposition::Exclude, Reason::PasswordField)
    );
    assert_eq!(field.leaves, Leaves::Nothing);
    let sign_in = decide(&Decision {
        page: Some("sign_in".into()),
        ..open()
    });
    assert_eq!(sign_in.disposition, Disposition::Exclude);
    // Not given, or false: not a reason by itself.
    assert_eq!(
        disposition(&Decision {
            password_field: Some(false),
            ..open()
        })
        .0,
        Disposition::Carry
    );
    // Excluded before anything else is needed.
    let bare = Decision {
        password_field: Some(true),
        capture: Some("seen".into()),
        window: Some("seen".into()),
        ..Decision::default()
    };
    assert_eq!(decide(&bare).disposition, Disposition::Exclude);
}

#[test]
fn the_built_in_table_reads_everyday_sites_and_holds_the_rest() {
    let at = |host: &str, path: Option<&str>| {
        let placed = page(host, path, None, false, None, Sources::built_in());
        (placed.kind, placed.read())
    };
    for host in [
        "www.youtube.com",
        "www.bilibili.com",
        "www.nicovideo.jp",
        "www.bbc.com",
        "www.nhk.or.jp",
        "www.straitstimes.com",
        "en.wikipedia.org",
        "ja.wikipedia.org",
        "developer.mozilla.org",
        "docs.rs",
        "github.com",
        "stackoverflow.com",
        "www.amazon.co.jp",
        "item.taobao.com",
        "shopee.sg",
        "store.steampowered.com",
        "tenki.jp",
        "docs.swift.org",
    ] {
        assert_eq!(at(host, Some("/")), (PageKind::Ordinary, true), "{host}");
    }
    for (host, path, kind) in [
        ("github.com", "/settings/security", PageKind::Account),
        (
            "www.amazon.co.jp",
            "/gp/your-account/order-history",
            PageKind::Account,
        ),
        ("www.youtube.com", "/account", PageKind::Account),
        ("accounts.google.com", "/", PageKind::Sensitive),
        ("www.dbs.com.sg", "/", PageKind::Sensitive),
        ("www.singpass.gov.sg", "/", PageKind::Sensitive),
        ("myna.go.jp", "/", PageKind::Sensitive),
        // Not in the table, but the host says what it is for: held.
        ("www.examplebank.bank", "/", PageKind::Sensitive),
        ("online-banking.example.com", "/", PageKind::Sensitive),
        ("my.city-hospital.example", "/", PageKind::Sensitive),
        ("tax.example.gov", "/", PageKind::Sensitive),
        ("www.example.go.jp", "/", PageKind::Sensitive),
        ("dating.example.net", "/", PageKind::Sensitive),
        // The stem inside a longer name counts too.
        ("www.mybank.example", "/", PageKind::Sensitive),
        ("www.bankofamerica.example", "/", PageKind::Sensitive),
        ("www.healthline.example", "/", PageKind::Sensitive),
        ("secure.someinsurer.example", "/", PageKind::Sensitive),
        ("www.webmedical.example", "/", PageKind::Sensitive),
        // An unknown site's account page is still an account page.
        ("some-blog.example", "/settings", PageKind::Account),
    ] {
        assert_eq!(at(host, Some(path)), (kind, false), "{host}{path}");
    }
    // A site no rule places, and nothing in its name says it is sensitive: read (a block list).
    for host in [
        "some-blog.example",
        "news.example.co.jp",
        "matcha-recipes.example",
        "lawn-care.example",
        // Short words only count as a whole label, not inside other words.
        "syntax-highlight.example",
        "flawless.example",
        "alphabet.example",
    ] {
        assert_eq!(at(host, Some("/")), (PageKind::Unknown, true), "{host}");
    }
    assert_eq!(at("mail.google.com", None), (PageKind::Messaging, true));
}

#[test]
fn account_paths_need_an_account_word() {
    for path in [
        "/settings",
        "/user/settings/security",
        "/my-account",
        "/account/billing",
        "/checkout?step=2",
        "/sign-in",
        "/two-factor",
        "/mypage/",
        "/api-keys",
    ] {
        assert!(myriad_perception_filter::page::account_path(path), "{path}");
    }
    for path in [
        "/",
        "/docs/security",
        "/wiki/Privacy",
        "/profile/someone",
        "/watch?v=settings",
        "/blog/2026/10/accounting-basics",
    ] {
        assert!(
            !myriad_perception_filter::page::account_path(path),
            "{path}"
        );
    }
}
