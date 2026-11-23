//! What the site tells an end of its own rows (`log.vector`) while a frame of
//! that end's takes long: the end is told what the site holds now, not what it
//! held at each row, which would have it send the site's own rows back.

use super::*;
use crate::services::her_log::{self, store};

/// Four rows the site writes while the Mac's `log.vector` is worked through,
/// held where `at` says: the Mac gets them in its answer. How many of them
/// the Mac, holding what it was sent, would send back for each vector after.
async fn sent_back(at: &str) -> Option<Vec<u64>> {
    let site = site().await?;
    let _turn = her_log::lend::SERIAL.lock().await;
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
    let theirs = store::holdings(&site.db, OWNER, &personal).await.unwrap();

    let (begun, go) = super::super::held::next(&device.id, at);
    mac.send(
        "log.vector",
        None,
        json!({ "audience": personal, "vector": theirs.vector }),
    )
    .await;
    tokio::time::timeout(WAIT, begun)
        .await
        .expect("the frame came to where it is held")
        .unwrap();
    let owner = her_log::owner(&site.db, OWNER).await.unwrap();
    let mut ours = theirs;
    for n in 0..4 {
        store::write_owners(
            &site.db,
            her_log::keys::root(),
            &owner,
            &personal,
            None,
            &format!("segment:site_{n}"),
            "segment.open",
            &json!({ "at": 1, "place": "private" }),
        )
        .await
        .unwrap();
        // Told as the row was kept, whenever the listener hears of it too.
        ours = store::holdings(&site.db, OWNER, &personal).await.unwrap();
        super::super::tell_log_links(
            OWNER,
            "log.vector",
            json!({ "audience": personal, "vector": ours.vector }),
        );
    }
    // Behind them all.
    super::super::tell_log_links(
        OWNER,
        "lease.epoch",
        json!({ "epoch": 9, "holder": "node", "how": "first" }),
    );
    go.send(()).unwrap();

    let mut rows = 0;
    let mut back = Vec::new();
    loop {
        let frame = mac.next().await;
        if frame["t"] == "lease.epoch" {
            break;
        }
        if frame["body"]["audience"] != personal.as_str() {
            continue;
        }
        if frame["t"] == "log.rows" {
            rows += frame["body"]["rows"].as_array().unwrap().len();
        } else if frame["t"] == "log.vector" && rows > 0 {
            let said: std::collections::BTreeMap<String, u64> =
                serde_json::from_value(frame["body"]["vector"].clone()).unwrap();
            back.push(
                ours.vector
                    .iter()
                    .map(|(origin, held)| {
                        held.saturating_sub(said.get(origin).copied().unwrap_or(0))
                    })
                    .sum(),
            );
        }
    }
    assert_eq!(rows, 4);
    site.close().await;
    Some(back)
}

/// Written before the site counts what it holds: its answer says them all,
/// and none of the vectors waiting says less after it.
#[tokio::test]
async fn rows_written_during_a_long_frame_are_not_asked_back_one_vector_at_a_time() {
    if let Some(back) = sent_back("log.vector").await {
        assert_eq!(back, [0], "its answer, and nothing it covers");
    }
}

/// Written between that count and the page: they are in the page, so its
/// answer says them.
#[tokio::test]
async fn rows_kept_as_the_page_is_read_are_in_the_vector_after_it() {
    if let Some(back) = sent_back("a page of log.vector").await {
        assert_eq!(back, [0], "its answer, and nothing it covers");
    }
}
