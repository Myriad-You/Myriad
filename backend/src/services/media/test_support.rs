//! Opt-in integration fixtures, one PostgreSQL schema and media root per test.
use super::*;
use sea_orm::{ConnectionTrait, DatabaseConnection};

pub(super) struct Fixture {
    pub db: DatabaseConnection,
    pub service: MediaService,
    schema: String,
}
impl Fixture {
    pub async fn new() -> Option<Self> {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            eprintln!("skipping media integration test: MYRIAD_MEDIA_TEST_DATABASE_URL is unset");
            return None;
        };
        let admin = sea_orm::Database::connect(&url).await.unwrap();
        let schema = format!("media_test_{}", Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        admin.close().await.unwrap();
        let mut options = sea_orm::ConnectOptions::new(url);
        options
            .set_schema_search_path(&schema)
            .max_connections(5)
            .sqlx_logging(false);
        let db = sea_orm::Database::connect(options).await.unwrap();
        ::migration::Migrator::up(&db, None).await.unwrap();
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (1, 'media-test')")
            .await
            .unwrap();
        let service = MediaService::new(std::env::temp_dir().join(&schema));
        Some(Self {
            db,
            service,
            schema,
        })
    }
    pub async fn image(&self) -> MediaAsset {
        self.service
            .create_from_bytes(
                &self.db,
                MediaContext::site(MediaActor::admin(1).unwrap(), MediaSource::Upload),
                NewMediaBytes {
                    bytes: png().into(),
                    claimed_mime: "image/png".into(),
                    filename: "test.png".into(),
                    max_bytes: 1024 * 1024,
                    derived_from_id: None,
                    exposure: MediaExposure::Private,
                },
            )
            .await
            .unwrap()
    }
    pub async fn close(self) {
        self.db
            .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .unwrap();
        self.db.close().await.unwrap();
        let _ = tokio::fs::remove_dir_all(self.service.store().root()).await;
    }
}
pub(super) fn png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=").unwrap()
}
