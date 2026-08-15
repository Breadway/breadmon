//! `bread.mon.*` event integration — optional, non-blocking. See
//! `EVENTS.md` at the repo root for the full contract. breadmon works
//! identically with or without breadd running; every call here is
//! fire-and-forget (`BreadClient::emit` never blocks or errors this
//! process) so a missing or restarting breadd never affects apply itself.

use bread_utils::bread_client::BreadClient;
use serde_json::{json, Value};

/// This app's id in bread's sibling-app namespace registry
/// (`bread_shared::apps::KNOWN_APPS`) — events publish as `bread.mon.*`.
pub const APP_ID: &str = "mon";

/// JSON payload for `bread.mon.applied`. `profile` is the named snapshot
/// that was just applied, or `null` for an ad-hoc layout.
pub fn applied_data(profile: Option<&str>) -> Value {
    json!({ "profile": profile })
}

/// Publishes `bread.mon.applied` after a successful hyprctl apply.
/// Fire-and-forget and non-fatal by design — breadd being absent or not
/// installed must never affect breadmon's own apply path.
pub fn emit_applied(profile: Option<&str>) {
    BreadClient::connect(APP_ID).emit("bread.mon.applied", applied_data(profile));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_data_serializes_name_or_null() {
        assert_eq!(applied_data(Some("dock")), json!({ "profile": "dock" }));
        assert_eq!(applied_data(None), json!({ "profile": null }));
    }

    #[test]
    fn emit_applied_is_silent_when_breadd_is_down() {
        // No daemon in the unit-test environment; must not panic or block.
        emit_applied(Some("dock"));
        emit_applied(None);
    }
}
