use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteHandle {
    pub id: Uuid,
    pub project_id: Uuid,
    pub environment: String,
    pub target_url: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DrainResult {
    CompletedGracefully,
    ForcedTimeout,
}

pub trait TrafficRouter: Send + Sync {
    fn prepare(&mut self, release: &crate::releases::Release) -> Result<RouteHandle>;
    fn activate(&mut self, route: &RouteHandle) -> Result<()>;
    fn drain(&mut self, route: &RouteHandle, timeout: Duration) -> Result<DrainResult>;
    fn remove(&mut self, route: &RouteHandle) -> Result<()>;
}

#[derive(Default, Clone)]
pub struct MockTrafficRouter {
    pub active_routes:
        std::sync::Arc<std::sync::Mutex<std::collections::HashMap<(Uuid, String), RouteHandle>>>,
    pub prepared_routes: std::sync::Arc<std::sync::Mutex<Vec<RouteHandle>>>,
    pub drained_routes: std::sync::Arc<std::sync::Mutex<Vec<(RouteHandle, DrainResult)>>>,
    pub removed_routes: std::sync::Arc<std::sync::Mutex<Vec<RouteHandle>>>,
}

impl MockTrafficRouter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TrafficRouter for MockTrafficRouter {
    fn prepare(&mut self, release: &crate::releases::Release) -> Result<RouteHandle> {
        let handle = RouteHandle {
            id: release.id,
            project_id: release.project_id,
            environment: release.environment.clone(),
            target_url: release
                .url
                .clone()
                .unwrap_or_else(|| format!("http://127.0.0.1:{}", release.port.unwrap_or(8080))),
        };
        self.prepared_routes.lock().unwrap().push(handle.clone());
        Ok(handle)
    }

    fn activate(&mut self, route: &RouteHandle) -> Result<()> {
        let key = (route.project_id, route.environment.clone());
        self.active_routes
            .lock()
            .unwrap()
            .insert(key, route.clone());
        Ok(())
    }

    fn drain(&mut self, route: &RouteHandle, _timeout: Duration) -> Result<DrainResult> {
        self.drained_routes
            .lock()
            .unwrap()
            .push((route.clone(), DrainResult::CompletedGracefully));
        Ok(DrainResult::CompletedGracefully)
    }

    fn remove(&mut self, route: &RouteHandle) -> Result<()> {
        let key = (route.project_id, route.environment.clone());
        self.active_routes.lock().unwrap().remove(&key);
        self.removed_routes.lock().unwrap().push(route.clone());
        Ok(())
    }
}
