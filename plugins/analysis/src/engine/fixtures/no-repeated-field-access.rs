fn process(state: &mut AppState) {
    state.config.db.set_timeout(30);
    state.name.set_value("hello");
    state.config.db.set_pool_size(10);
}
