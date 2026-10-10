use std::sync::Arc;

use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::cli::FrameworkTool;
use crate::config::LiveConfig;
use crate::types::{DashboardPanelId, FanProfileAutoSwitch};

const POLL_SECS: u64 = 2;

/// Switches the active fan profile when the power source (or its assignment) changes.
/// A manual pick sticks until then.
pub async fn run(cli_lock: Arc<tokio::sync::RwLock<Option<FrameworkTool>>>, cfg: LiveConfig) {
    info!("Fan profile auto-switch task started");
    let mut switcher = AutoSwitcher::default();

    loop {
        sleep(Duration::from_secs(POLL_SECS)).await;

        let (auto, fan_enabled) = {
            let cfg = cfg.read().await;
            (
                cfg.fan.auto_switch.clone(),
                cfg.ui.is_panel_enabled(DashboardPanelId::Fan),
            )
        };
        let Some(auto) = auto.filter(|_| fan_enabled) else {
            switcher = AutoSwitcher::default();
            continue;
        };
        let Some(cli) = cli_lock.read().await.clone() else {
            continue;
        };
        let Ok(power) = cli.power().await else {
            continue;
        };
        let Some(ac_present) = power.ac_present else {
            continue;
        };
        let Some(id) = switcher.observe(ac_present, &auto) else {
            continue;
        };

        let name = {
            let cfg = cfg.read().await;
            if cfg.fan.active_profile.as_deref() == Some(id.as_str()) {
                continue;
            }
            let profiles = cfg.fan.profiles.as_deref().unwrap_or_default();
            profiles.iter().find(|p| p.id == id).map(|p| p.name.clone())
        };
        info!(
            "fan profiles: {} -> switching to '{}'",
            if ac_present { "plugged in" } else { "on battery" },
            name.as_deref().unwrap_or(&id)
        );
        if let Err(e) = cfg.persist(None, |c| c.fan.activate_profile(&id)).await {
            warn!("fan profiles: failed to persist switch: {}", e);
        }
    }
}

#[derive(Default)]
struct AutoSwitcher {
    pending_ac: Option<bool>,
    applied: Option<(bool, Option<String>)>,
}

impl AutoSwitcher {
    /// Returns the profile to activate when the settled power source or its assignment changes.
    fn observe(&mut self, ac_present: bool, auto: &FanProfileAutoSwitch) -> Option<String> {
        // Require two matching reads so a dock renegotiating USB-C PD doesn't flip profiles.
        if self.pending_ac != Some(ac_present) {
            self.pending_ac = Some(ac_present);
            return None;
        }
        let target = if ac_present {
            auto.ac.clone()
        } else {
            auto.battery.clone()
        };
        let key = (ac_present, target.clone());
        if self.applied.as_ref() == Some(&key) {
            return None;
        }
        self.applied = Some(key);
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(ac: Option<&str>, battery: Option<&str>) -> FanProfileAutoSwitch {
        FanProfileAutoSwitch {
            ac: ac.map(String::from),
            battery: battery.map(String::from),
        }
    }

    #[test]
    fn switches_on_settled_source_change_only() {
        let a = auto(Some("home"), Some("quiet"));
        let mut s = AutoSwitcher::default();
        assert_eq!(s.observe(true, &a), None);
        assert_eq!(s.observe(true, &a).as_deref(), Some("home"));
        assert_eq!(s.observe(true, &a), None);

        // A single flapping read is ignored.
        assert_eq!(s.observe(false, &a), None);
        assert_eq!(s.observe(true, &a), None);
        assert_eq!(s.observe(true, &a), None);

        assert_eq!(s.observe(false, &a), None);
        assert_eq!(s.observe(false, &a).as_deref(), Some("quiet"));
        assert_eq!(s.observe(false, &a), None);
    }

    #[test]
    fn reassigning_current_source_switches() {
        let mut s = AutoSwitcher::default();
        s.observe(true, &auto(Some("home"), None));
        assert_eq!(s.observe(true, &auto(Some("home"), None)).as_deref(), Some("home"));
        assert_eq!(s.observe(true, &auto(Some("home"), Some("quiet"))), None);
        assert_eq!(
            s.observe(true, &auto(Some("cool"), Some("quiet"))).as_deref(),
            Some("cool")
        );
    }

    #[test]
    fn unassigned_source_keeps_current_profile() {
        let a = auto(Some("home"), None);
        let mut s = AutoSwitcher::default();
        s.observe(false, &a);
        assert_eq!(s.observe(false, &a), None);
        s.observe(true, &a);
        assert_eq!(s.observe(true, &a).as_deref(), Some("home"));
    }
}
