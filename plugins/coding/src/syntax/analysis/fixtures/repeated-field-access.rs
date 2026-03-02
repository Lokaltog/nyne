fn process(state: &mut AppState) {
    state.config.db.set_timeout(30);
    state.config.db.set_pool_size(10);
    state.config.db.set_retries(3);
    state.config.db.enable_logging();
}
