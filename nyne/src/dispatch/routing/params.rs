use std::collections::HashMap;

/// Accumulated captures from route matching.
///
/// Captures from parent segments propagate to all child handlers [DD-9].
#[derive(Debug, Default, Clone)]
pub struct RouteParams {
    singles: HashMap<&'static str, String>,
    rest: HashMap<&'static str, Vec<String>>,
}

/// Insert and retrieve captured route parameters.
impl RouteParams {
    /// Insert a single-segment capture.
    pub fn insert_single(&mut self, name: &'static str, value: String) { self.singles.insert(name, value); }

    /// Insert a rest capture (1+ segments).
    pub(super) fn insert_rest(&mut self, name: &'static str, segments: Vec<String>) {
        self.rest.insert(name, segments);
    }

    /// Get a single-segment capture.
    ///
    /// # Panics
    /// Panics if the capture name doesn't exist — programmer error.
    #[allow(clippy::panic)]
    pub fn get(&self, name: &str) -> &str {
        self.singles
            .get(name)
            .map_or_else(|| panic!("no capture named '{name}' in this route"), String::as_str)
    }

    /// Get a rest capture (1+ segments).
    ///
    /// # Panics
    /// Panics if the capture name doesn't exist — programmer error.
    #[allow(clippy::panic)]
    pub fn get_rest(&self, name: &str) -> &[String] {
        self.rest
            .get(name)
            .map_or_else(|| panic!("no rest capture named '{name}' in this route"), Vec::as_slice)
    }
}
