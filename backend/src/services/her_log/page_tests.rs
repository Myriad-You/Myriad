//! A computer far behind takes her log a page at a time on a link where it
//! listed `log.pages`: each answer at most a page, the next only for the
//! `log.vector` that names the page it holds, a duplicate answered with
//! nothing, a new link going on from what it holds; and a link without the
//! capability gets everything at once, byte for byte as before. The other
//! way too: a page the computer sends the server is named back once the
//! server settled its last batch.

use super::*;
use crate::services::agent::link::counts;

/// A link whose hello listed `log2` and `log.pages`.
fn paging() -> relay::Session {
    relay::Session::of(&["log2".to_string(), relay::PAGES.to_string()])
}

const LATER: i64 = T0 + 2 * MINUTE;

/// What the server answers `frame` from `side` with, as `side` gets it.
async fn answers(db: &DatabaseConnection, side: &mut Side, frame: &Value, now: i64) -> Vec<Value> {
    let device = side.id.clone();
    let outs = relay::take(
        db,
        &root(),
        &on(),
        side.user,
        &device,
        Core::Pc,
        &mut side.link,
        frame,
        now,
    )
    .await
    .unwrap();
    outs.iter()
        .filter(|out| match &out.to {
            To::Sender | To::Everyone => true,
            To::Device(id) => *id == device,
            To::Hosts => false,
        })
        .map(delivered)
        .collect()
}

fn of_kind<'a>(frames: &'a [Value], t: &str) -> Vec<&'a Value> {
    frames.iter().filter(|frame| frame["t"] == t).collect()
}

/// The ids of the rows the batches among `frames` carry, in order.
fn row_ids(frames: &[Value]) -> Vec<String> {
    of_kind(frames, "log.rows")
        .into_iter()
        .flat_map(|frame| {
            frame["body"]["rows"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|row| row["id"].as_str().unwrap().to_string())
        .collect()
}

/// The page the last batch among `frames` names (`more`).
fn more_of(frames: &[Value]) -> Option<String> {
    of_kind(frames, "log.rows")
        .last()
        .and_then(|frame| frame["body"]["more"].as_str())
        .map(str::to_string)
}

/// `side`'s log.vector of `audience` naming `page`, without its taking
/// anything first.
fn naming(side: &mut Side, audience: &str, page: &str) -> Value {
    let mut frame = side.vector(audience);
    frame["body"]["after"] = json!(page);
    frame
}

/// `side` takes the batches among `frames`; the log.vector naming the page
/// they end, when more is to go.
fn take_page(side: &mut Side, frames: &[Value]) -> Option<Value> {
    let mut next = None;
    for frame in of_kind(frames, "log.rows") {
        for reply in side.take(frame) {
            if reply["body"].get("after").is_some() {
                next = Some(reply);
            }
        }
    }
    next
}

/// `spans` lines of hers on `nodes` new nodes (`node:<tag><n>`), written by
/// `side` in `audience` from `at` on, and sent to the server once (what it
/// lacks of them).
async fn written(
    db: &DatabaseConnection,
    side: &mut Side,
    (audience, tag): (&str, &str),
    nodes: i64,
    spans: i64,
    at: i64,
) {
    for n in 0..nodes {
        let node = format!("node:{tag}{n}");
        side.write(
            audience,
            &node,
            "node.created",
            Author::Her,
            Some("一件事"),
            at + n,
        );
    }
    for n in 0..spans {
        let node = format!("node:{tag}{}", n % nodes);
        let text = format!("第 {n} 句");
        side.write(
            audience,
            &node,
            "node.span",
            Author::Her,
            Some(&text),
            at + nodes + n,
        );
    }
    push(db, side, audience).await;
}

/// What the server lacks of `side`'s `audience`, sent to it once.
async fn push(db: &DatabaseConnection, side: &mut Side, audience: &str) {
    let theirs = store::holdings(db, OWNER, audience).await.unwrap().vector;
    for frame in side.send(audience, &theirs, Vec::new()) {
        let answered = answers(db, side, &frame, T0 + MINUTE).await;
        assert!(of_kind(&answered, "error").is_empty(), "{answered:?}");
    }
}

/// The server holding `spans` of the partner's rows on ten nodes, from
/// the Mac, which holds her.
async fn many_rows(db: &DatabaseConnection, spans: i64) -> Side {
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    written(db, &mut mac, (PERSONAL, "n"), 10, spans, T0).await;
    mac
}

/// `side` holds what `mac` holds of `audience`.
fn holds_all(side: &Side, mac: &Side, audience: &str) {
    let held = |side: &Side| vector::holdings(&side.log(audience)).unwrap();
    assert_eq!(held(side).vector, held(mac).vector);
    assert_eq!(side.fold(audience, &[]), mac.fold(audience, &[]));
}

/// What a log.vector was answered with, for the log: counts, and once.
#[tokio::test]
async fn what_a_vector_was_answered_with_is_told_once() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_answered").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    many_rows(db, 290).await;
    let held: u64 = store::holdings(db, OWNER, PERSONAL)
        .await
        .unwrap()
        .vector
        .values()
        .sum();
    let mut air = Side::new("dev_air");
    air.link = paging();
    let asking = air.vector(PERSONAL);
    let frames = answers(db, &mut air, &asking, LATER).await;
    let answered = air.link.answered().unwrap();
    assert_eq!(
        (answered.held, answered.batches, answered.rows),
        (held, 1, row_ids(&frames).len())
    );
    assert!(answered.rows > 0 && answered.page.bodies >= answered.rows);
    assert_eq!(air.link.answered(), None);
    // Not a vector the site had a page for: the same one again.
    answers(db, &mut air, &asking, LATER).await;
    assert_eq!(air.link.answered(), None);
    schema.drop().await;
}

#[tokio::test]
async fn a_computer_far_behind_takes_her_log_a_page_at_a_time() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    let mac = many_rows(db, 290).await;
    let mut air = Side::new("dev_air");
    air.link = paging();
    let mut seen = BTreeSet::new();
    let mut pages = 0;
    let mut asking = air.vector(PERSONAL);
    loop {
        let frames = answers(db, &mut air, &asking, LATER).await;
        assert!(
            of_kind(&frames, "log.rows").len() <= 1,
            "a page is one batch here"
        );
        pages += 1;
        for id in row_ids(&frames) {
            assert!(seen.insert(id.clone()), "{id} went twice");
        }
        let Some(after) = take_page(&mut air, &frames) else {
            break;
        };
        if pages == 2 {
            // The same log.vector twice: the second gets nothing again.
            let page = answers(db, &mut air, &after, LATER).await;
            assert_eq!(of_kind(&page, "log.rows").len(), 1);
            assert!(answers(db, &mut air, &after, LATER).await.is_empty());
            for id in row_ids(&page) {
                assert!(seen.insert(id));
            }
            pages += 1;
            asking = take_page(&mut air, &page).expect("more pages are to come");
            continue;
        }
        if pages == 4 {
            // The link drops with the next page asked for: a new link goes
            // on from what the computer holds.
            air.link = paging();
            asking = air.vector(PERSONAL);
            continue;
        }
        asking = after;
        assert!(pages < 40, "paging does not end");
    }
    assert!(pages >= 5, "{pages}");
    holds_all(&air, &mac, PERSONAL);
    schema.drop().await;
}

#[tokio::test]
async fn a_computer_that_did_not_list_pages_gets_everything_at_once_as_before() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_off").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    many_rows(db, 290).await;
    let mut win = Side::new("dev_win");
    let asking = win.vector(PERSONAL);
    let frames = answers(db, &mut win, &asking, LATER).await;
    let batches: Vec<Value> = of_kind(&frames, "log.rows")
        .into_iter()
        .map(|frame| frame["body"].clone())
        .collect();
    assert!(batches.len() > 1);
    assert!(batches.iter().all(|batch| batch.get("more").is_none()));
    // What the server sent before paging, for the same vector.
    let before = store::send(
        db,
        &root(),
        &owner(db).await,
        PERSONAL,
        &Vector::new(),
        Vec::new(),
        &win.pc(),
        2,
    )
    .await
    .unwrap();
    let before: Vec<Value> = before
        .batches
        .into_iter()
        .map(|batch| serde_json::to_value(batch).unwrap())
        .collect();
    assert_eq!(batches, before);
    schema.drop().await;
}

/// The contract's page: sixteen batches, then the rest for its after.
#[tokio::test]
async fn a_page_is_sixteen_batches() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_sixteen").await else {
        return;
    };
    let db = &schema.db;
    let mac = many_rows(db, 1100).await;
    let mut air = Side::new("dev_air");
    air.link = paging();
    let asking = air.vector(PERSONAL);
    let first = answers(db, &mut air, &asking, LATER).await;
    assert_eq!(of_kind(&first, "log.rows").len(), 16);
    assert!(more_of(&first).is_some());
    let after = take_page(&mut air, &first).unwrap();
    let rest = answers(db, &mut air, &after, LATER).await;
    assert!(!of_kind(&rest, "log.rows").is_empty());
    assert!(more_of(&rest).is_none());
    assert!(take_page(&mut air, &rest).is_none());
    holds_all(&air, &mac, PERSONAL);
    schema.drop().await;
}

/// The server as the end that takes a page: once it settled the batch
/// that ends one, taken or refused, its log.vector names it, and only then.
#[tokio::test]
async fn the_server_names_back_a_page_it_settled() {
    let Some(schema) = schema("her_log_pages_back").await else {
        return;
    };
    let db = &schema.db;
    let mut air = Side::new("dev_air");
    air.link = paging();
    let line = |air: &mut Side, object: &str, at: i64| {
        air.write(
            PERSONAL,
            object,
            "persona.field",
            Author::Owner,
            Some("a"),
            at,
        )
    };
    let named = |frames: &[Value]| -> Vec<Option<String>> {
        of_kind(frames, "log.vector")
            .into_iter()
            .map(|frame| frame["body"]["after"].as_str().map(str::to_string))
            .collect()
    };
    // Taken.
    let row = line(&mut air, "persona:name", T0);
    let mut frame = air.rows_frame(PERSONAL, vec![row]);
    frame["body"]["more"] = json!("page_of_air_one");
    let back = answers(db, &mut air, &frame, T0).await;
    assert_eq!(named(&back), [Some("page_of_air_one".to_string())]);
    // A batch that ends no page is answered as before.
    let row = line(&mut air, "persona:city", T0 + 1);
    let frame = air.rows_frame(PERSONAL, vec![row]);
    assert_eq!(named(&answers(db, &mut air, &frame, T0).await), [None]);
    // A link that did not list log.pages has no pages to name.
    air.link = relay::Session::of(&["log2".to_string()]);
    let row = line(&mut air, "persona:town", T0 + 2);
    let mut frame = air.rows_frame(PERSONAL, vec![row]);
    frame["body"]["more"] = json!("page_of_air_three");
    assert_eq!(named(&answers(db, &mut air, &frame, T0).await), [None]);
    // Refused: a clock too far ahead. The error, then the vector naming it.
    let mut ahead = Side::new("dev_ahead");
    ahead.link = paging();
    let row = line(&mut ahead, "persona:mood", T0 + 10 * MINUTE);
    let mut frame = ahead.rows_frame(PERSONAL, vec![row]);
    frame["body"]["more"] = json!("page_of_air_two");
    let back = answers(db, &mut ahead, &frame, T0).await;
    assert_eq!(back[0]["body"]["code"], "clock_ahead");
    assert_eq!(named(&back), [Some("page_of_air_two".to_string())]);
    // Refused by the contract: named all the same, with the server's
    // vector of an audience it holds.
    let mut spoilt = ahead.frame(
        "log.rows",
        Some("batch_spoilt"),
        json!({ "audience": PERSONAL, "rows": "none", "payloads": [], "more": "page_spoilt" }),
    );
    let back = answers(db, &mut ahead, &spoilt, T0).await;
    assert_eq!(back[0]["t"], "error");
    let spoilt_code = back[0]["body"]["code"].as_str().unwrap().to_string();
    assert_eq!(named(&back), [Some("page_spoilt".to_string())]);
    let vector = &of_kind(&back, "log.vector")[0]["body"]["vector"];
    assert!(!vector.as_object().unwrap().is_empty());
    // An audience it may not hold, refused: named with nothing of what the
    // server holds of it, however the batch was refused.
    let group = air.write(
        GROUP,
        "persona:x",
        "persona.field",
        Author::Owner,
        Some("a"),
        T0,
    );
    let mut frame = air.rows_frame(GROUP, vec![group]);
    frame["body"]["more"] = json!("page_of_a_group");
    spoilt["body"]["audience"] = json!(GROUP);
    for (frame, code) in [(frame, "not_holder"), (spoilt, spoilt_code.as_str())] {
        let back = answers(db, &mut ahead, &frame, T0).await;
        assert_eq!(back[0]["body"]["code"], code, "{back:?}");
        let named = of_kind(&back, "log.vector");
        assert_eq!(named.len(), 1);
        assert_eq!(named[0]["body"]["vector"], json!({}));
        assert!(named[0]["body"].get("beyond").is_none());
    }
    // The database failing on one: no name; the link is to close.
    let mut sky = Side::new("dev_sky");
    sky.link = paging();
    assert!(!sky.link.unsettled());
    db.execute_unprepared("ALTER TABLE agent_log_rows RENAME TO agent_log_rows_gone")
        .await
        .unwrap();
    for (n, page) in ["page_of_air_four", "page_of_air_five"]
        .into_iter()
        .enumerate()
    {
        let row = line(&mut sky, &format!("persona:sky{n}"), T0 + 3 + n as i64);
        let mut frame = sky.rows_frame(PERSONAL, vec![row]);
        frame["body"]["more"] = json!(page);
        assert!(off_take(db, &mut sky, &on(), &frame).await.is_err());
        assert!(sky.link.unsettled());
        sky.link = paging();
    }
    let unsettled = counts::pages_unsettled_by("dev_sky").unwrap();
    assert_eq!((unsettled.in_a_row, unsettled.closes.count), (2, 2));
    db.execute_unprepared("ALTER TABLE agent_log_rows_gone RENAME TO agent_log_rows")
        .await
        .unwrap();
    // A page named back again: the closes in a row start over.
    let row = line(&mut sky, "persona:clear", T0 + 6);
    let mut frame = sky.rows_frame(PERSONAL, vec![row]);
    frame["body"]["more"] = json!("page_of_air_six");
    assert_eq!(
        named(&answers(db, &mut sky, &frame, T0).await),
        [Some("page_of_air_six".to_string())]
    );
    let unsettled = counts::pages_unsettled_by("dev_sky").unwrap();
    assert_eq!((unsettled.in_a_row, unsettled.closes.count), (0, 2));
    // What is not a page id, or not an audience, is not named back: the
    // contract's check reads neither shape.
    let row = line(&mut sky, "persona:odd", T0 + 7);
    let mut frame = sky.rows_frame(PERSONAL, vec![row]);
    frame["body"]["more"] = json!("not a page id!");
    let back = answers(db, &mut sky, &frame, T0).await;
    assert!(named(&back).iter().all(|name| name.is_none()), "{back:?}");
    let nowhere = sky.frame(
        "log.rows",
        Some("batch_nowhere"),
        json!({ "audience": "nowhere at all", "rows": "none", "payloads": [], "more": "page_nowhere" }),
    );
    let back = answers(db, &mut sky, &nowhere, T0).await;
    assert_eq!(back[0]["t"], "error");
    assert!(of_kind(&back, "log.vector").is_empty(), "{back:?}");
    schema.drop().await;
}

/// A log.vector the server could not answer lets its audience's page go:
/// the next one without after gets a page again.
#[tokio::test]
async fn a_log_vector_not_answered_lets_its_page_go() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_err").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    many_rows(db, 290).await;
    let mut air = Side::new("dev_air");
    air.link = paging();
    let off = Settings {
        enabled: false,
        ..on()
    };
    let asking = air.vector(PERSONAL);
    let first = answers(db, &mut air, &asking, LATER).await;
    let page = more_of(&first).unwrap();
    // One that does not name the page, not answered: the page stays.
    let stale = air.vector(PERSONAL);
    assert!(off_take(db, &mut air, &off, &stale).await.is_err());
    // One naming it that the contract refuses (more rows asked for than a
    // frame may carry): the page goes.
    let mut spoilt = naming(&mut air, PERSONAL, &page);
    let ask: Vec<String> = (0..=wire::ASK_ROWS).map(|n| format!("row_{n}")).collect();
    spoilt["body"]["ask"] = json!(ask);
    let refused = answers(db, &mut air, &spoilt, LATER).await;
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0]["t"], "error");
    let asking = air.vector(PERSONAL);
    let again = answers(db, &mut air, &asking, LATER).await;
    assert_eq!(row_ids(&again), row_ids(&first));
    // One naming it that does not read (a vector that is none): the page
    // goes.
    let page = more_of(&again).unwrap();
    let mut spoilt = naming(&mut air, PERSONAL, &page);
    spoilt["body"]["vector"] = json!("all of it");
    assert!(off_take(db, &mut air, &on(), &spoilt).await.is_err());
    let asking = air.vector(PERSONAL);
    let again = answers(db, &mut air, &asking, LATER).await;
    assert_eq!(row_ids(&again), row_ids(&first));
    // One naming it, not answered: the page goes.
    let after = take_page(&mut air, &again).unwrap();
    assert!(off_take(db, &mut air, &off, &after).await.is_err());
    let asking = air.vector(PERSONAL);
    let again = answers(db, &mut air, &asking, LATER).await;
    assert_eq!(of_kind(&again, "log.rows").len(), 1);
    // A stale one not answered leaves the page: the side names it next.
    let after = take_page(&mut air, &again).unwrap();
    let stale = air.vector(PERSONAL);
    assert!(off_take(db, &mut air, &off, &stale).await.is_err());
    let next = answers(db, &mut air, &after, LATER).await;
    assert_eq!(of_kind(&next, "log.rows").len(), 1);
    schema.drop().await;
}

/// `frame` from `side` taken with `off` (her log off, or on).
async fn off_take(
    db: &DatabaseConnection,
    side: &mut Side,
    off: &Settings,
    frame: &Value,
) -> Result<Vec<Outgoing>, crate::services::her_log::Error> {
    relay::take(
        db,
        &root(),
        off,
        OWNER,
        &side.id.clone(),
        Core::Pc,
        &mut side.link,
        frame,
        LATER,
    )
    .await
}

/// While a page is unfinished, a log.vector that does not name it gets
/// nothing; neither does one naming a page not on its way (another
/// audience's, the last one twice); a page that brought nothing the side
/// did not hold ends the round. Each is counted for the status page.
#[tokio::test]
async fn a_log_vector_that_names_no_page_on_its_way_gets_none() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_passed").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    let mut mac = many_rows(db, 290).await;
    written(db, &mut mac, (OWN, "o"), 3, 150, T0 + 1000).await;
    let mut air = Side::new("dev_air_passed");
    air.link = paging();
    let asking = air.vector(PERSONAL);
    let first = answers(db, &mut air, &asking, LATER).await;
    let page = more_of(&first).unwrap();
    // Its rows were refused: the side names the page holding no more.
    let stuck = naming(&mut air, PERSONAL, &page);
    assert!(answers(db, &mut air, &stuck, LATER).await.is_empty());
    // The next log.vector without after gets the same page again.
    let asking = air.vector(PERSONAL);
    let again = answers(db, &mut air, &asking, LATER).await;
    assert_eq!(row_ids(&again), row_ids(&first));
    let page = more_of(&again).unwrap();
    // While it is unfinished: one without after, one naming another page.
    assert!(answers(db, &mut air, &asking, LATER).await.is_empty());
    let elsewhere = naming(&mut air, PERSONAL, "page_of_another_link");
    assert!(answers(db, &mut air, &elsewhere, LATER).await.is_empty());
    // Another audience pages beside it, and names only its own page.
    let own = air.vector(OWN);
    let own_first = answers(db, &mut air, &own, LATER).await;
    assert_eq!(of_kind(&own_first, "log.rows").len(), 1);
    let open: Vec<String> = counts::pages_open_by("dev_air_passed")
        .into_iter()
        .map(|(kind, _)| kind)
        .collect();
    assert_eq!(open, ["own", "personal"]);
    let crossed = naming(&mut air, OWN, &page);
    assert!(answers(db, &mut air, &crossed, LATER).await.is_empty());
    let mut next = [take_page(&mut air, &again), take_page(&mut air, &own_first)];
    let mut last = None;
    for _ in 0..40 {
        if next.iter().all(Option::is_none) {
            break;
        }
        for asking in &mut next {
            let Some(frame) = asking.take() else {
                continue;
            };
            let frames = answers(db, &mut air, &frame, LATER).await;
            *asking = take_page(&mut air, &frames);
            if asking.is_none() {
                last = Some(frame);
            }
        }
    }
    assert!(next.iter().all(Option::is_none), "paging does not end");
    // The log.vector that got the last page, twice: nothing again.
    assert!(
        answers(db, &mut air, &last.unwrap(), LATER)
            .await
            .is_empty()
    );
    holds_all(&air, &mac, PERSONAL);
    holds_all(&air, &mac, OWN);
    let passed: BTreeMap<&str, u64> = counts::pages_passed_by("dev_air_passed")
        .into_iter()
        .map(|(why, passed)| (why, passed.count))
        .collect();
    assert_eq!(passed.get("no_headway"), Some(&1));
    assert_eq!(passed.get("no_such_page"), Some(&1));
    // That one is on its way is the rule while paging: not counted; how
    // long it has been is.
    assert_eq!(passed.get("page_unfinished"), None);
    assert!(counts::pages_open_by("dev_air_passed").is_empty());
    schema.drop().await;
}

/// Rows asked again for wait for a page with room, the ones asked first
/// first, and none is lost: not those a log.vector that got no page asked
/// for, nor those past the most one page carries.
#[tokio::test]
async fn rows_asked_again_for_wait_their_turn_and_all_come() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_ask").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    let mut mac = many_rows(db, 290).await;
    // The Air took everything once, on a link without pages, but the
    // bodies of 74 lines did not come.
    let mut air = Side::new("dev_air");
    let spans: Vec<Row> = mac
        .log(PERSONAL)
        .into_iter()
        .filter(|row| row.kind == "node.span")
        .take(74)
        .collect();
    let lost: BTreeSet<String> = spans
        .iter()
        .filter_map(|row| Some(row.payload.as_ref()?.reference.clone()))
        .collect();
    let asking = air.vector(PERSONAL);
    for mut frame in of_kind(&answers(db, &mut air, &asking, LATER).await, "log.rows")
        .into_iter()
        .cloned()
    {
        if let Some(payloads) = frame["body"]["payloads"].as_array_mut() {
            payloads.retain(|one| !lost.contains(one["ref"].as_str().unwrap()));
        }
        air.take(&frame);
    }
    // The first asked (in the log, the last of them), then a few by a
    // log.vector that gets no page, then a few more.
    let ids: Vec<String> = spans.iter().map(|row| row.id.clone()).collect();
    let (early, late) = ids.split_at(10);
    let (stale, then) = early.split_at(5);
    // The Mac wrote more since: pages fill with that first.
    written(db, &mut mac, (PERSONAL, "m"), 10, 200, T0 + 1000).await;
    air.link = paging();
    let mut asking = air.vector(PERSONAL);
    asking["body"]["ask"] = json!(late);
    let mut came: BTreeMap<String, usize> = BTreeMap::new();
    for page in 1..40 {
        let frames = answers(db, &mut air, &asking, LATER).await;
        for id in row_ids(&frames) {
            if ids.contains(&id) {
                assert!(came.insert(id.clone(), page).is_none(), "{id} came twice");
            }
        }
        let next = take_page(&mut air, &frames);
        if page == 1 {
            let mut plain = air.vector(PERSONAL);
            plain["body"]["ask"] = json!(stale);
            assert!(answers(db, &mut air, &plain, LATER).await.is_empty());
        }
        let Some(mut next) = next else {
            break;
        };
        if page == 2 {
            next["body"]["ask"] = json!(then);
        }
        asking = next;
    }
    assert_eq!(came.len(), ids.len(), "every row asked again for came");
    let latest = late.iter().map(|id| came[id]).max().unwrap();
    let earliest = early.iter().map(|id| came[id]).min().unwrap();
    assert!(latest <= earliest, "{latest} > {earliest}");
    holds_all(&air, &mac, PERSONAL);
    schema.drop().await;
}

/// A forget written while a side pages goes on the next page, first, and
/// the side holds it past a gap: no page after sends it again.
#[tokio::test]
async fn a_forget_written_while_paging_comes_next_and_once() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_forget").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    let mut mac = many_rows(db, 290).await;
    let mut air = Side::new("dev_air");
    air.link = paging();
    let asking = air.vector(PERSONAL);
    let first = answers(db, &mut air, &asking, LATER).await;
    let mut seen: BTreeSet<String> = row_ids(&first).into_iter().collect();
    let mut asking = take_page(&mut air, &first).unwrap();
    mac.write(PERSONAL, "node:n3", "forget", Author::Owner, None, T0 + 500);
    push(db, &mut mac, PERSONAL).await;
    let mut pages = 1;
    loop {
        let frames = answers(db, &mut air, &asking, LATER).await;
        pages += 1;
        let rows = of_kind(&frames, "log.rows");
        if pages == 2 {
            assert_eq!(rows[0]["body"]["rows"][0]["kind"], "forget");
        }
        for id in row_ids(&frames) {
            assert!(seen.insert(id.clone()), "{id} went twice");
        }
        let next = take_page(&mut air, &frames);
        if pages == 2 {
            let held = vector::holdings(&air.log(PERSONAL)).unwrap();
            assert!(!held.beyond.is_empty(), "the forget is held past a gap");
        }
        let Some(next) = next else {
            break;
        };
        asking = next;
        assert!(pages < 40, "paging does not end");
    }
    holds_all(&air, &mac, PERSONAL);
    schema.drop().await;
}

/// Rows asked again for wait on a link up to as many as a page of asks
/// each could carry; past that they are dropped and counted, and paging
/// goes on.
#[tokio::test]
async fn rows_asked_again_for_past_what_a_link_keeps_are_dropped() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_asks").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    many_rows(db, 290).await;
    let mut air = Side::new("dev_air_asks");
    air.link = paging();
    let asking = air.vector(PERSONAL);
    let first = answers(db, &mut air, &asking, LATER).await;
    let after = take_page(&mut air, &first).unwrap();
    let kept = wire::ASK_ROWS * wire::PAGE_BATCHES;
    for round in 0..=kept / wire::ASK_ROWS {
        let mut plain = air.vector(PERSONAL);
        let ask: Vec<String> = (0..wire::ASK_ROWS)
            .map(|n| format!("row_asked_{round}_{n}"))
            .collect();
        plain["body"]["ask"] = json!(ask);
        assert!(answers(db, &mut air, &plain, LATER).await.is_empty());
    }
    let dropped = counts::pages_passed_by("dev_air_asks")
        .into_iter()
        .find(|(why, _)| *why == "ask_dropped")
        .map(|(_, passed)| passed.count);
    assert_eq!(dropped, Some(1));
    let next = answers(db, &mut air, &after, LATER).await;
    assert_eq!(of_kind(&next, "log.rows").len(), 1);
    schema.drop().await;
}

/// What the status page keeps of a page on its way is the link's that sent
/// it: let go when the page is, and when that link closes, but not when
/// another link of the device closes.
#[tokio::test]
async fn a_page_on_its_way_is_its_links() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_link").await else {
        return;
    };
    let db = &schema.db;
    let _pages = relay::pages_in_test(1);
    many_rows(db, 290).await;
    let device = "dev_air_link";
    let open = || -> Vec<String> {
        counts::pages_open_by(device)
            .into_iter()
            .map(|(kind, _)| kind)
            .collect()
    };
    let mut air = Side::new(device);
    air.link = paging().on_link(7);
    let asking = air.vector(PERSONAL);
    let first = answers(db, &mut air, &asking, LATER).await;
    assert_eq!(open(), ["personal"]);
    // Named but not answered: let go.
    let off = Settings {
        enabled: false,
        ..on()
    };
    let after = naming(&mut air, PERSONAL, &more_of(&first).unwrap());
    assert!(off_take(db, &mut air, &off, &after).await.is_err());
    assert!(open().is_empty());
    let asking = air.vector(PERSONAL);
    answers(db, &mut air, &asking, LATER).await;
    assert_eq!(open(), ["personal"]);
    // Another link of the device closing leaves it; its own closing does not.
    counts::pages_closed(device, 8);
    assert_eq!(open(), ["personal"]);
    counts::pages_closed(device, 7);
    assert!(open().is_empty());
    // A link that took over sends its own page; the old one closing after
    // leaves that.
    air.link = paging().on_link(9);
    let asking = air.vector(PERSONAL);
    answers(db, &mut air, &asking, LATER).await;
    counts::pages_closed(device, 7);
    assert_eq!(open(), ["personal"]);
    counts::pages_closed(device, 9);
    assert!(open().is_empty());
    schema.drop().await;
}

/// On a link that does not page, a side holding no more after the rows it
/// lacked went to it refused them: the same log.vector gets nothing again,
/// until the server holds more or the side does.
#[tokio::test]
async fn a_link_without_pages_does_not_send_what_was_refused_again() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_no_headway").await else {
        return;
    };
    let db = &schema.db;
    let mut mac = many_rows(db, 20).await;
    let mut win = Side::new("dev_win_refuses");
    let asking = win.vector(PERSONAL);
    let first = answers(db, &mut win, &asking, LATER).await;
    assert!(!row_ids(&first).is_empty());
    // It took none of them and says so: nothing again.
    assert!(answers(db, &mut win, &asking, LATER).await.is_empty());
    assert!(answers(db, &mut win, &asking, LATER).await.is_empty());
    // The server holds more: all of it goes.
    written(db, &mut mac, (PERSONAL, "m"), 1, 1, T0 + 5000).await;
    let again = answers(db, &mut win, &asking, LATER).await;
    assert!(row_ids(&again).len() > row_ids(&first).len());
    // It takes them: holding more, it lacks nothing.
    for frame in of_kind(&again, "log.rows") {
        let _ = win.take(frame);
    }
    holds_all(&win, &mac, PERSONAL);
    let caught_up = win.vector(PERSONAL);
    assert!(row_ids(&answers(db, &mut win, &caught_up, LATER).await).is_empty());
    let passed: BTreeMap<&str, u64> = counts::pages_passed_by("dev_win_refuses")
        .into_iter()
        .map(|(why, passed)| (why, passed.count))
        .collect();
    assert_eq!(passed.get("no_headway"), Some(&2));
    schema.drop().await;
}
