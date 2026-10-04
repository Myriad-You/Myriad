use super::*;

// Keep the previous behavior as an independent reference for aliases and paging.
fn previous_platform(platform: &str) -> String {
    let key = platform
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match key.as_str() {
        "steam" => "Steam".into(),
        "bilibili" | "bili" => "Bilibili".into(),
        "bangumi" | "bgm" => "Bangumi".into(),
        "x" | "twitter" | "xtwitter" => "X".into(),
        "netease" | "neteasemusic" | "neteasecloudmusic" => "Netease".into(),
        "mal" | "myanimelist" => "MyAnimeList".into(),
        "xbox" => "Xbox".into(),
        "psn" | "playstation" => "PlayStation".into(),
        _ => platform.trim().into(),
    }
}

fn previous_source_enabled(
    preferences: &LibrarySourcePreferences,
    item_type: &str,
    platform: &str,
) -> bool {
    let sources = preferences
        .categories
        .get(item_type)
        .cloned()
        .unwrap_or_else(|| {
            default_library_source_categories()
                .get(item_type)
                .cloned()
                .unwrap_or_default()
        });
    sources.contains(&previous_platform(platform))
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn previous_pagination(
    items: &[LibraryItem],
    preferences: Option<&LibrarySourcePreferences>,
    item_type: Option<&str>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<LibraryPage, &'static str> {
    if item_type.is_some_and(|item_type| !LIBRARY_ITEM_TYPES.contains(&item_type)) {
        return Err("Invalid library item type");
    }
    let filtered = items
        .iter()
        .filter(|item| {
            preferences
                .map(|preferences| {
                    previous_source_enabled(preferences, &item.item_type, &item.platform)
                })
                .unwrap_or(true)
        })
        .filter(|item| item_type.map(|kind| item.item_type == kind).unwrap_or(true))
        .collect::<Vec<_>>();
    let total = filtered.len();
    let offset = offset.unwrap_or(0).min(total);
    let limit = limit.unwrap_or(LIBRARY_DEFAULT_PAGE_LIMIT).clamp(1, 200);
    let items: Vec<LibraryItem> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect();
    let returned = items.len();
    let next = offset + returned;
    let has_more = next < total;
    Ok(LibraryPage {
        items,
        total,
        returned,
        offset,
        limit: Some(limit),
        has_more,
        next_offset: has_more.then_some(next),
    })
}

fn page_contract(page: Result<LibraryPage, &'static str>) -> Result<Value, &'static str> {
    page.map(|page| {
        json!({
            "items": page.items,
            "total": page.total,
            "returned": page.returned,
            "offset": page.offset,
            "limit": page.limit,
            "has_more": page.has_more,
            "next_offset": page.next_offset,
        })
    })
}

fn fixture(count: usize) -> Vec<LibraryItem> {
    (0..count)
        .map(|index| LibraryItem {
            id: index.to_string(),
            item_type: LIBRARY_ITEM_TYPES[index % LIBRARY_ITEM_TYPES.len()].into(),
            title: format!("Item {index}"),
            cover: None,
            platform: LIBRARY_PLATFORMS[index % LIBRARY_PLATFORMS.len()].into(),
            metadata: json!({"id": index, "title": "kept"}),
        })
        .collect()
}

#[test]
fn borrowed_source_filter_preserves_aliases_defaults_and_explicit_empty_sources() {
    let platforms = [
        "Steam",
        "steam",
        " ST_EAM ",
        "Bilibili",
        "bili",
        "b i-l_i",
        "Bangumi",
        "bgm",
        "Netease",
        "netease cloud music",
        "MyAnimeList",
        "MAL",
        "X",
        "X-Twitter",
        "Xbox",
        "PlayStation",
        "PSN",
        " Unknown ",
        "",
        " İ ",
    ];
    let preferences = [
        LibrarySourcePreferences::default(),
        LibrarySourcePreferences {
            categories: HashMap::new(),
            ..LibrarySourcePreferences::default()
        },
        LibrarySourcePreferences {
            categories: HashMap::from([
                ("game".into(), vec![]),
                ("music".into(), vec!["Netease".into(), "Unknown".into()]),
                ("custom".into(), vec!["PSN".into(), "PlayStation".into()]),
            ]),
            ..LibrarySourcePreferences::default()
        },
    ];
    for platform in platforms {
        assert_eq!(
            canonical_library_platform(platform),
            previous_platform(platform)
        );
        for prefs in &preferences {
            for kind in LIBRARY_ITEM_TYPES.into_iter().chain(["custom", "missing"]) {
                assert_eq!(
                    prefs.source_enabled(kind, platform),
                    previous_source_enabled(prefs, kind, platform),
                    "kind={kind} platform={platform:?}",
                );
            }
        }
    }
}

#[test]
fn streaming_pagination_matches_previous_contract_at_boundaries() {
    let items = fixture(253);
    let defaults = LibrarySourcePreferences::default();
    let partial = LibrarySourcePreferences {
        categories: HashMap::from([("game".into(), vec![])]),
        ..LibrarySourcePreferences::default()
    };
    for length in [0, 1, 31, items.len()] {
        for preferences in [None, Some(&defaults), Some(&partial)] {
            for kind in [
                None,
                Some("game"),
                Some("music"),
                Some("book"),
                Some("invalid"),
            ] {
                for offset in [
                    None,
                    Some(0),
                    Some(1),
                    Some(50),
                    Some(253),
                    Some(usize::MAX),
                ] {
                    for limit in [None, Some(0), Some(1), Some(50), Some(usize::MAX)] {
                        assert_eq!(
                            page_contract(paginate_library_items(
                                &items[..length],
                                preferences,
                                kind,
                                offset,
                                limit,
                            )),
                            page_contract(previous_pagination(
                                &items[..length],
                                preferences,
                                kind,
                                offset,
                                limit,
                            )),
                            "length={length} kind={kind:?} offset={offset:?} limit={limit:?}",
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "manual warm library filtering/paging timing and allocation workload"]
fn profile_warm_library_paging() {
    #[cfg(feature = "hotpath")]
    let _profile = hotpath::HotpathGuardBuilder::new("warm_library_paging").build();
    let items = fixture(20_000);
    let preferences = LibrarySourcePreferences::default();
    assert_eq!(
        page_contract(previous_pagination(
            &items,
            Some(&preferences),
            None,
            Some(75),
            Some(50)
        )),
        page_contract(paginate_library_items(
            &items,
            Some(&preferences),
            None,
            Some(75),
            Some(50)
        )),
    );
    let mut previous = Vec::new();
    let mut current = Vec::new();
    // Alternate variants; preparation and response serialization are excluded.
    for iteration in 0..30 {
        for old in if iteration % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let paginate = if old {
                previous_pagination
            } else {
                paginate_library_items
            };
            let started = Instant::now();
            let result = paginate(&items, Some(&preferences), None, Some(75), Some(50)).unwrap();
            let elapsed = started.elapsed().as_nanos();
            if old {
                previous.push(elapsed);
            } else {
                current.push(elapsed);
            }
            std::hint::black_box(result);
        }
    }
    for (variant, mut samples) in [("previous", previous), ("current", current)] {
        samples.sort_unstable();
        println!(
            "warm_library_paging: variant={variant} median_ns={} calls=30 items=20000 limit=50 offset=75",
            samples[15],
        );
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn previous_source_response(items: &[LibraryItem]) -> Value {
    json!(collect_library_source_options(items))
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn cached_source_response(snapshot: &LibrarySnapshot) -> Value {
    json!(snapshot.available_sources())
}

#[test]
fn cached_sources_preserve_all_types_counts_and_platform_order() {
    let items = fixture(253);
    let expected = previous_source_response(&items);
    let snapshot = LibrarySnapshot::new(items);
    assert_eq!(cached_source_response(&snapshot), expected);
    assert_eq!(snapshot.available_sources().len(), LIBRARY_ITEM_TYPES.len());
}

#[test]
#[ignore = "manual cached source statistics timing and allocation workload"]
fn profile_warm_library_sources() {
    #[cfg(feature = "hotpath")]
    let _profile = hotpath::HotpathGuardBuilder::new("warm_library_sources").build();
    let snapshot = LibrarySnapshot::new(fixture(20_000));
    assert_eq!(
        previous_source_response(snapshot.items()),
        cached_source_response(&snapshot),
    );
    let (mut previous, mut current) = (Vec::new(), Vec::new());
    for iteration in 0..30 {
        for old in if iteration % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let started = Instant::now();
            let result = if old {
                previous_source_response(snapshot.items())
            } else {
                cached_source_response(&snapshot)
            };
            let elapsed = started.elapsed().as_nanos();
            if old {
                previous.push(elapsed);
            } else {
                current.push(elapsed);
            }
            std::hint::black_box(result);
        }
    }
    for (variant, mut samples) in [("previous", previous), ("current", current)] {
        samples.sort_unstable();
        println!(
            "warm_library_sources: variant={variant} median_ns={} calls=30 items=20000 includes_json_value=true",
            samples[15],
        );
    }
}
