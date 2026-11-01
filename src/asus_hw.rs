// SPDX-License-Identifier: GPL-2.0-or-later
// debian_asusctl — Core Hardware, Power Profile, Fan, LED, ScreenPad, Battery, Keymap & Rollback Engine

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const VERSION: &str = "6.2.0-debian3";
pub const DEFAULT_CONFIRM_TIMEOUT_SECS: u64 = 90;

pub fn is_sandbox() -> bool {
    env::var("ASUSCTL_SANDBOX_DIR").is_ok()
}

pub fn root_prefix() -> PathBuf {
    if let Ok(sb) = env::var("ASUSCTL_SANDBOX_DIR") {
        let p = PathBuf::from(sb.trim());
        if p.is_absolute() && p.exists() {
            return p;
        }
    }
    PathBuf::from("/")
}

pub fn sys_path(rel: &str) -> PathBuf {
    let clean = rel.trim_start_matches('/');
    root_prefix().join(clean)
}

pub fn user_fallback_config_dir() -> PathBuf {
    if let Ok(home) = env::var("HOME") {
        PathBuf::from(home).join(".config/asusctl")
    } else {
        PathBuf::from("/tmp/asusctl")
    }
}

pub fn config_dir() -> PathBuf {
    sys_path("/etc/asusctl")
}

pub fn state_file() -> PathBuf {
    let primary = sys_path("/etc/asusctl/asusctl.conf");
    if !is_sandbox() {
        let user_sf = user_fallback_config_dir().join("asusctl.conf");
        if user_sf.exists() && fs::OpenOptions::new().write(true).open(&primary).is_err() {
            return user_sf;
        }
    }
    primary
}

pub fn backup_file() -> PathBuf {
    let primary = sys_path("/etc/asusctl/backup_state.conf");
    if !is_sandbox() {
        let user_bf = user_fallback_config_dir().join("backup_state.conf");
        if !primary.exists() && user_bf.exists() {
            return user_bf;
        }
    }
    primary
}

pub fn keymap_file() -> PathBuf {
    sys_path("/etc/asusctl/keymap.conf")
}

pub fn socket_path() -> PathBuf {
    sys_path("/run/asusctl.sock")
}

pub fn pending_confirm_file() -> PathBuf {
    sys_path("/run/asusctl.pending_confirm")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerProfile {
    Quiet,
    Balanced,
    Performance,
    FullSpeed,
}

impl PowerProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            PowerProfile::Quiet => "Quiet",
            PowerProfile::Balanced => "Balanced",
            PowerProfile::Performance => "Performance",
            PowerProfile::FullSpeed => "FullSpeed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "quiet" | "whisper" | "silent" | "low-power" | "eco" | "0" => Some(PowerProfile::Quiet),
            "balanced" | "standard" | "normal" | "auto" | "1" => Some(PowerProfile::Balanced),
            "performance" | "turbo" | "high" | "2" => Some(PowerProfile::Performance),
            "fullspeed" | "full-speed" | "max" | "boost" | "3" => Some(PowerProfile::FullSpeed),
            _ => None,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            PowerProfile::Quiet => PowerProfile::Balanced,
            PowerProfile::Balanced => PowerProfile::Performance,
            PowerProfile::Performance => PowerProfile::FullSpeed,
            PowerProfile::FullSpeed => PowerProfile::Quiet,
        }
    }

    pub fn next_standard_three(&self) -> Self {
        match self {
            PowerProfile::Quiet => PowerProfile::Balanced,
            PowerProfile::Balanced => PowerProfile::Performance,
            PowerProfile::Performance | PowerProfile::FullSpeed => PowerProfile::Quiet,
        }
    }
}

pub fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path.as_ref())
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn write_sysfs(path: impl AsRef<Path>, val: &str) -> Result<(), String> {
    let p = path.as_ref();
    let clean = val.trim();
    if clean.contains('\0') || clean.contains('\n') {
        return Err("Invalid control characters in sysfs value".to_string());
    }
    fs::write(p, format!("{}\n", clean))
        .or_else(|_| fs::write(p, clean.as_bytes()))
        .map_err(|e| format!("{}: {}", p.display(), e))
}

pub fn is_secure_config_file(path: &Path) -> bool {
    if let Ok(meta) = fs::metadata(path) {
        let mode = meta.mode();
        if (mode & 0o002) != 0 {
            return false;
        }
        if is_sandbox() {
            return true;
        }
        return meta.uid() == 0;
    }
    false
}

pub fn sanitize_text(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() && *c != '\'' && *c != '"' && *c != '`' && *c != '$' && *c != '\\')
        .collect()
}

pub fn find_asus_hwmon_dir() -> Option<PathBuf> {
    let base = sys_path("/sys/devices/platform/asus-nb-wmi/hwmon");
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(sys_path("/sys/class/hwmon")) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(name) = read_trimmed(p.join("name")) {
                if name.contains("asus") {
                    return Some(p);
                }
            }
        }
    }
    None
}

pub fn find_battery_threshold_path() -> Option<PathBuf> {
    for bat in ["BAT0", "BAT1", "BATT", "BATC"] {
        let p = sys_path(&format!(
            "/sys/class/power_supply/{}/charge_control_end_threshold",
            bat
        ));
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn find_kbd_backlight_dir() -> Option<PathBuf> {
    for cand in [
        "/sys/class/leds/asus::kbd_backlight",
        "/sys/devices/platform/asus-nb-wmi/leds/asus::kbd_backlight",
    ] {
        let p = sys_path(cand);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(entries) = fs::read_dir(sys_path("/sys/class/leds")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains("kbd_backlight") {
                return Some(entry.path());
            }
        }
    }
    None
}

pub fn find_screenpad_dir() -> Option<PathBuf> {
    for cand in [
        "/sys/class/backlight/asus_screenpad",
        "/sys/devices/platform/asus-nb-wmi/backlight/asus_screenpad",
    ] {
        let p = sys_path(cand);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn find_asus_wmi_input_event() -> Option<PathBuf> {
    let by_path = sys_path("/dev/input/by-path/platform-asus-nb-wmi-event");
    if by_path.exists() {
        if let Ok(canon) = fs::canonicalize(&by_path) {
            return Some(canon);
        }
        return Some(by_path);
    }
    if let Ok(content) = fs::read_to_string(sys_path("/proc/bus/input/devices")) {
        let mut is_asus_wmi = false;
        for line in content.lines() {
            if line.starts_with("N: Name=") {
                is_asus_wmi = line.contains("Asus WMI hotkeys")
                    || line.contains("ASUS WMI")
                    || line.contains("Asus Keyboard");
            } else if is_asus_wmi && line.starts_with("H: Handlers=") {
                for tok in line.split_whitespace() {
                    if tok.starts_with("event") {
                        let ev_path = sys_path(&format!("/dev/input/{}", tok));
                        if ev_path.exists() {
                            return Some(ev_path);
                        }
                    }
                }
                is_asus_wmi = false;
            }
        }
    }
    None
}

#[derive(Clone, Debug)]
pub struct AsusState {
    pub profile: PowerProfile,
    pub charge_limit: u8,
    pub kbd_brightness: u8,
    pub screenpad_enabled: bool,
    pub screenpad_brightness: u8,
    pub fnlock_enabled: bool,
    pub confirmed: bool,
}

impl Default for AsusState {
    fn default() -> Self {
        Self {
            profile: PowerProfile::Balanced,
            charge_limit: 100,
            kbd_brightness: 2,
            screenpad_enabled: true,
            screenpad_brightness: 180,
            fnlock_enabled: true,
            confirmed: true,
        }
    }
}

impl AsusState {
    pub fn load() -> Self {
        let mut st = Self::default();
        st.profile = detect_current_profile();
        if let Some(lim) = get_battery_limit() {
            st.charge_limit = lim;
        }
        if let Some((lvl, _)) = get_kbd_backlight() {
            st.kbd_brightness = lvl;
        }
        if let Some((en, br, _)) = get_screenpad_status() {
            st.screenpad_enabled = en;
            if br > 0 {
                st.screenpad_brightness = br;
            }
        }
        if let Some(fnl) = get_fnlock() {
            st.fnlock_enabled = fnl;
        }

        let sf = state_file();
        if let Ok(raw) = fs::read_to_string(&sf) {
            for line in raw.lines() {
                let ln = line.trim();
                if ln.is_empty() || ln.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = ln.split_once('=') {
                    let key = k.trim();
                    let val = v.trim();
                    match key {
                        "profile" => {
                            if let Some(p) = PowerProfile::from_str(val) {
                                st.profile = p;
                            }
                        }
                        "charge_limit" => {
                            if let Ok(n) = val.parse::<u8>() {
                                st.charge_limit = n.clamp(20, 100);
                            }
                        }
                        "kbd_brightness" => {
                            if let Ok(n) = val.parse::<u8>() {
                                st.kbd_brightness = n.clamp(0, 3);
                            }
                        }
                        "screenpad_enabled" => {
                            st.screenpad_enabled = matches!(val, "true" | "1" | "yes" | "on");
                        }
                        "screenpad_brightness" => {
                            if let Ok(n) = val.parse::<u8>() {
                                st.screenpad_brightness = n.clamp(1, 255);
                            }
                        }
                        "fnlock_enabled" => {
                            st.fnlock_enabled = matches!(val, "true" | "1" | "yes" | "on" | "Y");
                        }
                        "confirmed" => {
                            st.confirmed = matches!(val, "true" | "1" | "yes" | "on" | "Y");
                        }
                        _ => {}
                    }
                }
            }
        }
        st
    }

    pub fn save(&self) -> Result<(), String> {
        let dir = config_dir();
        let _ = fs::create_dir_all(&dir);
        let sf = state_file();
        let body = format!(
            "# /etc/asusctl/asusctl.conf — Persisted ASUS Hardware & Power State (debian_asusctl)\n\
             profile={}\n\
             charge_limit={}\n\
             kbd_brightness={}\n\
             screenpad_enabled={}\n\
             screenpad_brightness={}\n\
             fnlock_enabled={}\n\
             confirmed={}\n",
            self.profile.as_str(),
            self.charge_limit,
            self.kbd_brightness,
            self.screenpad_enabled,
            self.screenpad_brightness,
            self.fnlock_enabled,
            self.confirmed
        );
        if fs::write(&sf, &body).is_ok() {
            return Ok(());
        }
        if !is_sandbox() {
            let udir = user_fallback_config_dir();
            let _ = fs::create_dir_all(&udir);
            let usf = udir.join("asusctl.conf");
            return fs::write(&usf, body)
                .map_err(|e| format!("Failed writing {}: {}", usf.display(), e));
        }
        Err(format!("Failed writing {}", sf.display()))
    }
}

/// Captures a complete snapshot of current kernel/driver sysfs settings so the system
/// can automatically revert if unconfirmed or if a daemon crash occurs.
pub fn capture_backup_state(overwrite: bool) -> Result<PathBuf, String> {
    let bf = backup_file();
    if bf.exists() && !overwrite {
        return Ok(bf);
    }
    let _ = fs::create_dir_all(config_dir());

    let cpufv = read_trimmed(sys_path("/sys/devices/platform/asus-nb-wmi/cpufv")).unwrap_or_else(|| "0".to_string());
    let ttp = read_trimmed(sys_path("/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy")).unwrap_or_default();
    let acpi_pp = read_trimmed(sys_path("/sys/firmware/acpi/platform_profile")).unwrap_or_default();
    let pwm1 = find_asus_hwmon_dir()
        .and_then(|d| read_trimmed(d.join("pwm1_enable")))
        .unwrap_or_else(|| "2".to_string());
    let no_turbo = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/no_turbo")).unwrap_or_else(|| "0".to_string());
    let dyn_boost = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/hwp_dynamic_boost")).unwrap_or_else(|| "1".to_string());
    let min_perf = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/min_perf_pct")).unwrap_or_else(|| "15".to_string());
    let max_perf = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/max_perf_pct")).unwrap_or_else(|| "100".to_string());
    let gov = read_trimmed(sys_path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")).unwrap_or_else(|| "powersave".to_string());
    let epp = read_trimmed(sys_path("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")).unwrap_or_else(|| "balance_performance".to_string());
    let pl1 = read_trimmed(sys_path("/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw")).unwrap_or_else(|| "45000000".to_string());
    let pl2 = read_trimmed(sys_path("/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw")).unwrap_or_else(|| "90000000".to_string());
    let (sp_en, sp_br, _) = get_screenpad_status().unwrap_or((true, 180, 255));
    let (main_br, _) = get_main_display_brightness().unwrap_or((264, 528));
    let (kbd_br, _) = get_kbd_backlight().unwrap_or((2, 3));
    let bat_lim = get_battery_limit().unwrap_or(100);
    let fnlock = get_fnlock().unwrap_or(true);

    let content = format!(
        "# /etc/asusctl/backup_state.conf — Pre-Install / Last-Known-Good System Driver State\n\
         cpufv={}\n\
         throttle_thermal_policy={}\n\
         acpi_platform_profile={}\n\
         pwm1_enable={}\n\
         no_turbo={}\n\
         hwp_dynamic_boost={}\n\
         min_perf_pct={}\n\
         max_perf_pct={}\n\
         scaling_governor={}\n\
         energy_performance_preference={}\n\
         rapl_pl1_uw={}\n\
         rapl_pl2_uw={}\n\
         screenpad_bl_power={}\n\
         screenpad_brightness={}\n\
         intel_backlight={}\n\
         kbd_brightness={}\n\
         charge_limit={}\n\
         fnlock={}\n",
        cpufv,
        ttp,
        acpi_pp,
        pwm1,
        no_turbo,
        dyn_boost,
        min_perf,
        max_perf,
        gov,
        epp,
        pl1,
        pl2,
        if sp_en { 1 } else { 0 },
        sp_br.max(120),
        main_br,
        kbd_br,
        bat_lim,
        if fnlock { "Y" } else { "N" }
    );
    if fs::write(&bf, &content).is_ok() {
        return Ok(bf);
    }
    if !is_sandbox() {
        let udir = user_fallback_config_dir();
        let _ = fs::create_dir_all(&udir);
        let ubf = udir.join("backup_state.conf");
        fs::write(&ubf, content)
            .map_err(|e| format!("Failed writing backup {}: {}", ubf.display(), e))?;
        return Ok(ubf);
    }
    Err(format!("Failed writing backup {}", bf.display()))
}

/// Restores all kernel sysfs driver parameters, ScreenPad Plus power/brightness, primary backlight,
/// and previous driver binaries (if backed up) when reverting or recovering from a crash/timeout.
pub fn revert_to_backup_state() -> Result<Vec<String>, String> {
    let bf = backup_file();
    let mut kv = BTreeMap::new();
    if let Ok(raw) = fs::read_to_string(&bf) {
        for line in raw.lines() {
            let ln = line.trim();
            if ln.is_empty() || ln.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = ln.split_once('=') {
                kv.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }

    let mut restored = Vec::new();

    // 1. Always ensure ScreenPad Plus (DP-3) is powered ON and visible so second screen never stays black
    if let Some(sp_dir) = find_screenpad_dir() {
        let sp_power = kv.get("screenpad_bl_power").map(|s| s.as_str()).unwrap_or("1");
        let sp_br = kv.get("screenpad_brightness").map(|s| s.as_str()).unwrap_or("180");
        let safe_power = if sp_power == "0" { "1" } else { sp_power };
        let _ = write_sysfs(sp_dir.join("brightness"), sp_br);
        if write_sysfs(sp_dir.join("bl_power"), safe_power).is_ok() {
            let _ = write_sysfs(sp_dir.join("brightness"), sp_br);
            sync_screenpad_xrandr(true);
            restored.push(format!("screenpad(bl_power={}, brightness={})", safe_power, sp_br));
        }
    }

    // 2. Restore primary display backlight (intel_backlight)
    if let Some(ibr) = kv.get("intel_backlight") {
        let ib_path = sys_path("/sys/class/backlight/intel_backlight/brightness");
        if ib_path.exists() {
            if write_sysfs(&ib_path, ibr).is_ok() {
                restored.push(format!("intel_backlight={}", ibr));
            } else if let Ok(v) = ibr.parse::<u32>() {
                restore_main_display_brightness(v);
                restored.push(format!("intel_backlight={}", v));
            }
        }
    }

    // 3. Restore ASUS WMI / ATK thermal & fan policy to safe automatic kernel defaults
    let cpufv_p = sys_path("/sys/devices/platform/asus-nb-wmi/cpufv");
    if cpufv_p.exists() {
        let v = kv.get("cpufv").map(|s| s.as_str()).unwrap_or("0");
        if write_sysfs(&cpufv_p, v).is_ok() {
            restored.push(format!("cpufv={}", v));
        }
    }
    if let Some(hwmon) = find_asus_hwmon_dir() {
        let pwm1 = hwmon.join("pwm1_enable");
        if pwm1.exists() {
            let v = kv.get("pwm1_enable").map(|s| s.as_str()).unwrap_or("2");
            if write_sysfs(&pwm1, v).is_ok() {
                restored.push(format!("pwm1_enable={}", v));
            }
        }
    }

    // 4. Restore Intel P-State & CPUFreq governors
    let pstate = sys_path("/sys/devices/system/cpu/intel_pstate");
    if pstate.exists() {
        let nt = kv.get("no_turbo").map(|s| s.as_str()).unwrap_or("0");
        let db = kv.get("hwp_dynamic_boost").map(|s| s.as_str()).unwrap_or("1");
        let minp = kv.get("min_perf_pct").map(|s| s.as_str()).unwrap_or("15");
        let maxp = kv.get("max_perf_pct").map(|s| s.as_str()).unwrap_or("100");
        let _ = write_sysfs(pstate.join("no_turbo"), nt);
        let _ = write_sysfs(pstate.join("hwp_dynamic_boost"), db);
        let _ = write_sysfs(pstate.join("min_perf_pct"), minp);
        if write_sysfs(pstate.join("max_perf_pct"), maxp).is_ok() {
            restored.push(format!("intel_pstate(no_turbo={}, max_perf={}%)", nt, maxp));
        }
    }

    let gov = kv.get("scaling_governor").map(|s| s.as_str()).unwrap_or("powersave");
    let epp = kv.get("energy_performance_preference").map(|s| s.as_str()).unwrap_or("balance_performance");
    if let Ok(entries) = fs::read_dir(sys_path("/sys/devices/system/cpu")) {
        let mut cnt = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit()) && !name[3..].is_empty() {
                let cpufreq = entry.path().join("cpufreq");
                if cpufreq.exists() {
                    let _ = write_sysfs(cpufreq.join("scaling_governor"), gov);
                    if write_sysfs(cpufreq.join("energy_performance_preference"), epp).is_ok() {
                        cnt += 1;
                    }
                }
            }
        }
        if cnt > 0 {
            restored.push(format!("cpufreq({}/{})", gov, epp));
        }
    }

    // 5. Restore RAPL power limits
    let rapl = sys_path("/sys/class/powercap/intel-rapl:0");
    if rapl.exists() {
        let pl1 = kv.get("rapl_pl1_uw").map(|s| s.as_str()).unwrap_or("45000000");
        let pl2 = kv.get("rapl_pl2_uw").map(|s| s.as_str()).unwrap_or("90000000");
        let _ = write_sysfs(rapl.join("constraint_0_power_limit_uw"), pl1);
        let _ = write_sysfs(rapl.join("constraint_1_power_limit_uw"), pl2);
        restored.push("rapl_power_limits".to_string());
    }

    // 6. Restore keyboard backlight, battery limit, and fnlock
    if let Some(kbr) = kv.get("kbd_brightness") {
        let _ = set_kbd_backlight(kbr);
    }
    if let Some(blim) = kv.get("charge_limit").and_then(|s| s.parse::<u8>().ok()) {
        let _ = set_battery_limit(blim);
    }
    if let Some(fnl) = kv.get("fnlock") {
        let _ = set_fnlock(fnl);
    }

    // 7. If a previous /usr/bin/asusctl binary was backed up before install, restore it on full package rollback
    let prev_bin = sys_path("/var/lib/asusctl/previous_driver_backup/asusctl.orig");
    if prev_bin.exists() {
        let target_bin = sys_path("/usr/bin/asusctl");
        if fs::copy(&prev_bin, &target_bin).is_ok() {
            restored.push("restored_previous_asusctl_binary".to_string());
        }
    }

    let _ = fs::remove_file(pending_confirm_file());
    Ok(restored)
}

pub fn confirm_driver_state() -> Result<String, String> {
    let mut st = AsusState::load();
    st.confirmed = true;
    st.save()?;
    let _ = fs::remove_file(pending_confirm_file());
    let _ = capture_backup_state(true);
    Ok("Driver state confirmed and saved as permanent configuration.".to_string())
}

pub fn is_driver_confirmed() -> bool {
    if pending_confirm_file().exists() {
        return false;
    }
    AsusState::load().confirmed
}

pub fn detect_current_profile() -> PowerProfile {
    if let Some(hwmon) = find_asus_hwmon_dir() {
        if let Some(pwm) = read_trimmed(hwmon.join("pwm1_enable")) {
            if pwm == "0" {
                return PowerProfile::FullSpeed;
            }
        }
    }
    if let Some(pp) = read_trimmed(sys_path("/sys/firmware/acpi/platform_profile")) {
        if let Some(p) = PowerProfile::from_str(&pp) {
            return p;
        }
    }
    if let Some(ttp) = read_trimmed(sys_path("/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy")) {
        return match ttp.as_str() {
            "2" => PowerProfile::Quiet,
            "1" => PowerProfile::Performance,
            _ => PowerProfile::Balanced,
        };
    }
    if let Some(no_turbo) = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/no_turbo")) {
        if no_turbo == "1" {
            return PowerProfile::Quiet;
        }
    }
    if let Some(gov) = read_trimmed(sys_path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")) {
        if gov == "performance" {
            return PowerProfile::Performance;
        }
    }
    if let Some(epp) = read_trimmed(sys_path("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")) {
        if epp == "power" {
            return PowerProfile::Quiet;
        } else if epp == "performance" {
            return PowerProfile::Performance;
        }
    }
    PowerProfile::Balanced
}

pub fn apply_power_profile(profile: PowerProfile) -> Result<Vec<String>, String> {
    let _ = capture_backup_state(false);
    let mut actions: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // 1. ACPI platform_profile
    let acpi_pp = sys_path("/sys/firmware/acpi/platform_profile");
    if acpi_pp.exists() {
        let val = match profile {
            PowerProfile::Quiet => "quiet",
            PowerProfile::Balanced => "balanced",
            PowerProfile::Performance | PowerProfile::FullSpeed => "performance",
        };
        if write_sysfs(&acpi_pp, val).is_ok() {
            actions.push(format!("acpi_platform_profile={}", val));
        }
    }

    // 2. ASUS WMI throttle_thermal_policy
    let ttp = sys_path("/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy");
    if ttp.exists() {
        let val = match profile {
            PowerProfile::Quiet => "2",
            PowerProfile::Balanced => "0",
            PowerProfile::Performance | PowerProfile::FullSpeed => "1",
        };
        if write_sysfs(&ttp, val).is_ok() {
            actions.push(format!("throttle_thermal_policy={}", val));
        }
    }

    // 3. ASUS ATK cpufv
    let cpufv = sys_path("/sys/devices/platform/asus-nb-wmi/cpufv");
    if cpufv.exists() {
        let val = match profile {
            PowerProfile::Quiet => "2",
            PowerProfile::Balanced => "0",
            PowerProfile::Performance | PowerProfile::FullSpeed => "1",
        };
        match write_sysfs(&cpufv, val) {
            Ok(_) => actions.push(format!("asus_nb_wmi.cpufv={}", val)),
            Err(e) => errors.push(e),
        }
    }

    // 4. ASUS hwmon Fan PWM Control (0=FullSpeed Max Fans, 2=Auto Firmware Curve)
    if let Some(hwmon) = find_asus_hwmon_dir() {
        let pwm1 = hwmon.join("pwm1_enable");
        if pwm1.exists() {
            let pwm_val = match profile {
                PowerProfile::FullSpeed => "0",
                _ => "2",
            };
            match write_sysfs(&pwm1, pwm_val) {
                Ok(_) => actions.push(format!(
                    "fan_pwm1_enable={} ({})",
                    pwm_val,
                    if pwm_val == "0" { "Max FullSpeed" } else { "Auto Curve" }
                )),
                Err(e) => errors.push(e),
            }
        }
    }

    // 5. Intel P-State Turbo, Dynamic Boost & Perf Pct
    let pstate_dir = sys_path("/sys/devices/system/cpu/intel_pstate");
    if pstate_dir.exists() {
        let (no_turbo, dyn_boost, min_pct, max_pct) = match profile {
            PowerProfile::Quiet => ("1", "0", "15", "75"),
            PowerProfile::Balanced => ("0", "1", "15", "100"),
            PowerProfile::Performance | PowerProfile::FullSpeed => ("0", "1", "25", "100"),
        };
        if write_sysfs(pstate_dir.join("no_turbo"), no_turbo).is_ok() {
            actions.push(format!(
                "intel_pstate.turbo={}",
                if no_turbo == "0" { "ON" } else { "OFF" }
            ));
        }
        let _ = write_sysfs(pstate_dir.join("hwp_dynamic_boost"), dyn_boost);
        let _ = write_sysfs(pstate_dir.join("min_perf_pct"), min_pct);
        if write_sysfs(pstate_dir.join("max_perf_pct"), max_pct).is_ok() {
            actions.push(format!("intel_pstate.max_perf_pct={}%", max_pct));
        }
    }

    // 6. Per-CPU scaling_governor & energy_performance_preference (EPP)
    let (gov, epp) = match profile {
        PowerProfile::Quiet => ("powersave", "power"),
        PowerProfile::Balanced => ("powersave", "balance_performance"),
        PowerProfile::Performance | PowerProfile::FullSpeed => ("performance", "performance"),
    };
    let mut gov_cnt = 0usize;
    let mut epp_cnt = 0usize;
    if let Ok(entries) = fs::read_dir(sys_path("/sys/devices/system/cpu")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("cpu")
                && name[3..].chars().all(|c| c.is_ascii_digit())
                && !name[3..].is_empty()
            {
                let cpufreq = entry.path().join("cpufreq");
                if cpufreq.exists() {
                    if write_sysfs(cpufreq.join("scaling_governor"), gov).is_ok() {
                        gov_cnt += 1;
                    }
                    if write_sysfs(cpufreq.join("energy_performance_preference"), epp).is_ok() {
                        epp_cnt += 1;
                    }
                }
            }
        }
    }
    if gov_cnt > 0 {
        actions.push(format!("cpu_governor={} ({} cores)", gov, gov_cnt));
    }
    if epp_cnt > 0 {
        actions.push(format!("cpu_epp={} ({} cores)", epp, epp_cnt));
    }

    // 7. Intel RAPL Package Power Limits (PL1 & PL2 in microwatts)
    let rapl_dir = sys_path("/sys/class/powercap/intel-rapl:0");
    if rapl_dir.exists() {
        let (pl1_uw, pl2_uw) = match profile {
            PowerProfile::Quiet => ("28000000", "45000000"),
            PowerProfile::Balanced => ("45000000", "90000000"),
            PowerProfile::Performance | PowerProfile::FullSpeed => ("70000000", "135000000"),
        };
        let c0 = rapl_dir.join("constraint_0_power_limit_uw");
        let c1 = rapl_dir.join("constraint_1_power_limit_uw");
        if c0.exists() && write_sysfs(&c0, pl1_uw).is_ok() {
            let _ = write_sysfs(&c1, pl2_uw);
            actions.push(format!(
                "rapl_power_limits=PL1:{}W/PL2:{}W",
                pl1_uw.parse::<u64>().unwrap_or(0) / 1_000_000,
                pl2_uw.parse::<u64>().unwrap_or(0) / 1_000_000
            ));
        }
    }

    if actions.is_empty() && !errors.is_empty() {
        return Err(format!(
            "Permission denied or failed writing sysfs nodes ({}). Run with sudo or ensure asusctld.service is active.",
            errors.join("; ")
        ));
    }

    let mut st = AsusState::load();
    st.profile = profile;
    let _ = st.save();

    Ok(actions)
}

pub fn get_fan_telemetry() -> Vec<(String, u32)> {
    let mut fans = Vec::new();
    if let Some(hwmon) = find_asus_hwmon_dir() {
        for idx in 1..=4 {
            let inp = hwmon.join(format!("fan{}_input", idx));
            if let Some(val_str) = read_trimmed(&inp) {
                if let Ok(rpm) = val_str.parse::<u32>() {
                    let lbl = read_trimmed(hwmon.join(format!("fan{}_label", idx)))
                        .unwrap_or_else(|| format!("fan{}", idx));
                    fans.push((lbl, rpm));
                }
            }
        }
    }
    fans
}

pub fn get_fan_mode() -> String {
    if let Some(hwmon) = find_asus_hwmon_dir() {
        if let Some(pwm) = read_trimmed(hwmon.join("pwm1_enable")) {
            return match pwm.as_str() {
                "0" => "0 (FullSpeed / Maximum Boost)".to_string(),
                "1" => "1 (Manual PWM)".to_string(),
                "2" => "2 (Automatic Thermal Curve)".to_string(),
                other => other.to_string(),
            };
        }
    }
    "Auto".to_string()
}

pub fn set_fan_mode(mode: &str) -> Result<String, String> {
    let hwmon = find_asus_hwmon_dir().ok_or_else(|| "ASUS hwmon interface not found".to_string())?;
    let pwm1 = hwmon.join("pwm1_enable");
    let val = match mode.trim().to_ascii_lowercase().as_str() {
        "fullspeed" | "full" | "max" | "boost" | "0" => "0",
        "manual" | "1" => "1",
        "auto" | "standard" | "quiet" | "2" => "2",
        other => return Err(format!("Unknown fan mode '{}' (use: auto, fullspeed, manual)", other)),
    };
    write_sysfs(&pwm1, val)?;
    Ok(get_fan_mode())
}

pub fn get_kbd_backlight() -> Option<(u8, u8)> {
    let dir = find_kbd_backlight_dir()?;
    let cur = read_trimmed(dir.join("brightness"))?.parse::<u32>().ok()?;
    let max = read_trimmed(dir.join("max_brightness"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(3);
    Some(((cur & 0xff) as u8, max as u8))
}

pub fn set_kbd_backlight(level_str: &str) -> Result<u8, String> {
    let dir = find_kbd_backlight_dir()
        .ok_or_else(|| "ASUS keyboard backlight LED interface not found".to_string())?;
    let (cur, max) = get_kbd_backlight().unwrap_or((0, 3));
    let target: u8 = match level_str.trim().to_ascii_lowercase().as_str() {
        "off" | "0" => 0,
        "low" | "min" | "1" => 1,
        "med" | "medium" | "mid" | "2" => 2,
        "high" | "max" | "3" => max,
        "next" | "up" | "+1" => {
            if cur >= max { 0 } else { cur + 1 }
        }
        "prev" | "down" | "-1" => {
            if cur == 0 { max } else { cur - 1 }
        }
        other => other
            .parse::<u8>()
            .map_err(|_| format!("Invalid keyboard brightness '{}' (use: off, low, med, high, 0..3)", other))?
            .min(max),
    };
    write_sysfs(dir.join("brightness"), &target.to_string())?;
    let mut st = AsusState::load();
    st.kbd_brightness = target;
    let _ = st.save();
    Ok(target)
}

pub fn get_screenpad_status() -> Option<(bool, u8, u8)> {
    let dir = find_screenpad_dir()?;
    let raw_br = read_trimmed(dir.join("actual_brightness"))
        .or_else(|| read_trimmed(dir.join("brightness")))?
        .parse::<u32>()
        .ok()?;
    let max_br = read_trimmed(dir.join("max_brightness"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(255) as u8;
    let bl_power = read_trimmed(dir.join("bl_power"))
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(1);
    // In Linux 7.x asus-wmi.ko (update_screenpad_bl_status):
    //   bl_power == 0 calls DEVS(0x00050031, 0) -> ScreenPad POWER OFF (DP-3 disconnected)
    //   bl_power == 1 calls DEVS(0x00050031, 1) -> ScreenPad POWER ON  (DP-3 connected)
    let clean_br = (raw_br & 0xff) as u8;
    let enabled = bl_power != 0 && clean_br > 0;
    Some((enabled, clean_br, max_br))
}

pub fn sync_screenpad_xrandr(enable: bool) {
    if env::var("ASUSCTL_SANDBOX_DIR").is_ok() {
        return;
    }
    let script = if enable {
        "sleep 1.2; \
         if ! xrandr --listmonitors 2>/dev/null | grep -q 'DP-3'; then \
             edp_geom=$(xrandr --listmonitors 2>/dev/null | awk '/eDP-1/{print $3}'); \
             edp_h=$(echo \"$edp_geom\" | awk -F'[x/+]' '{print $3}'); \
             edp_h=${edp_h:-1440}; \
             xrandr --output DP-3 --mode 3840x1100 --scale 1x1 --pos 0x${edp_h} 2>/dev/null || xrandr --output DP-3 --auto --below eDP-1 --scale 1x1 2>/dev/null; \
         fi"
    } else {
        "sleep 0.3; \
         if xrandr --listmonitors 2>/dev/null | grep -q 'DP-3'; then \
             xrandr --output DP-3 --off 2>/dev/null; \
         fi"
    };
    run_as_desktop_user(script);
}

pub fn get_main_display_brightness() -> Option<(u32, u32)> {
    let dir = sys_path("/sys/class/backlight/intel_backlight");
    if !dir.exists() {
        return None;
    }
    let cur = read_trimmed(dir.join("brightness"))?.parse::<u32>().ok()?;
    let max = read_trimmed(dir.join("max_brightness"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(528);
    Some((cur, max))
}

pub fn restore_main_display_brightness(saved_br: u32) {
    if env::var("ASUSCTL_SANDBOX_DIR").is_ok() {
        let ib = sys_path("/sys/class/backlight/intel_backlight/brightness");
        let _ = write_sysfs(ib, &saved_br.to_string());
        return;
    }
    let script = format!(
        "for delay in 0.1 0.6; do \
            sleep $delay; \
            cur=$(cat /sys/class/backlight/intel_backlight/brightness 2>/dev/null || echo {}); \
            if [ \"$cur\" != \"{}\" ]; then \
                echo {} > /sys/class/backlight/intel_backlight/brightness 2>/dev/null || \
                gdbus call --system --dest org.freedesktop.login1 --object-path /org/freedesktop/login1/session/auto --method org.freedesktop.login1.Session.SetBrightness backlight intel_backlight {} >/dev/null 2>&1 || true; \
            fi; \
         done",
        saved_br, saved_br, saved_br, saved_br
    );
    let _ = Command::new("/bin/sh").arg("-c").arg(&script).spawn();
}

pub fn set_screenpad(action: &str, arg: Option<&str>) -> Result<(bool, u8), String> {
    let dir = find_screenpad_dir().ok_or_else(|| {
        "ASUS ScreenPad backlight (/sys/class/backlight/asus_screenpad) not found".to_string()
    })?;
    let mut st = AsusState::load();
    let (cur_en, cur_br, max_br) =
        get_screenpad_status().unwrap_or((true, st.screenpad_brightness, 255));
    let saved_main_br = get_main_display_brightness().map(|(c, _)| c);

    match action.trim().to_ascii_lowercase().as_str() {
        "on" | "enable" => {
            let br = if cur_br < 15 {
                st.screenpad_brightness.max(180)
            } else {
                cur_br
            };
            let _ = write_sysfs(dir.join("brightness"), &br.to_string());
            write_sysfs(dir.join("bl_power"), "1")?;
            write_sysfs(dir.join("brightness"), &br.to_string())?;
            sync_screenpad_xrandr(true);
            st.screenpad_enabled = true;
            st.screenpad_brightness = br;
        }
        "off" | "disable" => {
            if cur_br >= 15 {
                st.screenpad_brightness = cur_br;
            }
            write_sysfs(dir.join("bl_power"), "0")?;
            sync_screenpad_xrandr(false);
            st.screenpad_enabled = false;
        }
        "toggle" => {
            if cur_en {
                return set_screenpad("off", None);
            } else {
                return set_screenpad("on", None);
            }
        }
        "up" | "step-up" => {
            let next_br = if !cur_en {
                st.screenpad_brightness.max(60)
            } else {
                cur_br.saturating_add(25).min(max_br).max(25)
            };
            let _ = write_sysfs(dir.join("brightness"), &next_br.to_string());
            write_sysfs(dir.join("bl_power"), "1")?;
            write_sysfs(dir.join("brightness"), &next_br.to_string())?;
            if !cur_en {
                sync_screenpad_xrandr(true);
            }
            st.screenpad_enabled = true;
            st.screenpad_brightness = next_br;
        }
        "down" | "step-down" => {
            let next_br = cur_br.saturating_sub(25);
            if next_br < 10 {
                return set_screenpad("off", None);
            }
            write_sysfs(dir.join("bl_power"), "1")?;
            write_sysfs(dir.join("brightness"), &next_br.to_string())?;
            st.screenpad_enabled = true;
            st.screenpad_brightness = next_br;
        }
        "brightness" | "set" => {
            let val_s = arg.ok_or_else(|| "Provide brightness value 0..255".to_string())?;
            let val: u8 = val_s
                .parse::<u16>()
                .map_err(|_| format!("Invalid ScreenPad brightness '{}'", val_s))?
                .min(max_br as u16) as u8;
            if val == 0 {
                return set_screenpad("off", None);
            }
            let _ = write_sysfs(dir.join("brightness"), &val.to_string());
            write_sysfs(dir.join("bl_power"), "1")?;
            write_sysfs(dir.join("brightness"), &val.to_string())?;
            if !cur_en {
                sync_screenpad_xrandr(true);
            }
            st.screenpad_enabled = true;
            st.screenpad_brightness = val;
        }
        other => {
            if let Ok(val) = other.parse::<u16>() {
                return set_screenpad("brightness", Some(&val.min(255).to_string()));
            }
            return Err(format!(
                "Unknown screenpad command '{}' (use: on, off, toggle, up, down, brightness <0..255>)",
                other
            ));
        }
    }
    if let Some(main_br) = saved_main_br {
        restore_main_display_brightness(main_br);
    }
    let _ = st.save();
    Ok((
        st.screenpad_enabled,
        if st.screenpad_enabled {
            st.screenpad_brightness
        } else {
            0
        },
    ))
}

pub fn get_battery_limit() -> Option<u8> {
    let p = find_battery_threshold_path()?;
    read_trimmed(p)?.parse::<u8>().ok()
}

pub fn set_battery_limit(limit: u8) -> Result<u8, String> {
    if !(20..=100).contains(&limit) {
        return Err(format!(
            "Battery charge limit must be between 20 and 100 (got {})",
            limit
        ));
    }
    let p = find_battery_threshold_path()
        .ok_or_else(|| "Battery charge_control_end_threshold sysfs path not found".to_string())?;
    write_sysfs(p, &limit.to_string())?;
    let mut st = AsusState::load();
    st.charge_limit = limit;
    let _ = st.save();
    Ok(limit)
}

pub fn get_fnlock() -> Option<bool> {
    let p = sys_path("/sys/module/asus_wmi/parameters/fnlock_default");
    let v = read_trimmed(p)?;
    Some(matches!(v.as_str(), "Y" | "1" | "true"))
}

pub fn set_fnlock(mode: &str) -> Result<bool, String> {
    let p = sys_path("/sys/module/asus_wmi/parameters/fnlock_default");
    if !p.exists() {
        return Err("/sys/module/asus_wmi/parameters/fnlock_default not found".to_string());
    }
    let cur = get_fnlock().unwrap_or(true);
    let target = match mode.trim().to_ascii_lowercase().as_str() {
        "on" | "1" | "y" | "yes" | "true" => true,
        "off" | "0" | "n" | "no" | "false" => false,
        "toggle" => !cur,
        other => return Err(format!("Invalid fnlock mode '{}' (use: on, off, toggle)", other)),
    };
    write_sysfs(p, if target { "Y" } else { "N" })?;
    let mut st = AsusState::load();
    st.fnlock_enabled = target;
    let _ = st.save();
    Ok(target)
}

#[derive(Clone, Debug)]
pub struct KeyBinding {
    pub button_name: String,
    pub keycodes: Vec<u16>,
    pub scancodes: Vec<u32>,
    pub description: String,
    pub action: String,
}

pub fn default_key_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding {
            button_name: "fan_performance".to_string(),
            keycodes: vec![203, 482, 470, 169],
            scancodes: vec![0x9d, 0xb3, 0x99, 0xae, 0x6c],
            description: "ASUS Fan / Performance Mode Toggle Key (Above Touchpad / Fn+F)".to_string(),
            action: "profile-next".to_string(),
        },
        KeyBinding {
            button_name: "task_swap".to_string(),
            keycodes: vec![202],
            scancodes: vec![0x38, 0xffff01, 0x8a],
            description: "ASUS Task Swap Key (Swap Active Window Between Main OLED & ScreenPad Plus)".to_string(),
            action: "swap-monitors".to_string(),
        },
        KeyBinding {
            button_name: "screenpad_toggle".to_string(),
            keycodes: vec![431, 149, 183],
            scancodes: vec![0x6a, 0x2d, 0x37],
            description: "ASUS ScreenPad Plus On/Off Toggle Key (Above Touchpad)".to_string(),
            action: "screenpad-toggle".to_string(),
        },
        KeyBinding {
            button_name: "myasus_hub".to_string(),
            keycodes: vec![148],
            scancodes: vec![0x86, 0x39],
            description: "MyASUS / ProArt / ROG Custom Launcher Key (Fn+F12)".to_string(),
            action: "status-notify".to_string(),
        },
        KeyBinding {
            button_name: "kbd_backlight_up".to_string(),
            keycodes: vec![230, 185],
            scancodes: vec![0xc4],
            description: "Keyboard Backlight Brightness Up (Fn+F8)".to_string(),
            action: "kbd-up".to_string(),
        },
        KeyBinding {
            button_name: "kbd_backlight_down".to_string(),
            keycodes: vec![229, 184],
            scancodes: vec![0xc5],
            description: "Keyboard Backlight Brightness Down (Fn+F7)".to_string(),
            action: "kbd-down".to_string(),
        },
        KeyBinding {
            button_name: "touchpad_toggle".to_string(),
            keycodes: vec![530, 531],
            scancodes: vec![0x6b],
            description: "ASUS Touchpad Enable/Disable Key (Fn+F6)".to_string(),
            action: "touchpad-toggle".to_string(),
        },
        KeyBinding {
            button_name: "mic_mute".to_string(),
            keycodes: vec![248],
            scancodes: vec![0x7c],
            description: "ASUS Microphone Mute Toggle Key (Fn+F9)".to_string(),
            action: "mic-mute".to_string(),
        },
        KeyBinding {
            button_name: "camera_toggle".to_string(),
            keycodes: vec![212],
            scancodes: vec![0x82, 0x85],
            description: "ASUS Webcam Shield / Camera Toggle Key (Fn+F10)".to_string(),
            action: "camera-notify".to_string(),
        },
        KeyBinding {
            button_name: "selective_screenshot".to_string(),
            keycodes: vec![634],
            scancodes: vec![0x2a, 0x6e],
            description: "ASUS Screen Snipping / Screenshot Key (Fn+F11)".to_string(),
            action: "screenshot".to_string(),
        },
    ]
}

pub fn is_valid_action(act: &str) -> bool {
    let trimmed = act.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.contains('\n') || trimmed.contains('\0') {
        return false;
    }
    if let Some(cmd) = trimmed.strip_prefix("exec:") {
        return !cmd.trim().is_empty();
    }
    matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "profile-next"
            | "profile-standard"
            | "swap-monitors"
            | "screenpad-toggle"
            | "screenpad-up"
            | "screenpad-down"
            | "kbd-up"
            | "kbd-down"
            | "touchpad-toggle"
            | "mic-mute"
            | "screenshot"
            | "camera-notify"
            | "status-notify"
    )
}

pub fn load_key_bindings() -> Vec<KeyBinding> {
    let mut bindings = default_key_bindings();
    let kf = keymap_file();
    if kf.exists() && is_secure_config_file(&kf) {
        if let Ok(raw) = fs::read_to_string(&kf) {
            let mut overrides = BTreeMap::new();
            for line in raw.lines() {
                let ln = line.trim();
                if ln.is_empty() || ln.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = ln.split_once('=') {
                    let act = v.trim();
                    if is_valid_action(act) {
                        overrides.insert(k.trim().to_ascii_lowercase(), act.to_string());
                    }
                }
            }
            for b in &mut bindings {
                if let Some(custom_act) = overrides.get(&b.button_name.to_ascii_lowercase()) {
                    b.action = custom_act.clone();
                }
            }
        }
    }
    bindings
}

pub fn save_key_bindings(bindings: &[KeyBinding]) -> Result<(), String> {
    let _ = fs::create_dir_all(config_dir());
    let kf = keymap_file();
    let mut out = String::from(
        "# /etc/asusctl/keymap.conf — Custom ASUS Hardware Keyboard Button Bindings (debian_asusctl)\n\
         # Built-in actions: profile-next, profile-standard, swap-monitors, screenpad-toggle,\n\
         # screenpad-up, screenpad-down, kbd-up, kbd-down, touchpad-toggle, mic-mute,\n\
         # screenshot, camera-notify, status-notify, or exec:<command>\n\n",
    );
    for b in bindings {
        if !is_valid_action(&b.action) {
            return Err(format!("Invalid or unsafe action '{}'", b.action));
        }
        out.push_str(&format!(
            "# {} (Keycodes: {:?}, Scancodes: {:x?})\n",
            b.description, b.keycodes, b.scancodes
        ));
        out.push_str(&format!("{}={}\n\n", b.button_name, b.action));
    }
    fs::write(&kf, out).map_err(|e| format!("Failed writing {}: {}", kf.display(), e))
}

pub fn send_desktop_notification(summary: &str, body: &str, icon: &str) {
    if env::var("ASUSCTL_SANDBOX_DIR").is_ok() {
        return;
    }
    let icon_clean = sanitize_text(icon);
    let sum_clean = sanitize_text(summary);
    let body_clean = sanitize_text(body);
    let script = format!(
        "if [ \"$(id -u)\" = \"0\" ]; then \
            for uid_dir in /run/user/*; do \
                uid=$(basename \"$uid_dir\"); \
                user=$(id -nu \"$uid\" 2>/dev/null); \
                if [ -n \"$user\" ] && [ -S \"$uid_dir/bus\" ]; then \
                    xauth=$(ls /home/$user/.Xauthority /run/user/$uid/gdm/Xauthority 2>/dev/null | head -n 1); \
                    runuser -u \"$user\" -- env DISPLAY=:0 XAUTHORITY=\"$xauth\" DBUS_SESSION_BUS_ADDRESS=\"unix:path=$uid_dir/bus\" notify-send -a 'ASUS Control' -i '{}' -t 2500 '{}' '{}' >/dev/null 2>&1 & \
                fi; \
            done; \
         else \
            DISPLAY=\"${{DISPLAY:-:0}}\" notify-send -a 'ASUS Control' -i '{}' -t 2500 '{}' '{}' >/dev/null 2>&1 & \
         fi",
        icon_clean, sum_clean, body_clean, icon_clean, sum_clean, body_clean
    );
    let _ = Command::new("/bin/sh").arg("-c").arg(&script).spawn();
}

pub fn run_as_desktop_user(cmd_str: &str) {
    if env::var("ASUSCTL_SANDBOX_DIR").is_ok() {
        return;
    }
    let escaped = cmd_str.replace('"', "\\\"");
    let script = format!(
        "if [ \"$(id -u)\" = \"0\" ]; then \
            for uid_dir in /run/user/*; do \
                uid=$(basename \"$uid_dir\"); \
                user=$(id -nu \"$uid\" 2>/dev/null); \
                if [ -n \"$user\" ]; then \
                    xauth=$(ls /home/$user/.Xauthority /run/user/$uid/gdm/Xauthority 2>/dev/null | head -n 1); \
                    runuser -u \"$user\" -- env DISPLAY=:0 XAUTHORITY=\"$xauth\" DBUS_SESSION_BUS_ADDRESS=\"unix:path=$uid_dir/bus\" /bin/sh -c \"{}\" >/dev/null 2>&1 & \
                    break; \
                fi; \
            done; \
         else \
            DISPLAY=\"${{DISPLAY:-:0}}\" /bin/sh -c \"{}\" >/dev/null 2>&1 & \
         fi",
        escaped, escaped
    );
    let _ = Command::new("/bin/sh").arg("-c").arg(&script).spawn();
}

pub fn execute_button_action(action: &str) -> Result<String, String> {
    let act = action.trim();
    if !is_valid_action(act) {
        return Err(format!("Rejected invalid button action '{}'", act));
    }
    if let Some(custom_cmd) = act.strip_prefix("exec:") {
        run_as_desktop_user(custom_cmd.trim());
        return Ok(format!("Executed custom command: {}", custom_cmd.trim()));
    }
    match act.to_ascii_lowercase().as_str() {
        "profile-next" => {
            let next_p = detect_current_profile().next();
            let details = apply_power_profile(next_p)?;
            let fans = get_fan_telemetry();
            let fan_str = fans
                .iter()
                .map(|(l, r)| format!("{}: {} RPM", l, r))
                .collect::<Vec<_>>()
                .join(" | ");
            let msg = format!("Mode: {} | {}", next_p.as_str(), fan_str);
            send_desktop_notification(
                &format!("ASUS Performance Profile: {}", next_p.as_str()),
                &msg,
                "power-profile-performance-symbolic",
            );
            Ok(format!("Switched to {} ({})", next_p.as_str(), details.join(", ")))
        }
        "profile-standard" => {
            let next_p = detect_current_profile().next_standard_three();
            let details = apply_power_profile(next_p)?;
            send_desktop_notification(
                &format!("ASUS Performance Profile: {}", next_p.as_str()),
                &details.join(", "),
                "power-profile-balanced-symbolic",
            );
            Ok(format!("Switched to {}", next_p.as_str()))
        }
        "screenpad-toggle" => {
            let (en, br) = set_screenpad("toggle", None)?;
            let state_lbl = if en {
                format!("ON (Brightness: {}/255)", br)
            } else {
                "OFF".to_string()
            };
            send_desktop_notification("ASUS ScreenPad Plus", &state_lbl, "video-display");
            Ok(format!("ScreenPad Plus: {}", state_lbl))
        }
        "screenpad-up" => {
            let (_, br) = set_screenpad("up", None)?;
            send_desktop_notification(
                "ASUS ScreenPad Plus",
                &format!("Brightness: {}/255", br),
                "display-brightness",
            );
            Ok(format!("ScreenPad brightness: {}/255", br))
        }
        "screenpad-down" => {
            let (_, br) = set_screenpad("down", None)?;
            send_desktop_notification(
                "ASUS ScreenPad Plus",
                &format!("Brightness: {}/255", br),
                "display-brightness",
            );
            Ok(format!("ScreenPad brightness: {}/255", br))
        }
        "kbd-up" => {
            let lvl = set_kbd_backlight("up")?;
            send_desktop_notification(
                "ASUS Keyboard Backlight",
                &format!("Level: {}/3", lvl),
                "keyboard-brightness",
            );
            Ok(format!("Keyboard Backlight: {}/3", lvl))
        }
        "kbd-down" => {
            let lvl = set_kbd_backlight("down")?;
            send_desktop_notification(
                "ASUS Keyboard Backlight",
                &format!("Level: {}/3", lvl),
                "keyboard-brightness",
            );
            Ok(format!("Keyboard Backlight: {}/3", lvl))
        }
        "swap-monitors" => {
            let swap_cmd = "if command -v xdotool >/dev/null 2>&1; then \
                wid=$(xdotool getactivewindow 2>/dev/null); \
                if [ -n \"$wid\" ]; then \
                    eval $(xdotool getwindowgeometry --shell \"$wid\" 2>/dev/null); \
                    dp3_geom=$(xrandr --listmonitors 2>/dev/null | awk '/DP-3/{print $3}'); \
                    edp_geom=$(xrandr --listmonitors 2>/dev/null | awk '/eDP-1/{print $3}'); \
                    dp3_y=$(echo \"$dp3_geom\" | awk -F'+' '{print $3}'); \
                    dp3_x=$(echo \"$dp3_geom\" | awk -F'+' '{print $2}'); \
                    edp_x=$(echo \"$edp_geom\" | awk -F'+' '{print $2}'); \
                    edp_y=$(echo \"$edp_geom\" | awk -F'+' '{print $3}'); \
                    split_y=${dp3_y:-1440}; \
                    target_dp3_x=$(( ${dp3_x:-0} + 160 )); \
                    target_dp3_y=$(( ${split_y} + 80 )); \
                    target_edp_x=$(( ${edp_x:-0} + 160 )); \
                    target_edp_y=$(( ${edp_y:-0} + 80 )); \
                    if [ \"${Y:-0}\" -lt \"$split_y\" ]; then \
                        wmctrl -i -r \"$wid\" -b remove,maximized_vert,maximized_horz 2>/dev/null; \
                        xdotool windowmove \"$wid\" \"$target_dp3_x\" \"$target_dp3_y\"; \
                    else \
                        wmctrl -i -r \"$wid\" -b remove,maximized_vert,maximized_horz 2>/dev/null; \
                        xdotool windowmove \"$wid\" \"$target_edp_x\" \"$target_edp_y\"; \
                    fi; \
                fi; \
            fi";
            run_as_desktop_user(swap_cmd);
            send_desktop_notification(
                "ASUS Task Swap",
                "Swapped active window between Main Display (eDP-1) and ScreenPad Plus (DP-3)",
                "preferences-desktop-display",
            );
            Ok("Triggered Task Swap between Main OLED and ScreenPad Plus".to_string())
        }
        "touchpad-toggle" => {
            let tp_cmd = "for id in $(xinput list --id-only 'ELAN1406:00 04F3:3101 Touchpad' 'ELAN9009:00 04F3:2C23 Touchpad' 2>/dev/null); do \
                st=$(xinput list-props \"$id\" | awk '/Device Enabled/{print $NF}'); \
                if [ \"$st\" = \"1\" ]; then xinput disable \"$id\"; notify-send -a 'ASUS Control' 'Touchpad Disabled'; else xinput enable \"$id\"; notify-send -a 'ASUS Control' 'Touchpad Enabled'; fi; \
            done";
            run_as_desktop_user(tp_cmd);
            Ok("Toggled ASUS Touchpad".to_string())
        }
        "mic-mute" => {
            let mic_cmd = "pactl set-source-mute @DEFAULT_SOURCE@ toggle 2>/dev/null || amixer set Capture toggle 2>/dev/null";
            run_as_desktop_user(mic_cmd);
            send_desktop_notification(
                "ASUS Audio",
                "Toggled Microphone Mute",
                "microphone-sensitivity-muted",
            );
            Ok("Toggled Microphone Mute".to_string())
        }
        "screenshot" => {
            let shot_cmd = "xfce4-screenshooter -r 2>/dev/null || flameshot gui 2>/dev/null || gnome-screenshot -a 2>/dev/null";
            run_as_desktop_user(shot_cmd);
            Ok("Launched ASUS Screenshot Snipping Tool".to_string())
        }
        "camera-notify" => {
            send_desktop_notification("ASUS Webcam", "Camera hotkey pressed", "camera-web");
            Ok("Camera hotkey handled".to_string())
        }
        "status-notify" => {
            let prof = detect_current_profile();
            let fans = get_fan_telemetry();
            let fan_str = fans
                .iter()
                .map(|(l, r)| format!("{}: {} RPM", l, r))
                .collect::<Vec<_>>()
                .join(", ");
            let bat = get_battery_limit().unwrap_or(100);
            let (sp_en, sp_br, _) = get_screenpad_status().unwrap_or((false, 0, 255));
            let body = format!(
                "Profile: {} | Fans: {} | Battery Cap: {}% | ScreenPad: {}",
                prof.as_str(),
                if fan_str.is_empty() { "Auto".to_string() } else { fan_str },
                bat,
                if sp_en {
                    format!("ON ({}/255)", sp_br)
                } else {
                    "OFF".to_string()
                }
            );
            send_desktop_notification("ASUS System Status", &body, "computer");
            Ok(body)
        }
        other => Err(format!("Unsupported action '{}'", other)),
    }
}

pub fn format_status_report() -> String {
    let vendor = read_trimmed(sys_path("/sys/class/dmi/id/sys_vendor"))
        .unwrap_or_else(|| "ASUSTeK COMPUTER INC.".to_string());
    let product = read_trimmed(sys_path("/sys/class/dmi/id/product_name"))
        .unwrap_or_else(|| "ASUS Notebook".to_string());
    let board =
        read_trimmed(sys_path("/sys/class/dmi/id/board_name")).unwrap_or_else(|| "Unknown".to_string());
    let bios = read_trimmed(sys_path("/sys/class/dmi/id/bios_version"))
        .unwrap_or_else(|| "Unknown".to_string());
    let prof = detect_current_profile();
    let gov = read_trimmed(sys_path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"))
        .unwrap_or_else(|| "N/A".to_string());
    let epp = read_trimmed(sys_path(
        "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference",
    ))
    .unwrap_or_else(|| "N/A".to_string());
    let no_turbo = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/no_turbo"))
        .unwrap_or_else(|| "0".to_string());
    let max_perf = read_trimmed(sys_path("/sys/devices/system/cpu/intel_pstate/max_perf_pct"))
        .unwrap_or_else(|| "100".to_string());
    let pl1_w = read_trimmed(sys_path("/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw"))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| format!("{}W", v / 1_000_000))
        .unwrap_or_else(|| "N/A".to_string());
    let pl2_w = read_trimmed(sys_path("/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw"))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|v| format!("{}W", v / 1_000_000))
        .unwrap_or_else(|| "N/A".to_string());

    let fans = get_fan_telemetry();
    let fan_summary = if fans.is_empty() {
        "N/A".to_string()
    } else {
        fans.iter()
            .map(|(l, r)| format!("{}: {} RPM", l, r))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let fan_mode = get_fan_mode();
    let (kbd_lvl, kbd_max) = get_kbd_backlight().unwrap_or((0, 3));
    let (main_br, main_max) = get_main_display_brightness().unwrap_or((264, 528));
    let main_pct = (main_br * 100) / main_max.max(1);
    let (sp_en, sp_br, sp_max) = get_screenpad_status().unwrap_or((false, 0, 255));
    let bat_lim = get_battery_limit().unwrap_or(100);
    let fnlock = get_fnlock().unwrap_or(true);
    let wmi_ev = find_asus_wmi_input_event()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "not found".to_string());
    let sock = socket_path();
    let daemon_up = sock.exists();
    let confirmed = is_driver_confirmed();

    format!(
        "======================================================================\n\
         debian_asusctl v{} — ASUS Hardware & Power Performance Controller\n\
         ======================================================================\n\
         System Model       : {} ({})\n\
         Motherboard / BIOS : {} (BIOS {})\n\
         Daemon Socket      : {} ({})\n\
         Driver Status      : {}\n\
         WMI Hotkey Device  : {}\n\
         ----------------------------------------------------------------------\n\
         [Power & Thermal Performance]\n\
         Active Profile     : {} (Choices: Quiet, Balanced, Performance, FullSpeed)\n\
         CPU Governor / EPP : {} / {}\n\
         Intel Turbo Boost  : {} (Max Perf: {}%)\n\
         RAPL Package Power : PL1 (Sustained) = {} | PL2 (Burst) = {}\n\
         Fan Speeds         : {}\n\
         Fan Control Mode   : {}\n\
         ----------------------------------------------------------------------\n\
         [Peripherals, Backlights & Battery]\n\
         Main Display (eDP) : Brightness {}/{} ({}%)\n\
         ScreenPad Plus     : {} (Brightness: {}/{})\n\
         Keyboard Backlight : Level {}/{}\n\
         Battery Charge Cap : {}%\n\
         ASUS Fn-Lock       : {}\n\
         ======================================================================",
        VERSION,
        product,
        vendor,
        board,
        bios,
        sock.display(),
        if daemon_up { "ONLINE" } else { "DIRECT-SYSFS MODE" },
        if confirmed {
            "CONFIRMED (Permanent)"
        } else {
            "PENDING USER CONFIRMATION (Run 'asusctl confirm' to keep or 'asusctl revert' to rollback)"
        },
        wmi_ev,
        prof.as_str(),
        gov,
        epp,
        if no_turbo == "0" { "ENABLED" } else { "DISABLED" },
        max_perf,
        pl1_w,
        pl2_w,
        fan_summary,
        fan_mode,
        main_br,
        main_max,
        main_pct,
        if sp_en { "ENABLED" } else { "OFF" },
        sp_br,
        sp_max,
        kbd_lvl,
        kbd_max,
        bat_lim,
        if fnlock { "ENABLED (Y)" } else { "DISABLED (N)" }
    )
}
