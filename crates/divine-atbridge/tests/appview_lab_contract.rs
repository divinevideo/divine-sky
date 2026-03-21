use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn appview_schema_includes_media_view_tables_and_queries() {
    let up = std::fs::read_to_string(repo_root().join("migrations/003_appview_read_model/up.sql"))
        .unwrap();
    let queries =
        std::fs::read_to_string(repo_root().join("crates/divine-bridge-db/src/queries.rs"))
            .unwrap();

    for table in [
        "appview_repos",
        "appview_profiles",
        "appview_posts",
        "appview_media_views",
        "appview_service_state",
    ] {
        assert!(up.contains(table), "missing {table}");
    }

    assert!(queries.contains("upsert_appview_media_view"));
    assert!(queries.contains("load_post_with_media_view"));
}
