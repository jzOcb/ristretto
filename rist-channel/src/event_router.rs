//! Event-to-target routing for the channel server.

use std::path::PathBuf;

use rist_shared::protocol::Event;
use rist_shared::{EventFilter, SessionId};

/// Routing table for daemon events.
#[derive(Debug, Default)]
pub struct EventRouter {
    /// Routes keyed by event filter.
    routes: Vec<EventRoute>,
}

/// One routing rule from an event filter to one or more targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRoute {
    /// Filter used to match events.
    pub event_filter: EventFilter,
    /// Targets that should receive matching events.
    pub targets: Vec<RouteTarget>,
}

/// Delivery target for routed events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteTarget {
    /// Push via Claude Code MCP channel protocol.
    McpChannel { session_id: SessionId },
    /// Write event to a file for polling or tailing.
    FileNotification { path: PathBuf },
    /// Send event to an HTTP webhook.
    Webhook { url: String },
    /// Write event to agent stdin via the daemon.
    AgentStdin { session_id: SessionId },
}

impl EventRouter {
    /// Creates an empty router.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a route for an event filter to a target.
    pub fn add_route(&mut self, filter: EventFilter, target: RouteTarget) {
        if let Some(route) = self
            .routes
            .iter_mut()
            .find(|route| route.event_filter == filter)
        {
            route.targets.push(target);
            return;
        }

        self.routes.push(EventRoute {
            event_filter: filter,
            targets: vec![target],
        });
    }

    /// Returns all targets that should receive an event.
    #[must_use]
    pub fn route(&self, event: &Event) -> Vec<&RouteTarget> {
        self.routes
            .iter()
            .filter(|route| filter_matches_event(&route.event_filter, event))
            .flat_map(|route| route.targets.iter())
            .collect()
    }

    /// Removes all session-bound routes for a session identifier.
    pub fn remove_session(&mut self, session_id: SessionId) {
        for route in &mut self.routes {
            route.targets.retain(|target| match target {
                RouteTarget::McpChannel {
                    session_id: target_session_id,
                }
                | RouteTarget::AgentStdin {
                    session_id: target_session_id,
                } => *target_session_id != session_id,
                RouteTarget::FileNotification { .. } | RouteTarget::Webhook { .. } => true,
            });
        }

        self.routes.retain(|route| !route.targets.is_empty());
    }
}

fn filter_matches_event(filter: &EventFilter, event: &Event) -> bool {
    match (filter, event) {
        (EventFilter::All, _) => true,
        (EventFilter::PtyData, Event::PtyData { .. }) => true,
        (EventFilter::StatusChange, Event::StatusChange { .. }) => true,
        (EventFilter::AgentExited, Event::AgentExited { .. }) => true,
        (EventFilter::TaskUpdate, Event::TaskUpdate { .. }) => true,
        (EventFilter::ContextWarning, Event::ContextWarning { .. }) => true,
        (EventFilter::LoopDetected, Event::LoopDetected { .. }) => true,
        (EventFilter::Unknown, _) | (_, Event::Unknown) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::*;
    use rist_shared::{AgentStatus, TaskStatus};

    #[test]
    fn add_route_and_route_returns_matching_targets() {
        let session_id = SessionId(Uuid::nil());
        let file_target = RouteTarget::FileNotification {
            path: PathBuf::from("/tmp/ristretto-events.jsonl"),
        };
        let stdin_target = RouteTarget::AgentStdin { session_id };

        let mut router = EventRouter::new();
        router.add_route(EventFilter::StatusChange, file_target.clone());
        router.add_route(EventFilter::StatusChange, stdin_target.clone());
        router.add_route(
            EventFilter::All,
            RouteTarget::Webhook {
                url: "http://localhost:8080/events".to_owned(),
            },
        );

        let event = Event::StatusChange {
            id: session_id,
            old: AgentStatus::Idle,
            new: AgentStatus::Working,
        };

        let targets = router.route(&event);
        assert_eq!(targets.len(), 3);
        assert!(targets.contains(&&file_target));
        assert!(targets.contains(&&stdin_target));
    }

    #[test]
    fn remove_session_cleans_up_all_session_targets() {
        let session_id = SessionId(Uuid::nil());
        let other_session_id = SessionId(Uuid::from_u128(1));
        let mut router = EventRouter::new();

        router.add_route(
            EventFilter::AgentExited,
            RouteTarget::McpChannel { session_id },
        );
        router.add_route(
            EventFilter::AgentExited,
            RouteTarget::AgentStdin { session_id },
        );
        router.add_route(
            EventFilter::AgentExited,
            RouteTarget::AgentStdin {
                session_id: other_session_id,
            },
        );

        router.remove_session(session_id);

        let event = Event::AgentExited {
            id: session_id,
            exit_code: 0,
        };
        let targets = router.route(&event);
        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0],
            &RouteTarget::AgentStdin {
                session_id: other_session_id,
            }
        );
    }

    #[test]
    fn route_returns_empty_for_unmatched_events() {
        let mut router = EventRouter::new();
        router.add_route(
            EventFilter::TaskUpdate,
            RouteTarget::Webhook {
                url: "http://localhost:8080/events".to_owned(),
            },
        );

        let event = Event::StatusChange {
            id: SessionId(Uuid::nil()),
            old: AgentStatus::Thinking,
            new: AgentStatus::Waiting,
        };
        assert!(router.route(&event).is_empty());

        let task_event = Event::TaskUpdate {
            task_id: "task-1".to_owned(),
            status: TaskStatus::Working,
        };
        assert_eq!(router.route(&task_event).len(), 1);
    }
}
