//! Her log's pages over a real link (`log.pages`): a batch ending a page of
//! the device's that the site cannot settle closes the link, so paging
//! starts over on the next.

use super::*;

#[tokio::test]
async fn a_page_the_site_cannot_settle_closes_the_link() {
    let _minds = crate::services::her_log::lease::minds_go_to_devices_in_test(true);
    let Some(site) = site().await else {
        return;
    };
    let _turn = crate::services::her_log::lend::SERIAL.lock().await;
    crate::services::her_log::test_settings::set(crate::services::her_log::Settings {
        enabled: true,
        ..crate::services::her_log::Settings::default()
    });
    crate::services::her_log::store::start(&site.db, OWNER)
        .await
        .unwrap();
    let (device, token) = device(&site.db, "MacBook").await;
    let mut mac = Child::open(&site, &token).await;
    let welcome = mac.hello_with(&["partner", "log2", "log.pages"]).await;
    assert_eq!(welcome["body"]["sync"], "on");
    assert!(
        welcome["body"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("log.pages"))
    );
    let personal = format!("personal:{}", welcome["body"]["partner"].as_str().unwrap());
    // Her log goes off on the server with a page of the Mac's on its way:
    // the batch that ends it is answered, and nothing names it back.
    crate::services::her_log::test_settings::set(crate::services::her_log::Settings::default());
    let seq = mac
        .send(
            "log.rows",
            Some("batch_one"),
            json!({ "audience": personal, "rows": [], "payloads": [], "more": "page_one" }),
        )
        .await;
    let refused = mac.expect("error").await;
    assert_eq!(refused["body"]["code"], "sync_off");
    assert_eq!(refused["body"]["ref"], seq);
    assert_eq!(mac.closed().await, (4503, "unavailable".to_string()));
    let unsettled = crate::services::agent::link::counts::pages_unsettled_by(&device.id).unwrap();
    assert_eq!(unsettled.in_a_row, 1);
    site.close().await;
}
