fn example() -> Vec<String> {
    Vec::new()
}

// Type alias with deep nesting should not trigger — the alias IS the fix.
type ProcessTable = Arc<Mutex<Vec<AttachedProcess>>>;
