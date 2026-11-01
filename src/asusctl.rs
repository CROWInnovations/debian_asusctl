// SPDX-License-Identifier: GPL-2.0-or-later
// debian_asusctl — Command-Line Interface (asusctl) for Debian / Kali Linux on ASUS Laptops

#[allow(dead_code)]
mod asus_hw;

use asus_hw::*;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process;
use std::time::Duration;

fn print_help() {
    println!(
        "debian_asusctl v{} — ASUS Power Performance, Fan, ScreenPad & Custom Key Controller for Debian Linux

USAGE:
    asusctl [OPTIONS] [SUBCOMMAND]

STANDARD UPSTREAM-COMPATIBLE OPTIONS:
    -h, --help, -?, help           Print this full command & usage reference
    -v, --version                  Print version information
    -s, --show-supported           Show detected ASUS hardware & supported interfaces
    -k, --chg-led <LEVEL>          Set keyboard backlight brightness: off, low, med, high (or 0..3)
    -n, --next-kbd-bright          Cycle keyboard backlight to next brightness level
    -p, --prev-kbd-bright          Cycle keyboard backlight to previous brightness level
    -c, --chg-limit <20..100>      Set battery charge limit percentage (e.g. 80)
    -o, --one-shot-chg             Set battery charge limit to 100%

SUBCOMMANDS:
    status | info                  Display complete ASUS hardware, power, fan & peripheral status
    profile [ARGS]                 Query or switch ASUS power & thermal performance profiles:
        profile -p | status            Show active profile
        profile -l | list              List available profiles (Quiet, Balanced, Performance, FullSpeed)
        profile -P <NAME> | set <NAME> Set active profile (Quiet, Balanced, Performance, FullSpeed)
        profile -n | next              Cycle to next performance profile
    fan [status|auto|fullspeed]    Show fan RPM telemetry or set fan mode (auto=Curve, fullspeed=Max)
    led [status|off|low|med|high|up|down|0..3]
                                   Control or query ASUS keyboard backlight brightness
    screenpad [status|on|off|toggle|up|down|brightness <0..255>]
                                   Control ASUS ScreenPad Plus secondary display power & brightness
    battery [status|<20..100>]     Query or set ASUS battery charge control end threshold (20..100%)
    fnlock [status|on|off|toggle]  Query or toggle ASUS Fn-key lock (/sys/module/asus_wmi/parameters/fnlock_default)
    keys [SUBCOMMAND]              Configure & test ASUS custom hardware keyboard buttons:
        keys list                      List all ASUS custom hardware keys, keycodes & bound actions
        keys bind <BUTTON> <ACTION>    Bind a custom button to a built-in action or 'exec:<shell_cmd>'
        keys trigger <BUTTON>          Simulate pressing a custom button and execute its bound action
        keys reset                     Restore default button bindings in /etc/asusctl/keymap.conf
        keys monitor                   Live-monitor raw key/scancode events from Asus WMI hotkeys

CUSTOM HARDWARE BUTTON NAMES & BUILT-IN ACTIONS:
    Buttons : fan_performance, task_swap, screenpad_toggle, myasus_hub, kbd_backlight_up,
              kbd_backlight_down, touchpad_toggle, mic_mute, camera_toggle, selective_screenshot
    Actions : profile-next, profile-standard, swap-monitors, screenpad-toggle, screenpad-up,
              screenpad-down, kbd-up, kbd-down, touchpad-toggle, mic-mute, screenshot,
              status-notify, camera-notify, or 'exec:<custom shell command>'

CONFIGURATION & DAEMON FILES:
    State Config  : /etc/asusctl/asusctl.conf
    Keymap Config : /etc/asusctl/keymap.conf
    Daemon Socket : /run/asusctl.sock (systemd service: asusctld.service)

EXAMPLES / USAGE REFERENCE:
    asusctl status                                   # Full hardware, profile, fan, ScreenPad & battery status
    asusctl profile -l                               # List profiles (Quiet, Balanced, Performance, FullSpeed)
    asusctl profile -P Performance                   # Switch to Performance profile
    asusctl profile -n                               # Cycle to the next power profile
    asusctl fan status                               # Show CPU/GPU fan RPMs and PWM control mode
    asusctl fan fullspeed                            # Force maximum fan speed boost (pwm1_enable=0)
    asusctl fan auto                                 # Restore automatic thermal fan curve (pwm1_enable=2)
    asusctl -k high                                  # Set keyboard backlight to high (3/3)
    asusctl -n  /  asusctl -p                        # Step keyboard backlight up / down
    asusctl screenpad toggle                         # Toggle ScreenPad Plus secondary display ON/OFF
    asusctl screenpad brightness 200                 # Set ScreenPad Plus brightness (0..255)
    asusctl -c 80                                    # Cap battery charging at 80% for longevity
    asusctl fnlock toggle                            # Toggle ASUS Fn-Lock state
    asusctl keys list                                # Show all custom hardware buttons & bindings
    asusctl keys bind myasus_hub \"exec:xfce4-terminal\" # Bind Fn+F12 / MyASUS key to launch terminal
    asusctl keys trigger fan_performance             # Test-trigger the Fan/Performance button action
    sudo asusctl keys monitor                        # Live-monitor raw ASUS WMI key presses",
        VERSION
    );
}

fn send_daemon_request(cmd_line: &str) -> Option<Result<String, String>> {
    if !Path::new(SOCKET_PATH).exists() {
        return None;
    }
    let stream = UnixStream::connect(SOCKET_PATH).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(4)));
    let mut writer = stream.try_clone().ok()?;
    if writer.write_all(format!("{}\n", cmd_line).as_bytes()).is_err() {
        return None;
    }
    let _ = writer.flush();
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    if reader.read_line(&mut resp).is_err() {
        return None;
    }
    let trimmed = resp.trim();
    if let Some(rest) = trimmed.strip_prefix("OK:") {
        Some(Ok(rest.trim().replace("\\n", "\n")))
    } else if let Some(err) = trimmed.strip_prefix("ERR:") {
        Some(Err(err.trim().replace("\\n", "\n")))
    } else if !trimmed.is_empty() {
        Some(Ok(trimmed.to_string()))
    } else {
        None
    }
}

fn set_profile_cli(prof_name: &str) -> Result<(), String> {
    let prof = PowerProfile::from_str(prof_name).ok_or_else(|| {
        format!(
            "Invalid profile '{}'. Valid profiles: Quiet, Balanced, Performance, FullSpeed",
            prof_name
        )
    })?;
    if let Some(res) = send_daemon_request(&format!("PROFILE_SET {}", prof.as_str())) {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let actions = apply_power_profile(prof)?;
    println!(
        "Active profile set to: {} ({})",
        prof.as_str(),
        actions.join(", ")
    );
    Ok(())
}

fn next_profile_cli() -> Result<(), String> {
    if let Some(res) = send_daemon_request("PROFILE_NEXT") {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let next_p = detect_current_profile().next();
    let actions = apply_power_profile(next_p)?;
    println!(
        "Cycled active profile to: {} ({})",
        next_p.as_str(),
        actions.join(", ")
    );
    Ok(())
}

fn set_kbd_cli(level: &str) -> Result<(), String> {
    if let Ok(lvl) = set_kbd_backlight(level) {
        println!("Keyboard backlight brightness set to: {}/3", lvl);
        return Ok(());
    }
    if let Some(res) = send_daemon_request(&format!("KBD_SET {}", level)) {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let lvl = set_kbd_backlight(level)?;
    println!("Keyboard backlight brightness set to: {}/3", lvl);
    Ok(())
}

fn set_battery_cli(limit: u8) -> Result<(), String> {
    if let Ok(lim) = set_battery_limit(limit) {
        println!("Battery charge limit set to: {}%", lim);
        return Ok(());
    }
    if let Some(res) = send_daemon_request(&format!("BATTERY_SET {}", limit)) {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let lim = set_battery_limit(limit)?;
    println!("Battery charge limit set to: {}%", lim);
    Ok(())
}

fn set_fan_cli(mode: &str) -> Result<(), String> {
    if let Ok(m) = set_fan_mode(mode) {
        println!("Fan control mode set to: {}", m);
        return Ok(());
    }
    if let Some(res) = send_daemon_request(&format!("FAN_SET {}", mode)) {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let m = set_fan_mode(mode)?;
    println!("Fan control mode set to: {}", m);
    Ok(())
}

fn set_screenpad_cli(action: &str, arg: Option<&str>) -> Result<(), String> {
    if let Ok((en, br)) = set_screenpad(action, arg) {
        println!(
            "ScreenPad Plus: {} (Brightness: {}/255)",
            if en { "ENABLED" } else { "OFF" },
            br
        );
        return Ok(());
    }
    let req = match arg {
        Some(a) => format!("SCREENPAD {} {}", action, a),
        None => format!("SCREENPAD {}", action),
    };
    if let Some(res) = send_daemon_request(&req) {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let (en, br) = set_screenpad(action, arg)?;
    println!(
        "ScreenPad Plus: {} (Brightness: {}/255)",
        if en { "ENABLED" } else { "OFF" },
        br
    );
    Ok(())
}

fn set_fnlock_cli(mode: &str) -> Result<(), String> {
    if let Ok(st) = set_fnlock(mode) {
        println!("ASUS Fn-Lock set to: {}", if st { "ENABLED (Y)" } else { "DISABLED (N)" });
        return Ok(());
    }
    if let Some(res) = send_daemon_request(&format!("FNLOCK {}", mode)) {
        let msg = res?;
        println!("{}", msg);
        return Ok(());
    }
    let st = set_fnlock(mode)?;
    println!("ASUS Fn-Lock set to: {}", if st { "ENABLED (Y)" } else { "DISABLED (N)" });
    Ok(())
}

fn print_keys_list() {
    let bindings = load_key_bindings();
    println!("==========================================================================================");
    println!(
        "{:<22} {:<16} {:<22} {}",
        "BUTTON NAME", "LINUX KEYCODES", "BOUND ACTION", "HARDWARE DESCRIPTION"
    );
    println!("==========================================================================================");
    for b in bindings {
        let kc_str = b
            .keycodes
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:<22} {:<16} {:<22} {}",
            b.button_name, kc_str, b.action, b.description
        );
    }
    println!("------------------------------------------------------------------------------------------");
    println!("Config file : {}", KEYMAP_FILE);
    println!("Built-in actions: profile-next, profile-standard, swap-monitors, screenpad-toggle,");
    println!("                  screenpad-up, screenpad-down, kbd-up, kbd-down, touchpad-toggle,");
    println!("                  mic-mute, screenshot, status-notify, or 'exec:<shell command>'");
    println!("==========================================================================================");
}

fn monitor_wmi_keys() -> Result<(), String> {
    let ev_path = find_asus_wmi_input_event()
        .ok_or_else(|| "Asus WMI hotkeys input device not found".to_string())?;
    let mut f = File::open(&ev_path).map_err(|e| {
        format!(
            "Cannot open {} ({}). Run 'sudo asusctl keys monitor' or ensure user is in 'input' group.",
            ev_path.display(),
            e
        )
    })?;
    let bindings = load_key_bindings();
    println!(
        "Monitoring ASUS WMI Hotkeys on {} (Press Ctrl+C to exit)...",
        ev_path.display()
    );
    let mut buf = [0u8; 24];
    loop {
        if f.read_exact(&mut buf).is_err() {
            break;
        }
        let ev_type = u16::from_ne_bytes([buf[16], buf[17]]);
        let ev_code = u16::from_ne_bytes([buf[18], buf[19]]);
        let ev_val = i32::from_ne_bytes([buf[20], buf[21], buf[22], buf[23]]);
        if ev_type == 1 && ev_val == 1 {
            let matched = bindings
                .iter()
                .find(|b| b.keycodes.contains(&ev_code))
                .map(|b| format!("{} -> {}", b.button_name, b.action))
                .unwrap_or_else(|| "unmapped".to_string());
            println!("[KEY PRESS] keycode={} ({})", ev_code, matched);
        } else if ev_type == 4 && ev_code == 4 {
            let sc = ev_val as u32;
            let matched = bindings
                .iter()
                .find(|b| b.scancodes.contains(&sc))
                .map(|b| format!("{} -> {}", b.button_name, b.action))
                .unwrap_or_else(|| "unmapped".to_string());
            println!("[WMI SCAN ] scancode=0x{:02x} ({})", sc, matched);
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        println!("{}", format_status_report());
        return Ok(());
    }
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "-h" | "--help" | "-?" | "help"))
    {
        print_help();
        return Ok(());
    }

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => {
                print_help();
                return Ok(());
            }
            "-v" | "--version" | "version" => {
                println!("asusctl {}", VERSION);
                return Ok(());
            }
            "-s" | "--show-supported" | "status" | "info" => {
                println!("{}", format_status_report());
                return Ok(());
            }
            "-n" | "--next-kbd-bright" => {
                set_kbd_cli("up")?;
            }
            "-p" | "--prev-kbd-bright" => {
                set_kbd_cli("down")?;
            }
            "-k" | "--chg-led" => {
                i += 1;
                let lvl = args
                    .get(i)
                    .ok_or_else(|| "Missing argument for -k / --chg-led".to_string())?;
                set_kbd_cli(lvl)?;
            }
            "-c" | "--chg-limit" => {
                i += 1;
                let lim_s = args
                    .get(i)
                    .ok_or_else(|| "Missing limit (20..100) for -c / --chg-limit".to_string())?;
                let lim = lim_s
                    .parse::<u8>()
                    .map_err(|_| format!("Invalid battery charge limit '{}'", lim_s))?;
                set_battery_cli(lim)?;
            }
            "-o" | "--one-shot-chg" => {
                set_battery_cli(100)?;
            }
            "profile" | "profiles" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("-p");
                match sub {
                    "-p" | "status" | "get" => {
                        println!("Active profile is {}", detect_current_profile().as_str());
                        return Ok(());
                    }
                    "-l" | "list" => {
                        let cur = detect_current_profile();
                        for p in [
                            PowerProfile::Quiet,
                            PowerProfile::Balanced,
                            PowerProfile::Performance,
                            PowerProfile::FullSpeed,
                        ] {
                            let mark = if p == cur { "*" } else { " " };
                            println!("{} {}", mark, p.as_str());
                        }
                        return Ok(());
                    }
                    "-n" | "next" => {
                        next_profile_cli()?;
                        return Ok(());
                    }
                    "-P" | "set" => {
                        let target = args.get(i + 2).ok_or_else(|| {
                            "Provide profile name: Quiet, Balanced, Performance, or FullSpeed"
                                .to_string()
                        })?;
                        set_profile_cli(target)?;
                        return Ok(());
                    }
                    other => {
                        set_profile_cli(other)?;
                        return Ok(());
                    }
                }
            }
            "fan" | "fans" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                match sub {
                    "status" | "info" | "-s" => {
                        let fans = get_fan_telemetry();
                        println!("Fan Control Mode : {}", get_fan_mode());
                        for (lbl, rpm) in fans {
                            println!("{:<16} : {} RPM", lbl, rpm);
                        }
                        return Ok(());
                    }
                    mode => {
                        set_fan_cli(mode)?;
                        return Ok(());
                    }
                }
            }
            "led" | "kbd" | "backlight" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                match sub {
                    "status" | "get" => {
                        let (lvl, max) = get_kbd_backlight().unwrap_or((0, 3));
                        println!("Keyboard Backlight Brightness: {}/{}", lvl, max);
                        return Ok(());
                    }
                    lvl => {
                        set_kbd_cli(lvl)?;
                        return Ok(());
                    }
                }
            }
            "screenpad" | "screenpad-plus" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                match sub {
                    "status" | "get" => {
                        if let Some((en, br, max)) = get_screenpad_status() {
                            println!(
                                "ScreenPad Plus: {} (Brightness: {}/{})",
                                if en { "ENABLED" } else { "OFF" },
                                br,
                                max
                            );
                        } else {
                            println!("ScreenPad Plus interface not detected on this model.");
                        }
                        return Ok(());
                    }
                    action => {
                        let arg = args.get(i + 2).map(|s| s.as_str());
                        set_screenpad_cli(action, arg)?;
                        return Ok(());
                    }
                }
            }
            "battery" | "charge" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                match sub {
                    "status" | "get" => {
                        let lim = get_battery_limit().unwrap_or(100);
                        println!("Battery Charge Control End Threshold: {}%", lim);
                        return Ok(());
                    }
                    val_s => {
                        let lim = val_s
                            .parse::<u8>()
                            .map_err(|_| format!("Invalid battery limit '{}'", val_s))?;
                        set_battery_cli(lim)?;
                        return Ok(());
                    }
                }
            }
            "fnlock" | "fn-lock" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("status");
                match sub {
                    "status" | "get" => {
                        let st = get_fnlock().unwrap_or(true);
                        println!("ASUS Fn-Lock: {}", if st { "ENABLED (Y)" } else { "DISABLED (N)" });
                        return Ok(());
                    }
                    mode => {
                        set_fnlock_cli(mode)?;
                        return Ok(());
                    }
                }
            }
            "keys" | "key" | "keymap" | "buttons" => {
                let sub = args.get(i + 1).map(|s| s.as_str()).unwrap_or("list");
                match sub {
                    "list" | "status" | "-l" => {
                        print_keys_list();
                        return Ok(());
                    }
                    "reset" => {
                        if let Some(res) = send_daemon_request("KEYS_RESET") {
                            println!("{}", res?);
                            return Ok(());
                        }
                        let defs = default_key_bindings();
                        save_key_bindings(&defs)?;
                        println!("Restored default ASUS button bindings in {}", KEYMAP_FILE);
                        return Ok(());
                    }
                    "bind" | "set" => {
                        let btn = args.get(i + 2).ok_or_else(|| {
                            "Usage: asusctl keys bind <button_name> <action>".to_string()
                        })?;
                        let act_parts = &args[i + 3..];
                        if act_parts.is_empty() {
                            return Err("Usage: asusctl keys bind <button_name> <action>".to_string());
                        }
                        let act = act_parts.join(" ");
                        if let Some(res) = send_daemon_request(&format!("KEYS_BIND {} {}", btn, act))
                        {
                            println!("{}", res?);
                            return Ok(());
                        }
                        let mut bindings = load_key_bindings();
                        let found = bindings
                            .iter_mut()
                            .find(|b| b.button_name.eq_ignore_ascii_case(btn))
                            .ok_or_else(|| {
                                format!(
                                    "Unknown button '{}'. Run 'asusctl keys list' to see button names.",
                                    btn
                                )
                            })?;
                        found.action = act.clone();
                        save_key_bindings(&bindings)?;
                        println!("Bound button '{}' -> '{}'", btn, act);
                        return Ok(());
                    }
                    "trigger" | "test" => {
                        let btn = args.get(i + 2).ok_or_else(|| {
                            "Usage: asusctl keys trigger <button_name>".to_string()
                        })?;
                        let bindings = load_key_bindings();
                        let found = bindings
                            .iter()
                            .find(|b| b.button_name.eq_ignore_ascii_case(btn))
                            .ok_or_else(|| {
                                format!(
                                    "Unknown button '{}'. Run 'asusctl keys list' to see button names.",
                                    btn
                                )
                            })?;
                        if let Ok(out) = execute_button_action(&found.action) {
                            println!("Triggered '{}' ({}): {}", found.button_name, found.action, out);
                            return Ok(());
                        }
                        if let Some(res) = send_daemon_request(&format!("KEYS_TRIGGER {}", btn)) {
                            println!("{}", res?);
                            return Ok(());
                        }
                        let out = execute_button_action(&found.action)?;
                        println!("Triggered '{}' ({}): {}", found.button_name, found.action, out);
                        return Ok(());
                    }
                    "monitor" | "listen" => {
                        monitor_wmi_keys()?;
                        return Ok(());
                    }
                    other => {
                        return Err(format!(
                            "Unknown keys subcommand '{}' (use: list, bind, trigger, reset, monitor)",
                            other
                        ));
                    }
                }
            }
            other => {
                return Err(format!(
                    "Unrecognized option or subcommand '{}'. Run 'asusctl --help' for usage.",
                    other
                ));
            }
        }
        i += 1;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
