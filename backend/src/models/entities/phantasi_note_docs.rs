//! 云端笔记文档。草稿和定时稿只活在这里，发布后才有 `phantasi_items`。

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "phantasi_note_docs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// 发布后才有。草稿 / 定时恒为 NULL。
    pub item_id: Option<i32>,
    #[sea_orm(column_type = "Text")]
    pub title: String,
    #[sea_orm(column_type = "Text")]
    pub content_md: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub topic: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub image: Option<String>,
    /// `draft` | `scheduled` | `published`
    pub status: String,
    pub scheduled_at: Option<DateTimeWithTimeZone>,
    /// 打算公开时用的发布时间；未发布也可先记着。
    pub published_at: Option<DateTimeWithTimeZone>,
    pub revision: i64,
    pub last_edited_by: Option<i32>,
    /// 到点发布失败时写给管理端看。成功就清空。
    #[sea_orm(column_type = "Text", nullable)]
    pub last_error: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// Workbench rows carry only an excerpt, even when a draft is very large.
/// Derive the cover in SQL so an image late in the document is still found
/// without transferring the entire Markdown body to the application.
pub fn list_query() -> sea_orm::Select<Entity> {
    use sea_orm::{QuerySelect, sea_query::Expr};
    Entity::find()
        .select_only()
        .columns([
            Column::Id,
            Column::UserId,
            Column::ItemId,
            Column::Title,
            Column::Topic,
            Column::Status,
            Column::ScheduledAt,
            Column::PublishedAt,
            Column::Revision,
            Column::LastEditedBy,
            Column::LastError,
            Column::CreatedAt,
            Column::UpdatedAt,
        ])
        .column_as(
            Expr::cust(r"LEFT(REGEXP_REPLACE(content_md, '^\s+', ''), 80)"),
            Column::ContentMd,
        )
        .column_as(
            Expr::cust(
                r"COALESCE(NULLIF(REGEXP_REPLACE(image, '^\s+|\s+$', '', 'g'), ''),
                  (SELECT capture[1]
                   FROM regexp_matches(content_md, '!\[[^]]*\]\([[:space:]]*([^[:space:])]+)[^)]*\)', 'g')
                        WITH ORDINALITY AS images(capture, position)
                   WHERE capture[1] NOT LIKE '#%'
                   ORDER BY position LIMIT 1))",
            ),
            Column::Image,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{
        ActiveValue::Set, ConnectOptions, ConnectionTrait, Database, DatabaseBackend, QueryOrder,
        Schema, Statement,
    };

    #[tokio::test]
    async fn list_projection_bounds_markdown_and_preserves_late_covers() {
        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            eprintln!("skipped PostgreSQL projection test: PHANTASI_TEST_DATABASE_URL is unset");
            return;
        };
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        let sql = Schema::new(DatabaseBackend::Postgres)
            .create_table_from_entity(Entity)
            .to_string(sea_orm::sea_query::PostgresQueryBuilder)
            .replacen("CREATE TABLE", "CREATE TEMP TABLE", 1);
        db.execute_unprepared(&sql).await.unwrap();

        let long_body = format!(
            " \n\t{}\n![anchor](#ignored)\n![cover](https://img.example/late.jpg \"Cover\")\n![other](https://img.example/other.jpg)",
            "正文🙂".repeat(10_000),
        );
        let cases = [
            (
                long_body.clone(),
                None,
                true,
                Some("https://img.example/late.jpg"),
            ),
            (
                long_body,
                Some(" /explicit.jpg "),
                true,
                Some("/explicit.jpg"),
            ),
            (" \n\t ".into(), None, false, None),
            ("短文".into(), None, true, None),
            (
                "![内文]( /body.jpg )".into(),
                Some(" \t\n "),
                true,
                Some("/body.jpg"),
            ),
        ];
        let now = crate::clock::now().fixed_offset();
        for (index, (body, image, _, _)) in cases.iter().enumerate() {
            ActiveModel {
                id: Set(index as i32 + 1),
                user_id: Set(1),
                title: Set(format!("Draft {index}")),
                content_md: Set(body.clone()),
                image: Set(image.map(str::to_owned)),
                status: Set("draft".into()),
                revision: Set(9),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let rows = list_query()
            .order_by_asc(Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(rows.len(), cases.len());
        for (row, (body, _, has_body, cover)) in rows.iter().zip(&cases) {
            assert_eq!(
                row.content_md,
                body.trim_start().chars().take(80).collect::<String>()
            );
            assert_eq!(!row.content_md.trim().is_empty(), *has_body);
            assert_eq!(row.image.as_deref(), *cover);
            assert_eq!(row.revision, 9);
        }
        // Editing still receives the original body, not the list excerpt.
        let full = Entity::find_by_id(1).one(&db).await.unwrap().unwrap();
        assert_eq!(full.content_md, cases[0].0);

        // RSS descriptions may themselves be full articles. The shared preview
        // expression caps Unicode characters and retains NULL summaries.
        let summary_sql = super::super::phantasi_items::preview_summary_sql("$1::text");
        for input in [Some("摘要🙂".repeat(5000)), None] {
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    format!("SELECT {summary_sql} AS summary"),
                    [input.clone().into()],
                ))
                .await
                .unwrap()
                .unwrap();
            let summary: Option<String> = row.try_get("", "summary").unwrap();
            assert_eq!(
                summary,
                input.map(|value| value.chars().take(2048).collect())
            );
        }
    }
}
