//! A computer far behind takes her log a page at a time on a link where it
//! listed `log.pages`: each answer at most a page, the next only for the
//! `log.vector` that names the page it holds, a duplicate answered with
//! nothing, a new link going on from what it holds; and a link without the
//! capability gets everything at once, byte for byte as before.

use super::*;

/// A link whose hello listed `log2` and `log.pages`.
fn paging() -> relay::Session {
    relay::Session::of(&["log2".to_string(), relay::PAGES.to_string()])
}

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

/// The server holding a good many of the partner's rows, from the Mac.
async fn many_rows(db: &DatabaseConnection) -> Side {
    let mut mac = Side::new("dev_mac");
    let ask = mac.frame("lease.ask", None, json!({ "why": "talk" }));
    exchange(db, &mut mac, vec![ask], T0).await;
    for n in 0..10 {
        let node = format!("node:n{n}");
        mac.write(
            PERSONAL,
            &node,
            "node.created",
            Author::Her,
            Some("一件事"),
            T0 + n,
        );
    }
    for n in 0..290 {
        let node = format!("node:n{}", n % 10);
        let text = format!("第 {n} 句");
        mac.write(
            PERSONAL,
            &node,
            "node.span",
            Author::Her,
            Some(&text),
            T0 + 10 + n,
        );
    }
    sync(db, &mut mac, PERSONAL, T0 + MINUTE).await;
    mac
}

#[tokio::test]
async fn a_computer_far_behind_takes_her_log_a_page_at_a_time() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages").await else {
        return;
    };
    let db = &schema.db;
    relay::pages_in_test(1);
    let mac = many_rows(db).await;
    let now = T0 + 2 * MINUTE;
    let mut air = Side::new("dev_air");
    air.link = paging();
    let mut seen = BTreeSet::new();
    let mut pages = 0;
    let mut asking = air.vector(PERSONAL);
    loop {
        let frames = answers(db, &mut air, &asking, now).await;
        let batches = of_kind(&frames, "log.rows");
        assert!(batches.len() <= 1, "a page is one batch here");
        pages += 1;
        let mut next = None;
        for frame in &frames {
            for row in frame["body"]["rows"].as_array().into_iter().flatten() {
                let id = row["id"].as_str().unwrap().to_string();
                assert!(seen.insert(id.clone()), "{id} went twice");
            }
            for reply in air.take(frame) {
                if reply["body"].get("after").is_some() {
                    next = Some(reply);
                }
            }
        }
        let Some(after) = next else {
            break;
        };
        if pages == 2 {
            // The same log.vector twice: the second gets nothing again.
            let page = answers(db, &mut air, &after, now).await;
            assert_eq!(of_kind(&page, "log.rows").len(), 1);
            let again = after.clone();
            assert!(answers(db, &mut air, &again, now).await.is_empty());
            let mut more = None;
            for frame in &page {
                for row in frame["body"]["rows"].as_array().into_iter().flatten() {
                    assert!(seen.insert(row["id"].as_str().unwrap().to_string()));
                }
                for reply in air.take(frame) {
                    if reply["body"].get("after").is_some() {
                        more = Some(reply);
                    }
                }
            }
            pages += 1;
            asking = more.expect("more pages are to come");
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
    let held = |side: &Side| vector::holdings(&side.log(PERSONAL)).unwrap();
    assert_eq!(held(&air).vector, held(&mac).vector);
    assert_eq!(air.fold(PERSONAL, &[]), mac.fold(PERSONAL, &[]));
}

#[tokio::test]
async fn a_computer_that_did_not_list_pages_gets_everything_at_once_as_before() {
    let _minds = lease::minds_go_to_devices_in_test(true);
    let Some(schema) = schema("her_log_pages_off").await else {
        return;
    };
    let db = &schema.db;
    relay::pages_in_test(1);
    many_rows(db).await;
    let mut win = Side::new("dev_win");
    let asking = win.vector(PERSONAL);
    let frames = answers(db, &mut win, &asking, T0 + 2 * MINUTE).await;
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
}
