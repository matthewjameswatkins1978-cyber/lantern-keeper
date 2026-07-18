pub const BOOTSTRAP_QUERY: &str = r#"
DEFINE TABLE IF NOT EXISTS __lighting_schema SCHEMALESS;
UPSERT __lighting_schema:bootstrap CONTENT {
    project: "Lantern Keeper",
    service: "Lighting",
    version: 1,
    updated_at: time::now()
};
"#;
