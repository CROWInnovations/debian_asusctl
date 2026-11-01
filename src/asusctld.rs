// SPDX-License-Identifier: GPL-2.0-or-later
// debian_asusctl — Background Power Performance & Custom Keyboard Hotkey Daemon (asusctld)

#[allow(dead_code)]
mod asus_hw;

use asus_hw::*;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

fn restore_persisted_state() {
    let _ = fs::create_dir_all(CONFIG_DIR);
    if !Path::new(KEYMAP_FILE).exists() {
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

fn spawn_wmi_hotkey_listener() {
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
                    eprintln!("[asusctld] Input device {} disconnected, re-opening...", ev_path.display());
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
            Ok(format!("Restored default ASUS button bindings in {}", KEYMAP_FILE))
        }
        "KEYS_BIND" => {
            if arg1.is_empty() || arg2.is_empty() {
                return Err("Usage: KEYS_BIND <button> <action>".to_string());
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
    eprintln!(
        "Starting debian_asusctl daemon (asusctld v{})...",
        VERSION
    );
    restore_persisted_state();
    spawn_wmi_hotkey_listener();

    let _ = fs::remove_file(SOCKET_PATH);
    let listener = match UnixListener::bind(SOCKET_PATH) {
        Ok(l) => l,
        Err(e) => {
            eprintln!(
                "[asusctld] Warning: could not bind {} ({}). Running in hotkey-only mode.",
                SOCKET_PATH, e
            );
            loop {
                thread::sleep(Duration::from_secs(3600));
            }
        }
    };

    // Allow desktop users to communicate with asusctld over /run/asusctl.sock without sudo
    let _ = fs::set_permissions(SOCKET_PATH, fs::Permissions::from_mode(0o666));
    eprintln!("[asusctld] Ready and listening on {}", SOCKET_PATH);

    for stream in listener.incoming().flatten() {
        thread::spawn(move || handle_client(stream));
    }
}
