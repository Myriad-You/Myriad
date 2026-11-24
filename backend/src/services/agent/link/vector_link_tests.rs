//! What the site tells an end of its own rows (`log.vector`) while a frame of
//! that end's takes long: the end is told what the site holds now, not what it
//! held at each row, which would have it send the site's own rows back.

use super::*;
use crate::services::her_log::{self, store};
use myriad_persona_log::Vector;

/// The Mac's `log.vector` worked through while the site writes four rows,
/// held where it was said, and what the Mac heard before what came behind
/// them all.
struct Worked {
    site: Site,
    mac: Child,
    personal: String,
    /// What the Mac said it holds.
    theirs: Vector,
    /// What the site holds after the four.
    ours: Vector,
    heard: Vec<Value>,
    _turn: tokio::sync::MutexGuard<'static, ()>,
}

/// One row of the site's own.
async fn wrote(site: &Site, personal: &str, object: &str) -> Vector {
    let owner = her_log::owner(&site.db, OWNER).await.unwrap();
    store::write_owners(
        &site.db,
        her_log::keys::root(),
        &owner,
        personal,
        None,
        object,
        "segment.open",
        &json!({ "at": 1, "place": "private" }),
    )
    .await
    .unwrap();
    (store::holdings(&site.db, OWNER, personal).await.unwrap()).vector
}

/// Four rows the site writes while the Mac's `log.vector` is worked through,
/// held where `at` says. The Mac lacks `behind` rows before them.
async fn worked(at: &str, behind: usize) -> Option<Worked> {
    let site = site().await?;
    let turn = her_log::lend::SERIAL.lock().await;
    her_log::test_settings::set(her_log::Settings {
        enabled: true,
        ..her_log::Settings::default()
    });
    store::start(&site.db, OWNER).await.unwrap();
    let (device, token) = device(&site.db, "MacBook").await;
    let mut mac = Child::open(&site, &token).await;
    let welcome = mac.hello_with(&["partner", "log2"]).await;
    assert_eq!(welcome["body"]["sync"], "on");
    let personal = format!("personal:{}", welcome["body"]["partner"].as_str().unwrap());
    mac.ping_in_turn().await;
    until(&mut mac, "pong").await;
    let theirs = (store::holdings(&site.db, OWNER, &personal).await.unwrap()).vector;
    for n in 0..behind {
        wrote(&site, &personal, &format!("segment:behind_{n}")).await;
    }

    let (begun, go) = super::super::held::next(&device.id, at);
    mac.send(
        "log.vector",
        None,
        json!({ "audience": personal, "vector": theirs }),
    )
    .await;
    tokio::time::timeout(WAIT, begun)
        .await
        .expect("the frame came to where it is held")
        .unwrap();
    let mut ours = theirs.clone();
    for n in 0..4 {
        // Told as the row was kept, whenever the listener hears of it too.
        ours = wrote(&site, &personal, &format!("segment:site_{n}")).await;
        super::super::tell_log_links(
            OWNER,
            "log.vector",
            json!({ "audience": personal, "vector": ours }),
        );
    }
    // Behind them all.
    super::super::tell_log_links(
        OWNER,
        "lease.epoch",
        json!({ "epoch": 9, "holder": "node", "how": "first" }),
    );
    go.send(()).unwrap();

    let mut heard = Vec::new();
    loop {
        let frame = mac.next().await;
        if frame["t"] == "lease.epoch" {
            break;
        }
        if frame["body"]["audience"] == personal.as_str() {
            heard.push(frame);
        }
    }
    Some(Worked {
        site,
        mac,
        personal,
        theirs,
        ours,
        heard,
        _turn: turn,
    })
}

/// What a `log.vector` says is held.
fn said(frame: &Value) -> Vector {
    serde_json::from_value(frame["body"]["vector"].clone()).unwrap()
}

/// The rows in `heard`.
fn rows_in(heard: &[Value]) -> usize {
    (heard.iter().filter(|frame| frame["t"] == "log.rows"))
        .map(|frame| frame["body"]["rows"].as_array().unwrap().len())
        .sum()
}

/// How many of `ours` an end that holds them would send back for each vector
/// in `heard` from the first rows on.
fn back(ours: &Vector, heard: &[Value]) -> Vec<u64> {
    (heard.iter().skip_while(|frame| frame["t"] != "log.rows"))
        .filter(|frame| frame["t"] == "log.vector")
        .map(|frame| {
            let said = said(frame);
            ours.iter()
                .map(|(origin, held)| held.saturating_sub(said.get(origin).copied().unwrap_or(0)))
                .sum()
        })
        .collect()
}

/// Four rows written where `at` says, for a Mac that lacks `behind` before
/// them: it gets them all in its answer. How many it would send back for each
/// vector after.
async fn sent_back(at: &str, behind: usize) -> Option<Vec<u64>> {
    let worked = worked(at, behind).await?;
    assert_eq!(rows_in(&worked.heard), 4 + behind);
    let back = back(&worked.ours, &worked.heard);
    worked.site.close().await;
    Some(back)
}

/// Written before the site counts what it holds: its answer says them all,
/// and none of the vectors waiting says less after it.
#[tokio::test]
async fn rows_written_during_a_long_frame_are_not_asked_back_one_vector_at_a_time() {
    if let Some(back) = sent_back("log.vector", 0).await {
        assert_eq!(back, [0], "its answer, and nothing it covers");
    }
}

/// Written between that count and the page: they are in the page, so its
/// answer says them.
#[tokio::test]
async fn rows_kept_as_the_page_is_read_are_in_the_vector_after_it() {
    if let Some(back) = sent_back("a page of log.vector", 1).await {
        assert_eq!(back, [0], "its answer, and nothing it covers");
    }
}

/// Written after the count of what the site holds, for a Mac that lacks none
/// of that: no page is read for it. Its answer says only what was counted,
/// none of which it lacks; it is told of them after, and its next log.vector
/// gets them all.
#[tokio::test]
async fn rows_kept_after_a_count_it_lacks_none_of_go_with_its_next_vector() {
    let Some(mut worked) = worked("a page of log.vector", 0).await else {
        return;
    };
    assert_eq!(rows_in(&worked.heard), 0);
    assert_eq!(worked.heard[0]["t"], "log.vector");
    assert_eq!(said(&worked.heard[0]), worked.theirs, "its answer");
    // It is told of them behind it: by what was told as each was kept, or by
    // the listener, which may come after what came behind those.
    let mut told = worked.heard.clone();
    while said(told.last().unwrap()) != worked.ours {
        let frame = worked.mac.next().await;
        if frame["t"] == "log.vector" && frame["body"]["audience"] == worked.personal.as_str() {
            told.push(frame);
        }
    }
    worked
        .mac
        .send(
            "log.vector",
            None,
            json!({ "audience": worked.personal, "vector": worked.theirs }),
        )
        .await;
    let mut heard = Vec::new();
    while rows_in(&heard) == 0
        || heard
            .last()
            .is_none_or(|frame: &Value| frame["t"] == "log.rows")
    {
        let frame = worked.mac.next().await;
        if frame["body"]["audience"] == worked.personal.as_str() {
            heard.push(frame);
        }
    }
    assert_eq!(rows_in(&heard), 4);
    assert_eq!(back(&worked.ours, &heard), [0], "its answer says them");
    worked.site.close().await;
}
