//! The group scenes of a consistency scene file (see
//! `agent::consistency_recording`): the lines heard in a world of their own,
//! and her whole prompt when she answers the last one written down as the
//! site put it together. Her soul is the scene's, not `g0::SOUL`.
//!
//! Also needs `MYRIAD_G0_DATABASE_URL`.

use super::*;
use crate::services::agent::consistency_recording::{at, her_on_the_site, record, scenes};

#[tokio::test(start_paused = true)]
#[ignore = "writes recordings of her prompts for a scene file"]
async fn her_group_prompts_recorded() {
    let Some((file, out)) = scenes() else {
        eprintln!("skipped: MYRIAD_CONSISTENCY_SCENES / MYRIAD_CONSISTENCY_OUT not set");
        return;
    };
    let _turns = WHOLE_TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (index, scene) in file["scenes"].as_array().unwrap().iter().enumerate() {
        let occasion = scene["occasion"].as_str().unwrap();
        if !occasion.starts_with("group_") {
            continue;
        }
        let name = scene["scene"].as_str().unwrap();
        let venue = scene["group"]["venue"].as_str().unwrap();
        // Each scene a chat of its own (the same name): what one heard is
        // not in the next one's talk.
        let chat: i64 = venue
            .strip_prefix("telegram:")
            .unwrap()
            .parse::<i64>()
            .unwrap()
            - index as i64;
        let venue = &format!("telegram:{chat}");
        let lines = scene["lines"].as_array().unwrap();
        let latest = &scene["latest"];
        let mut world = World::new(0.5);
        let Some(schema) = database(&mut world, name) else {
            return;
        };
        let first = lines
            .first()
            .map_or_else(|| at(&latest["at"]), |line| at(&line["at"]));
        world.base = first;
        world.titles.insert(
            venue.into(),
            scene["group"]["name"].as_str().unwrap().into(),
        );
        // The owner, who saved her.
        sea_orm::ConnectionTrait::execute_raw(
            &schema.db,
            sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO users (id, username, display_name) VALUES (1, 'owner1', $1)",
                [file["owner"]["name"].as_str().unwrap().into()],
            ),
        )
        .await
        .unwrap();
        her_on_the_site(&schema.db, &file).await;
        let her = &file["her"];
        let site = {
            let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
            myriad_merope::speaking::site_name(config.site_title.as_deref())
        };
        world.soul = myriad_merope::speaking::soul(
            her["name"].as_str().unwrap(),
            her["personality"].as_str().unwrap(),
            &myriad_merope::speaking::Home::site(&site),
        );
        // Members are paired with a site user each; anyone else is from
        // outside the community.
        let mut members = std::collections::BTreeMap::new();
        for line in lines.iter().chain([latest]) {
            if line["member"].as_bool() == Some(false) {
                continue;
            }
            let id = line["id"].as_str().unwrap();
            if !members.contains_key(id) {
                let user_id = 80_000 + members.len() as i32;
                world.members.insert(id.into(), user_id);
                site_member(
                    &schema.db,
                    user_id,
                    &format!("u{user_id}"),
                    line["from"].as_str().unwrap(),
                )
                .await;
                members.insert(id.to_string(), user_id);
            }
        }
        let operation = if latest["member"].as_bool() == Some(false) {
            "group_stranger"
        } else {
            "chat"
        };
        world
            .says
            .entry(operation.into())
            .or_default()
            .push_back((2.0, Ok("嗯".to_string())));
        g0::enter(world);
        let mut clock = first;
        for (n, line) in lines.iter().chain([latest]).enumerate() {
            let addressed = std::ptr::eq(line, latest);
            // The line she answers comes at the scene's now, as the other
            // end has it.
            let when = if addressed {
                at(&scene["now"])
            } else {
                at(&line["at"])
            };
            wait((when - clock).num_milliseconds() as f64 / 1000.0).await;
            clock = when;
            hear(super::line(
                chat,
                // Each scene's own message ids: one already heard is not heard again.
                index as i64 * 1000 + n as i64 + 1,
                line["id"].as_str().unwrap().parse().unwrap(),
                line["from"].as_str().unwrap(),
                line["text"].as_str().unwrap(),
                addressed,
            ))
            .await;
        }
        wait(600.0).await;
        let world = g0::leave().expect("the world of the scene");
        let said = world
            .log
            .iter()
            .filter(|entry| entry["kind"] == "say")
            .filter_map(|entry| entry["what"]["prompt"].as_str())
            .last()
            .unwrap_or_else(|| {
                for entry in &world.log {
                    let what = entry["what"].to_string();
                    eprintln!(
                        "{} {}",
                        entry["kind"],
                        what.chars().take(300).collect::<String>()
                    );
                }
                panic!("{name}: she was never asked to speak")
            });
        // A group turn goes to the model as one text (`call::Model::say`).
        record(&out, scene, &[("user", said)]);
        schema.drop_schema();
    }
}
