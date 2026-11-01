// SPDX-License-Identifier: GPL-2.0-or-later
// debian_asusctl — Background Power Performance & Custom Keyboard Hotkey Daemon (asusctld)

#[allow(dead_code)]
mod asus_hw;

use asus_hw::*;
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::panic;
use std::process::{self, Command};
use std::thread;
use std::time::{Duration, Instant};

fn install_daemon_crash_revert_hook() {
    panic::set_hook(Box::new(|panic_info| {
        eprintln!(
            "[asusctld] CRITICAL: Daemon panic detected ({}). Reverting to previous driver/hardware state...",
            panic_info
        );
        match revert_to_backup_state() {
            Ok(restored) => {
                eprintln!(
                    "[asusctld] Automatic crash rollback completed: {}",
                    restored.join(", ")
                );
            }
            Err(e) => {
                eprintln!("[asusctld] Rollback error during crash handler: {}", e);
            }
        }
    }));
}

fn restore_persisted_state() {
    let _ = fs::create_dir_all(config_dir());
    // Capture a pre-change backup snapshot if one does not already exist
    let _ = capture_backup_state(false);

    if !keymap_file().exists() {
        let _ = save_key_bindings(&default_key_bindings());
    }
    let st = AsusState::load();
    let _ = st.save();

    if let Ok(details) = apply_power_profile(st.profile) {
        eprintln!(
            "[asusctld] Restored power profile '{}' ({})",
            st.profile.as_str(),
            details.join(", ")
        );
    }
    let _ = set_battery_limit(st.charge_limit);
    let _ = set_kbd_backlight(&st.kbd_brightness.to_string());
    let _ = set_fnlock(if st.fnlock_enabled { "on" } else { "off" });
    if find_screenpad_dir().is_some() {
        if st.screenpad_enabled {
            let _ = set_screenpad("brightness", Some(&st.screenpad_brightness.to_string()));
        } else {
            let _ = set_screenpad("off", None);
        }
    }
}

fn spawn_confirmation_watchdog() {
    let needs_confirm = pending_confirm_file().exists() || !is_driver_confirmed();
    if !needs_confirm {
        return;
    }

    let timeout_secs = env::var("ASUSCTL_CONFIRM_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(90)
        .max(1);

    eprintln!(
        "[asusctld] Confirmation required within {}s (run 'asusctl confirm' to keep new driver settings, otherwise automatic rollback will trigger).",
        timeout_secs
    );

    thread::spawn(move || {
        let start = Instant::now();
        let total = Duration::from_secs(timeout_secs);
        while start.elapsed() < total {
            thread::sleep(Duration::from_millis(250));
            if is_driver_confirmed() && !pending_confirm_file().exists() {
                eprintln!("[asusctld] Driver configuration confirmed by user; watchdog disarmed.");
                return;
            }
        }

        if is_driver_confirmed() && !pending_confirm_file().exists() {
            return;
        }

        eprintln!(
            "[asusctld] WARNING: User confirmation timeout ({}s) expired without 'asusctl confirm'. Automatically reverting to previous driver/hardware state...",
            timeout_secs
        );
        match revert_to_backup_state() {
            Ok(restored) => {
                eprintln!(
                    "[asusctld] Auto-revert completed: {}",
                    restored.join(", ")
                );
            }
            Err(e) => {
                eprintln!("[asusctld] Auto-revert encountered error: {}", e);
            }
        }
        let _ = fs::remove_file(socket_path());
        process::exit(0);
    });
}

fn spawn_wmi_hotkey_listener() {
    if is_sandbox() {
        return;
    }
    thread::spawn(|| {
        let mut last_button = String::new();
        let mut last_press = Instant::now() - Duration::from_secs(10);

        loop {
            let Some(ev_path) = find_asus_wmi_input_event() else {
                eprintln!("[asusctld] Waiting for Asus WMI hotkeys input device...");
                thread::sleep(Duration::from_secs(5));
                continue;
            };

            let Ok(mut file) = File::open(&ev_path) else {
                eprintln!(
                    "[asusctld] Cannot open {} — retrying in 5s",
                    ev_path.display()
                );
                thread::sleep(Duration::from_secs(5));
                continue;
            };

            eprintln!(
                "[asusctld] Listening for ASUS custom hardware keys on {}",
                ev_path.display()
            );

            let mut buf = [0u8; 24];
            loop {
                if file.read_exact(&mut buf).is_err() {
                    eprintln!(
                        "[asusctld] Input device {} disconnected, re-opening...",
                        ev_path.display()
                    );
                    thread::sleep(Duration::from_secs(2));
                    break;
                }

                let ev_type = u16::from_ne_bytes([buf[16], buf[17]]);
                let ev_code = u16::from_ne_bytes([buf[18], buf[19]]);
                let ev_val = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);

                let mut matched_binding: Option<KeyBinding> = None;

                // EV_KEY (type == 1) and key press (value == 1)
                if ev_type == 1 && ev_val == 1 {
                    let bindings = load_key_bindings();
                    matched_binding = bindings
                        .into_iter()
                        .find(|b| b.keycodes.contains(&ev_code));
                }
                // EV_MSC (type == 4), MSC_SCAN (code == 4) for unmapped WMI scancodes (KEY_UNKNOWN=240)
                else if ev_type == 4 && ev_code == 4 && ev_val > 0 {
                    let sc = ev_val as u32;
                    let bindings = load_key_bindings();
                    matched_binding = bindings
                        .into_iter()
                        .find(|b| b.scancodes.contains(&sc));
                }

                if let Some(binding) = matched_binding {
                    let now = Instant::now();
                    // Debounce when kernel emits both EV_MSC (MSC_SCAN) and EV_KEY for the same physical press
                    if binding.button_name == last_button
                        && now.duration_since(last_press) < Duration::from_millis(250)
                    {
                        continue;
                    }
                    last_button = binding.button_name.clone();
                    last_press = now;

                    eprintln!(
                        "[asusctld] Button '{}' pressed -> executing action '{}'",
                        binding.button_name, binding.action
                    );
                    if let Err(err) = execute_button_action(&binding.action) {
                        eprintln!(
                            "[asusctld] Action '{}' failed: {}",
                            binding.action, err
                        );
                    }
                }
            }
        }
    });
}

fn handle_client(stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(4)));
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let cmd = line.trim();
    if cmd.is_empty() {
        return;
    }

    let response = process_command(cmd);
    let formatted = match response {
        Ok(msg) => format!("OK:{}\n", msg.replace('\n', "\\n")),
        Err(err) => format!("ERR:{}\n", err.replace('\n', "\\n")),
    };
    let _ = writer.write_all(formatted.as_bytes());
    let _ = writer.flush();
}

fn process_command(cmd: &str) -> Result<String, String> {
    let mut parts = cmd.splitn(3, ' ');
    let verb = parts.next().unwrap_or("").to_ascii_uppercase();
    let arg1 = parts.next().unwrap_or("").trim();
    let arg2 = parts.next().unwrap_or("").trim();

    match verb.as_str() {
        "STATUS" => Ok(format_status_report()),
        "CONFIRM" => {
            confirm_driver_state()?;
            Ok("debian_asusctl driver configuration confirmed and locked in.".to_string())
        }
        "REVERT" => {
            let restored = revert_to_backup_state()?;
            Ok(format!(
                "Reverted to previous driver/hardware state ({})",
                restored.join(", ")
            ))
        }
        "BACKUP" => {
            capture_backup_state(true)?;
            Ok(format!(
                "Saved current hardware/driver state snapshot to {}",
                backup_file().display()
            ))
        }
        "PROFILE_SET" => {
            let prof = PowerProfile::from_str(arg1).ok_or_else(|| {
                format!(
                    "Invalid profile '{}'. Valid profiles: Quiet, Balanced, Performance, FullSpeed",
                    arg1
                )
            })?;
            let actions = apply_power_profile(prof)?;
            Ok(format!(
                "Active profile set to: {} ({})",
                prof.as_str(),
                actions.join(", ")
            ))
        }
        "PROFILE_NEXT" => execute_button_action("profile-next"),
        "KBD_SET" => {
            let lvl = set_kbd_backlight(arg1)?;
            Ok(format!("Keyboard backlight brightness set to: {}/3", lvl))
        }
        "BATTERY_SET" => {
            let lim = arg1
                .parse::<u8>()
                .map_err(|_| format!("Invalid battery charge limit '{}'", arg1))?;
            let out = set_battery_limit(lim)?;
            Ok(format!("Battery charge limit set to: {}%", out))
        }
        "FAN_SET" => {
            let mode = set_fan_mode(arg1)?;
            Ok(format!("Fan control mode set to: {}", mode))
        }
        "SCREENPAD" => {
            let opt_arg = if arg2.is_empty() { None } else { Some(arg2) };
            let (en, br) = set_screenpad(arg1, opt_arg)?;
            Ok(format!(
                "ScreenPad Plus: {} (Brightness: {}/255)",
                if en { "ENABLED" } else { "OFF" },
                br
            ))
        }
        "FNLOCK" => {
            let st = set_fnlock(arg1)?;
            Ok(format!(
                "ASUS Fn-Lock set to: {}",
                if st { "ENABLED (Y)" } else { "DISABLED (N)" }
            ))
        }
        "KEYS_RESET" => {
            let defs = default_key_bindings();
            save_key_bindings(&defs)?;
            Ok(format!(
                "Restored default ASUS button bindings in {}",
                keymap_file().display()
            ))
        }
        "KEYS_BIND" => {
            if arg1.is_empty() || arg2.is_empty() {
                return Err("Usage: KEYS_BIND <button> <action>".to_string());
            }
            if !is_valid_action(arg2) {
                return Err(format!("Invalid or unsafe action '{}'", arg2));
            }
            let mut bindings = load_key_bindings();
            let found = bindings
                .iter_mut()
                .find(|b| b.button_name.eq_ignore_ascii_case(arg1))
                .ok_or_else(|| format!("Unknown button '{}'", arg1))?;
            found.action = arg2.to_string();
            save_key_bindings(&bindings)?;
            Ok(format!("Bound button '{}' -> '{}'", arg1, arg2))
        }
        "KEYS_TRIGGER" => {
            let bindings = load_key_bindings();
            let found = bindings
                .iter()
                .find(|b| b.button_name.eq_ignore_ascii_case(arg1))
                .ok_or_else(|| format!("Unknown button '{}'", arg1))?;
            let out = execute_button_action(&found.action)?;
            Ok(format!(
                "Triggered '{}' ({}): {}",
                found.button_name, found.action, out
            ))
        }
        other => Err(format!("Unknown daemon command '{}'", other)),
    }
}

fn main() {
    install_daemon_crash_revert_hook();
    eprintln!(
        "Starting debian_asusctl daemon (asusctld v{})...",
        VERSION
    );
    restore_persisted_state();
    spawn_confirmation_watchdog();
    spawn_wmi_hotkey_listener();

    let sock = socket_path();
    if let Some(parent) = sock.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::remove_file(&sock);
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[asusctld] Warning: could not bind {} ({}). Running in hotkey-only mode.",
                sock.display(),
                e
            );
            loop {
                thread::sleep(Duration::from_secs(3600));
            }
        }
    };

    // Restrict socket to root:sudo (mode 0660) for security
    let _ = fs::set_permissions(&sock, fs::Permissions::from_mode(0o660));
    if !is_sandbox() {
        let _ = Command::new("chgrp")
            .args(["sudo", &sock.to_string_lossy()])
            .status();
    }
    eprintln!("[asusctld] Ready and listening on {}", sock.display());

    for stream in listener.incoming().flatten() {
        thread::spawn(move || handle_client(stream));
    }
}
