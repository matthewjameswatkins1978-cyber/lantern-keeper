use lighting_store_surreal::SurrealStore;

#[derive(Clone)]
pub struct AppState {
    pub store: SurrealStore,
}

impl AppState {
    pub fn new(store: SurrealStore) -> Self {
        Self { store }
    }
}
