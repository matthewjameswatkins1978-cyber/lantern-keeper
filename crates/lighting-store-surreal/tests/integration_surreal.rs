use lighting_store_surreal::{StoreConfig, SurrealStore};
use uuid::Uuid;

#[tokio::test]
async fn connects_initialises_schema_and_runs_health_check() {
    if skip_integration_tests() {
        return;
    }

    fn skip_integration_tests() -> bool {
        matches!(
            std::env::var("LIGHTING_SKIP_INTEGRATION_TESTS").as_deref(),
            Ok("1") | Ok("true")
        )
    }

    dotenvy::dotenv().ok();

    let mut config = StoreConfig::from_env();
    config.database = format!("lighting_test_{}", Uuid::new_v4().simple());

    let store = SurrealStore::connect(&config)
        .await
        .unwrap_or_else(|error| panic!("failed to connect to local SurrealDB. Start it with `docker compose up -d surrealdb` from the lantern-keeper directory, or set LIGHTING_SKIP_INTEGRATION_TESTS=1 to deliberately skip this integration test. Error: {error}"));

    store
        .initialise_schema()
        .await
        .expect("schema initialisation should succeed against the disposable test database");
    store
        .health_check()
        .await
        .expect("health check should succeed against the disposable test database");
}
