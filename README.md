# debian_asusctl (`v6.2.0-debian2`)

**ASUS Power Performance, Thermal Fan Boost, ScreenPad Plus & Custom Hardware Button Controller for Debian / Kali Linux**

`debian_asusctl` is a zero-dependency, native Rust implementation of `asusctl` and its background `evdev`/WMI daemon `asusctld`, engineered specifically for Debian and Kali GNU/Linux on ASUS laptops — including **ASUS ZenBook Pro Duo (`UX582LR`)**, ZenBook Duo, ZenBook, VivoBook, TUF, and ROG series.

---

## Why `debian_asusctl`?

Upstream `asusctl` fails on several ASUS systems running Debian/Kali Linux 6.x/7.x kernels due to three hardware/kernel architectural gaps:

1. **ASUS ATK vs. ASUSWMI Power Profiles (`UX582LR`)**:
   * On laptops like the ZenBook Pro Duo `UX582LR`, `dmesg` reports `asus-nb-wmi: Detected ATK, not ASUSWMI, use DSTS` and `asus_armoury: No matching power limits found for this system`.
   * `debian_asusctl` coordinates **all 7 Linux power and thermal subsystems** simultaneously across 4 performance profiles (`Quiet`, `Balanced`, `Performance`, `FullSpeed`):
     * **ASUS ATK CPU Voltage/Frequency Policy**: `/sys/devices/platform/asus-nb-wmi/cpufv` (`2`=Quiet, `0`=Balanced, `1`=Performance)
     * **ASUS WMI Thermal Policy & ACPI Platform Profile**: `/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy` & `/sys/firmware/acpi/platform_profile`
     * **ASUS Dual-Fan PWM Control**: `/sys/devices/platform/asus-nb-wmi/hwmon/hwmon*/pwm1_enable` (`2`=Automatic Thermal Curve, `0`=FullSpeed Maximum Fan Boost) + live `cpu_fan` and `gpu_fan` RPM telemetry
     * **Intel P-State Turbo & Dynamic Boost**: `/sys/devices/system/cpu/intel_pstate/{no_turbo,hwp_dynamic_boost,min_perf_pct,max_perf_pct}`
     * **Per-Core CPUFreq Governor & EPP**: `scaling_governor` (`powersave` / `performance`) and `energy_performance_preference` (`power` / `balance_performance` / `performance`) across all logical cores
     * **Intel RAPL Package Power Limits (`PL1` / `PL2`)**: `/sys/class/powercap/intel-rapl:0/constraint_{0,1}_power_limit_uw` (`28W/45W` Quiet, `45W/90W` Balanced, `70W/135W` Performance & FullSpeed)

2. **Inverted `bl_power` Fix & Multi-Monitor Sync for Built-In Second Screen (`ScreenPad Plus` / `DP-3`)**:
   * In Linux kernel 7.x `asus-wmi.ko` (`update_screenpad_bl_status`), `/sys/class/backlight/asus_screenpad/bl_power` is inverted relative to standard Linux `FB_BLANK_*` backlight drivers (`bl_power = 1` calls `DEVS(0x00050031, 1)` to power **ON** the ScreenPad Plus, while `bl_power = 0` calls `DEVS(0x00050031, 0)` to power **OFF** the ScreenPad Plus).
   * `debian_asusctl` handles the inverted WMI power bit properly, synchronizes the high-resolution secondary display (`DP-3` `3840x1100` / scaled `3072x880` below `eDP-1`) via `xrandr`, and locks/preserves the primary display's backlight (`/sys/class/backlight/intel_backlight/brightness`) so desktop power managers (`xfce4-power-manager`) never spike primary screen brightness when toggling the second screen.

3. **Custom Hardware Keyboard Buttons (Bypassing the X11 `keycode > 247` Drop Limit)**:
   * The `Asus WMI hotkeys` input device emits Linux keycodes above `247` (`KEY_FN_F=482`, `KEY_FN_F5=470`, `KEY_DISPLAYTOGGLE=431`, `KEY_TOUCHPAD_TOGGLE=530`, `KEY_MICMUTE=248`, `KEY_SELECTIVE_SCREENSHOT=634`) and unmapped WMI scancodes (such as `0x6a` for the `UX582LR` ScreenPad toggle button) that X11 silently drops due to its 8-bit keycode limit.
   * `asusctld` listens directly to raw `evdev` `EV_KEY` and `EV_MSC` (`MSC_SCAN`) events on `/dev/input/by-path/platform-asus-nb-wmi-event` and executes configurable actions in `/etc/asusctl/keymap.conf` with desktop OSD notifications.

---

## Installation

### Option 1: Install the Prebuilt Standard Debian Package (`.deb`)

```bash
sudo dpkg -i debian_asusctl.deb
```

### Option 2: Build from Source & Install

Requirements: `rustc` (`>= 1.70`), `dpkg-deb`.

```bash
./build_deb.sh
sudo dpkg -i debian_asusctl.deb
```

Installing the package configures:
* `/usr/bin/asusctl` — Command-line controller
* `/usr/sbin/asusctld` — Background power & WMI hotkey daemon (`asusctld.service`)
* `/etc/asusctl/asusctl.conf` — Persistent power profile, ScreenPad, keyboard LED, battery cap & Fn-Lock state restored on boot
* `/etc/asusctl/keymap.conf` — Custom hardware button bindings
* `/lib/udev/rules.d/90-debian-asusctl.rules` & `/lib/udev/hwdb.d/90-debian-asusctl.hwdb` — Unprivileged `sudo`/`input` group hardware access and kernel WMI scancode mappings

---

## Usage Reference (`asusctl --help`)

```bash
# View full hardware, power profile, CPU governor/RAPL, fan RPMs, ScreenPad & battery status
asusctl status

# List or switch power performance profiles (Quiet, Balanced, Performance, FullSpeed)
asusctl profile -l
asusctl profile -P Performance
asusctl profile -n                                   # Cycle to the next profile

# Fan telemetry & manual FullSpeed boost
asusctl fan status
asusctl fan fullspeed                                # Force maximum fan speed boost (pwm1_enable=0)
asusctl fan auto                                     # Restore automatic thermal fan curve (pwm1_enable=2)

# Keyboard backlight control (off, low, med, high, or 0..3)
asusctl -k high
asusctl -n                                           # Step keyboard backlight up
asusctl -p                                           # Step keyboard backlight down

# ASUS ScreenPad Plus secondary display control
asusctl screenpad status
asusctl screenpad toggle                             # Toggle ScreenPad Plus ON/OFF
asusctl screenpad on
asusctl screenpad off
asusctl screenpad brightness 200                     # Set ScreenPad Plus brightness (0..255)

# Battery health charge limit (20..100%) & Fn-Lock
asusctl -c 80                                        # Cap battery charge at 80%
asusctl fnlock toggle                                # Toggle ASUS Fn-Lock state

# List, customize, test-trigger, or live-monitor custom ASUS keyboard buttons
asusctl keys list
asusctl keys bind myasus_hub "exec:xfce4-terminal"   # Bind Fn+F12 / MyASUS key to any shell command
asusctl keys trigger fan_performance                 # Simulate pressing a custom button
sudo asusctl keys monitor                            # Live-monitor raw ASUS WMI key/scancode events
```

---

## Custom Hardware Buttons (`/etc/asusctl/keymap.conf`)

| Button Name | Linux Keycodes | WMI Scancodes | Default Action | Description |
| :--- | :--- | :--- | :--- | :--- |
| `fan_performance` | `203, 482, 470, 169` | `0x9d, 0xb3, 0x99, 0xae, 0x6c` | `profile-next` | ASUS Fan / Performance Mode Toggle Key (Above Touchpad / `Fn+F`) |
| `task_swap` | `202` | `0x38, 0xffff01, 0x8a` | `swap-monitors` | ASUS Task Swap Key (Swap Active Window Between `eDP-1` & `DP-3`) |
| `screenpad_toggle` | `431, 149, 183` | `0x6a, 0x2d, 0x37` | `screenpad-toggle` | ASUS ScreenPad Plus On/Off Toggle Key (Above Touchpad) |
| `myasus_hub` | `148` | `0x86, 0x39` | `status-notify` | MyASUS / ProArt / ROG Launcher Key (`Fn+F12`) |
| `kbd_backlight_up` | `230, 185` | `0xc4` | `kbd-up` | Keyboard Backlight Brightness Up (`Fn+F8`) |
| `kbd_backlight_down` | `229, 184` | `0xc5` | `kbd-down` | Keyboard Backlight Brightness Down (`Fn+F7`) |
| `touchpad_toggle` | `530, 531` | `0x6b` | `touchpad-toggle` | Internal Touchpad Enable/Disable (`Fn+F6`) |
| `mic_mute` | `248` | `0x7c` | `mic-mute` | Microphone Mute Toggle (`Fn+F9`) |
| `camera_toggle` | `212` | `0x82, 0x85` | `camera-notify` | Webcam Shield / Camera Toggle (`Fn+F10`) |
| `selective_screenshot` | `634` | `0x2a, 0x6e` | `screenshot` | Region Screenshot Tool (`Fn+F11`) |

---

## License

SPDX-License-Identifier: `GPL-2.0-or-later`
