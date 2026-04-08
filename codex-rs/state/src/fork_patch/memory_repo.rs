use crate::ThreadMetadata;
use sqlx::Executor;
use sqlx::Row;
use sqlx::Sqlite;
use sqlx::query::Query;
use sqlx::sqlite::SqliteArguments;

pub(crate) const GLOBAL_MEMORY_SCOPE_KIND: &str = "global";
pub(crate) const GLOBAL_MEMORY_SCOPE_KEY: &str = "global";

pub(crate) fn default_memory_scope() -> (String, String) {
    (
        GLOBAL_MEMORY_SCOPE_KIND.to_string(),
        GLOBAL_MEMORY_SCOPE_KEY.to_string(),
    )
}

pub(crate) fn bind_thread_memory_scope<'q>(
    query: Query<'q, Sqlite, SqliteArguments<'q>>,
    metadata: &'q ThreadMetadata,
) -> Query<'q, Sqlite, SqliteArguments<'q>> {
    bind_memory_scope(
        query,
        metadata.memory_scope_kind.as_str(),
        metadata.memory_scope_key.as_str(),
    )
}

pub(crate) fn bind_memory_scope<'q>(
    query: Query<'q, Sqlite, SqliteArguments<'q>>,
    memory_scope_kind: &'q str,
    memory_scope_key: &'q str,
) -> Query<'q, Sqlite, SqliteArguments<'q>> {
    query.bind(memory_scope_kind).bind(memory_scope_key)
}

pub(crate) fn bind_phase2_job_key<'q>(
    query: Query<'q, Sqlite, SqliteArguments<'q>>,
    memory_scope_kind: &'q str,
    memory_scope_key: &'q str,
) -> Query<'q, Sqlite, SqliteArguments<'q>> {
    query.bind(phase2_job_key(memory_scope_kind, memory_scope_key))
}

pub(crate) async fn fetch_thread_memory_scope<'e, E>(
    executor: E,
    thread_id: &str,
) -> anyhow::Result<(String, String)>
where
    E: Executor<'e, Database = Sqlite>,
{
    let scope = sqlx::query(
        r#"
SELECT memory_scope_kind, memory_scope_key
FROM threads
WHERE id = ?
        "#,
    )
    .bind(thread_id)
    .fetch_one(executor)
    .await?;

    Ok((
        scope.try_get("memory_scope_kind")?,
        scope.try_get("memory_scope_key")?,
    ))
}

pub(crate) async fn thread_has_phase2_selection<'e, E>(
    executor: E,
    thread_id: &str,
) -> anyhow::Result<bool>
where
    E: Executor<'e, Database = Sqlite>,
{
    let selected_for_phase2 = sqlx::query_scalar::<_, i64>(
        r#"
SELECT selected_for_phase2
FROM stage1_outputs
WHERE thread_id = ?
        "#,
    )
    .bind(thread_id)
    .fetch_optional(executor)
    .await?
    .unwrap_or(0);

    Ok(selected_for_phase2 != 0)
}

pub(crate) async fn clear_phase2_selection_in_scope<'e, E>(
    executor: E,
    memory_scope_kind: &str,
    memory_scope_key: &str,
) -> anyhow::Result<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    bind_memory_scope(
        sqlx::query(
            r#"
UPDATE stage1_outputs
SET
    selected_for_phase2 = 0,
    selected_for_phase2_source_updated_at = NULL
WHERE thread_id IN (
    SELECT id
    FROM threads
    WHERE memory_scope_kind = ?
      AND memory_scope_key = ?
)
  AND (selected_for_phase2 != 0 OR selected_for_phase2_source_updated_at IS NOT NULL)
            "#,
        ),
        memory_scope_kind,
        memory_scope_key,
    )
    .execute(executor)
    .await?;

    Ok(())
}

pub(crate) fn phase2_job_key(memory_scope_kind: &str, memory_scope_key: &str) -> String {
    if memory_scope_kind == GLOBAL_MEMORY_SCOPE_KIND && memory_scope_key == GLOBAL_MEMORY_SCOPE_KEY
    {
        GLOBAL_MEMORY_SCOPE_KEY.to_string()
    } else {
        format!("{memory_scope_kind}:{memory_scope_key}")
    }
}
