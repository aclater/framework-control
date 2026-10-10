use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock, RwLockReadGuard};
use tracing::info;

use crate::types::{Config, ConfigEvent, PartialConfig, MAX_BATTERY_POLL_MS, MIN_BATTERY_POLL_MS};

#[derive(Clone)]
pub struct LiveConfig {
    inner: Arc<RwLock<Config>>,
    events: broadcast::Sender<ConfigEvent>,
}

impl LiveConfig {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(RwLock::new(load())),
            events,
        }
    }

    pub async fn read(&self) -> RwLockReadGuard<'_, Config> {
        self.inner.read().await
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConfigEvent> {
        self.events.subscribe()
    }

    /// Merge, persist, replace, and broadcast under one write lock.
    pub async fn persist(&self, client_id: Option<String>, apply: impl FnOnce(&mut Config)) -> Result<Config, String> {
        let mut w = self.inner.write().await;
        let mut cfg = w.clone();
        apply(&mut cfg);
        save(&cfg)?;
        *w = cfg.clone();
        let _ = self.events.send(ConfigEvent {
            client_id,
            config: cfg.clone(),
        });
        Ok(cfg)
    }

    pub async fn persist_partial(&self, client_id: Option<String>, req: PartialConfig) -> Result<Config, String> {
        self.persist(client_id, |cfg| apply_partial(cfg, req)).await
    }
}

fn apply_partial(merged: &mut Config, req: PartialConfig) {
    if let Some(fan) = req.fan {
        let mut new_fan = merged.fan.clone();
        if let Some(m) = fan.mode {
            new_fan.mode = Some(m);
        }
        if let Some(man) = fan.manual {
            new_fan.manual = Some(man);
        }
        if let Some(cur) = fan.curve {
            new_fan.curve = Some(cur);
        }
        if let Some(cal) = fan.calibration {
            new_fan.calibration = Some(cal);
        }
        // Overrides are replaced wholesale when provided. An empty array clears them entirely.
        if let Some(ov) = fan.overrides {
            new_fan.overrides = if ov.is_empty() { None } else { Some(ov) };
        }
        if let Some(profiles) = fan.profiles {
            new_fan.profiles = Some(profiles);
        }
        if let Some(auto) = fan.auto_switch {
            new_fan.auto_switch = Some(auto);
        }
        // Normalize first so pending live edits land in the outgoing profile before switching.
        match fan.active_profile {
            Some(id) if new_fan.active_profile.as_ref() != Some(&id) => {
                new_fan.normalize_profiles();
                new_fan.activate_profile(&id);
            }
            _ => {}
        }
        new_fan.normalize_profiles();
        merged.fan = new_fan;
    }
    if let Some(pow) = req.power {
        let mut new_pow = merged.power.clone();
        if let Some(ac_in) = pow.ac {
            let mut ac = new_pow.ac.unwrap_or_default();
            if let Some(s) = ac_in.tdp_watts {
                ac.tdp_watts = Some(s);
            }
            if let Some(s) = ac_in.thermal_limit_c {
                ac.thermal_limit_c = Some(s);
            }
            if let Some(s) = ac_in.epp_preference {
                ac.epp_preference = Some(s);
            }
            if let Some(s) = ac_in.governor {
                ac.governor = Some(s);
            }
            if let Some(s) = ac_in.min_freq_mhz {
                ac.min_freq_mhz = Some(s);
            }
            if let Some(s) = ac_in.max_freq_mhz {
                ac.max_freq_mhz = Some(s);
            }
            new_pow.ac = Some(ac);
        }
        if let Some(bat_in) = pow.battery {
            let mut bat = new_pow.battery.unwrap_or_default();
            if let Some(s) = bat_in.tdp_watts {
                bat.tdp_watts = Some(s);
            }
            if let Some(s) = bat_in.thermal_limit_c {
                bat.thermal_limit_c = Some(s);
            }
            if let Some(s) = bat_in.epp_preference {
                bat.epp_preference = Some(s);
            }
            if let Some(s) = bat_in.governor {
                bat.governor = Some(s);
            }
            if let Some(s) = bat_in.min_freq_mhz {
                bat.min_freq_mhz = Some(s);
            }
            if let Some(s) = bat_in.max_freq_mhz {
                bat.max_freq_mhz = Some(s);
            }
            new_pow.battery = Some(bat);
        }
        merged.power = new_pow;
    }
    if let Some(up) = req.updates {
        let mut new_up = merged.updates.clone();
        new_up.auto_install = up.auto_install;
        merged.updates = new_up;
    }
    if let Some(bat) = req.battery {
        let mut new_bat = merged.battery.clone();
        if let Some(s) = bat.charge_limit_max_pct {
            new_bat.charge_limit_max_pct = Some(s);
        }
        if let Some(s) = bat.charge_rate_c {
            new_bat.charge_rate_c = Some(s);
            new_bat.charge_rate_soc_threshold_pct = bat.charge_rate_soc_threshold_pct;
        }
        if let Some(ms) = bat.poll_ms {
            new_bat.poll_ms = Some(ms.clamp(MIN_BATTERY_POLL_MS, MAX_BATTERY_POLL_MS));
        }
        merged.battery = new_bat;
    }
    if let Some(tel) = req.telemetry {
        merged.telemetry = tel;
    }
    if let Some(ui) = req.ui {
        let mut new_ui = merged.ui.clone();
        if let Some(theme) = ui.theme {
            new_ui.theme = Some(theme);
        }
        if let Some(panels) = ui.panels {
            new_ui.panels = Some(panels);
        }
        if let Some(announcements) = ui.announcements {
            new_ui.announcements = Some(announcements);
        }
        merged.ui = new_ui;
    }
}

fn config_dir() -> PathBuf {
    // Explicit override always wins (all platforms)
    if let Ok(p) = std::env::var("FRAMEWORK_CONTROL_CONFIG") {
        let path = PathBuf::from(p);
        return path
            .parent()
            .map(|d| d.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
    }

    // Windows: prefer ProgramData for system-wide service config
    #[cfg(windows)]
    {
        let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
        return PathBuf::from(base).join("FrameworkControl");
    }

    // Linux: system-wide config
    #[cfg(target_os = "linux")]
    {
        return PathBuf::from("/etc").join("framework-control");
    }

    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        panic!("Unsupported platform: Framework Control currently supports Windows and Linux only");
    }
}

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("FRAMEWORK_CONTROL_CONFIG") {
        return PathBuf::from(p);
    }
    config_dir().join("config.json")
}

pub fn battery_history_path() -> PathBuf {
    config_dir().join("battery-history.json")
}

fn load() -> Config {
    let path = config_path();
    if let Ok(mut f) = File::open(&path) {
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_ok() {
            if let Ok(mut cfg) = serde_json::from_str::<Config>(&buf) {
                info!("Loaded config from {:?}", path);
                cfg.fan.normalize_profiles();
                return cfg;
            }
        }
    }
    let mut cfg = Config::default();
    cfg.fan.normalize_profiles();
    cfg
}

fn save(cfg: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut f = File::create(&path).map_err(|e| e.to_string())?;
    let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    f.write_all(s.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FanControlConfig, FanControlMode, FanProfile, FanProfileAutoSwitch, ManualConfig};

    fn fan_patch(fan: FanControlConfig) -> PartialConfig {
        PartialConfig {
            fan: Some(fan),
            power: None,
            battery: None,
            updates: None,
            telemetry: None,
            ui: None,
        }
    }

    fn profile(id: &str, duty: u32) -> FanProfile {
        FanProfile {
            id: id.into(),
            name: id.into(),
            mode: Some(FanControlMode::Manual),
            manual: Some(ManualConfig { duty_pct: duty }),
            curve: None,
            overrides: None,
        }
    }

    fn duty(cfg: &Config) -> u32 {
        cfg.fan.manual.as_ref().unwrap().duty_pct
    }

    fn profile_duty(cfg: &Config, id: &str) -> u32 {
        let p = cfg.fan.profiles.as_ref().unwrap().iter().find(|p| p.id == id).unwrap();
        p.manual.as_ref().unwrap().duty_pct
    }

    fn two_profiles() -> Config {
        let mut cfg = Config::default();
        cfg.fan.mode = Some(FanControlMode::Manual);
        cfg.fan.manual = Some(ManualConfig { duty_pct: 30 });
        cfg.fan.profiles = Some(vec![profile("home", 30), profile("quiet", 10)]);
        cfg.fan.active_profile = Some("home".into());
        cfg
    }

    #[test]
    fn legacy_config_becomes_default_profile() {
        let mut cfg = Config::default();
        cfg.fan.mode = Some(FanControlMode::Manual);
        cfg.fan.manual = Some(ManualConfig { duty_pct: 42 });
        cfg.fan.normalize_profiles();
        assert_eq!(cfg.fan.active_profile.as_deref(), Some("default"));
        assert_eq!(profile_duty(&cfg, "default"), 42);
    }

    #[test]
    fn edits_are_mirrored_into_active_profile() {
        let mut cfg = two_profiles();
        apply_partial(
            &mut cfg,
            fan_patch(FanControlConfig {
                manual: Some(ManualConfig { duty_pct: 55 }),
                ..Default::default()
            }),
        );
        assert_eq!(profile_duty(&cfg, "home"), 55);
        assert_eq!(profile_duty(&cfg, "quiet"), 10);
    }

    #[test]
    fn switching_loads_profile_and_keeps_calibration() {
        let mut cfg = two_profiles();
        cfg.fan.calibration = Some(crate::types::FanCalibration {
            updated_at: 1,
            fans: vec![],
        });
        apply_partial(
            &mut cfg,
            fan_patch(FanControlConfig {
                active_profile: Some("quiet".into()),
                ..Default::default()
            }),
        );
        assert_eq!(cfg.fan.active_profile.as_deref(), Some("quiet"));
        assert_eq!(duty(&cfg), 10);
        assert!(cfg.fan.calibration.is_some());
        assert_eq!(profile_duty(&cfg, "home"), 30);
    }

    #[test]
    fn stale_profile_list_cannot_overwrite_active_settings() {
        let mut cfg = two_profiles();
        cfg.fan.manual = Some(ManualConfig { duty_pct: 70 });
        cfg.fan.normalize_profiles();
        let mut renamed = vec![profile("home", 30), profile("quiet", 10)];
        renamed[0].name = "Home".into();
        apply_partial(
            &mut cfg,
            fan_patch(FanControlConfig {
                profiles: Some(renamed),
                ..Default::default()
            }),
        );
        assert_eq!(cfg.fan.profiles.as_ref().unwrap()[0].name, "Home");
        assert_eq!(profile_duty(&cfg, "home"), 70);
        assert_eq!(duty(&cfg), 70);
    }

    #[test]
    fn deleting_active_profile_falls_back_and_clears_assignments() {
        let mut cfg = two_profiles();
        cfg.fan.auto_switch = Some(FanProfileAutoSwitch {
            ac: Some("home".into()),
            battery: Some("quiet".into()),
        });
        apply_partial(
            &mut cfg,
            fan_patch(FanControlConfig {
                profiles: Some(vec![profile("quiet", 10)]),
                ..Default::default()
            }),
        );
        assert_eq!(cfg.fan.active_profile.as_deref(), Some("quiet"));
        assert_eq!(duty(&cfg), 10);
        let auto = cfg.fan.auto_switch.as_ref().unwrap();
        assert_eq!(auto.ac, None);
        assert_eq!(auto.battery.as_deref(), Some("quiet"));
    }

    #[test]
    fn unknown_profile_id_is_ignored() {
        let mut cfg = two_profiles();
        apply_partial(
            &mut cfg,
            fan_patch(FanControlConfig {
                active_profile: Some("missing".into()),
                ..Default::default()
            }),
        );
        assert_eq!(cfg.fan.active_profile.as_deref(), Some("home"));
        assert_eq!(duty(&cfg), 30);
    }
}
