#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="/home/bob/Projects/debian_asusctl"
PKG_DIR="${PROJECT_DIR}/pkg"
RUSTC_BIN="/home/bob/.cargo/bin/rustc"
if [ ! -x "${RUSTC_BIN}" ]; then
    RUSTC_BIN="$(command -v rustc)"
fi

echo "[1/5] Cleaning and preparing Debian package tree in ${PKG_DIR}..."
rm -rf "${PKG_DIR}"
mkdir -p \
    "${PKG_DIR}/DEBIAN" \
    "${PKG_DIR}/usr/bin" \
    "${PKG_DIR}/usr/sbin" \
    "${PKG_DIR}/etc/asusctl" \
    "${PKG_DIR}/lib/systemd/system" \
    "${PKG_DIR}/lib/udev/rules.d" \
    "${PKG_DIR}/lib/udev/hwdb.d" \
    "${PKG_DIR}/usr/share/doc/debian-asusctl" \
    "${PKG_DIR}/usr/share/applications"

echo "[2/5] Compiling Rust binaries (asusctl & asusctld) with ${RUSTC_BIN}..."
"${RUSTC_BIN}" -O -C strip=symbols "${PROJECT_DIR}/src/asusctl.rs" -o "${PKG_DIR}/usr/bin/asusctl"
"${RUSTC_BIN}" -O -C strip=symbols "${PROJECT_DIR}/src/asusctld.rs" -o "${PKG_DIR}/usr/sbin/asusctld"
chmod 0755 "${PKG_DIR}/usr/bin/asusctl" "${PKG_DIR}/usr/sbin/asusctld"

# Also copy compiled binaries to project root and ~/.local/bin for immediate use
cp -f "${PKG_DIR}/usr/bin/asusctl" "${PROJECT_DIR}/asusctl"
cp -f "${PKG_DIR}/usr/sbin/asusctld" "${PROJECT_DIR}/asusctld"
mkdir -p /home/bob/.local/bin
cp -f "${PKG_DIR}/usr/bin/asusctl" /home/bob/.local/bin/asusctl
chmod 0755 /home/bob/.local/bin/asusctl

echo "[3/5] Writing default configuration, systemd service, udev rules, and hwdb mappings..."

cat > "${PKG_DIR}/etc/asusctl/asusctl.conf" << 'EOF'
# /etc/asusctl/asusctl.conf — Persisted ASUS Hardware & Power State (debian_asusctl)
profile=Balanced
charge_limit=100
kbd_brightness=2
screenpad_enabled=true
screenpad_brightness=180
fnlock_enabled=true
EOF

cat > "${PKG_DIR}/etc/asusctl/keymap.conf" << 'EOF'
# /etc/asusctl/keymap.conf — Custom ASUS Hardware Keyboard Button Bindings (debian_asusctl)
# Supported built-in actions:
#   profile-next       : Cycle Power & Fan Profile (Quiet -> Balanced -> Performance -> FullSpeed)
#   profile-standard   : Cycle 3 Standard Profiles (Quiet -> Balanced -> Performance)
#   swap-monitors      : Move active window between Main Display (eDP-1) and ScreenPad Plus (DP-3)
#   screenpad-toggle   : Toggle ASUS ScreenPad Plus secondary display ON/OFF
#   screenpad-up       : Increase ScreenPad Plus brightness
#   screenpad-down     : Decrease ScreenPad Plus brightness
#   kbd-up             : Increase keyboard backlight brightness (0..3)
#   kbd-down           : Decrease keyboard backlight brightness (0..3)
#   touchpad-toggle    : Toggle internal ELAN touchpad ON/OFF
#   mic-mute           : Toggle microphone mute via PulseAudio/PipeWire/ALSA
#   screenshot         : Launch area screenshot tool (xfce4-screenshooter -r / flameshot)
#   status-notify      : Show desktop OSD notification with Fan RPMs, Profile, TDP & Battery
#   exec:<command>     : Execute any custom Linux shell command as the desktop user

# ASUS Fan / Performance Mode Toggle Key (Above Touchpad / Fn+F) (Keycodes: [203, 482, 470, 169], Scancodes: [9d, b3, 99, ae, 6c])
fan_performance=profile-next

# ASUS Task Swap Key (Swap Active Window Between Main OLED & ScreenPad Plus) (Keycodes: [202], Scancodes: [38, ffff01, 8a])
task_swap=swap-monitors

# ASUS ScreenPad Plus On/Off Toggle Key (Above Touchpad) (Keycodes: [431, 149, 183], Scancodes: [6a, 2d, 37])
screenpad_toggle=screenpad-toggle

# MyASUS / ProArt / ROG Custom Launcher Key (Fn+F12) (Keycodes: [148])
myasus_hub=status-notify

# Keyboard Backlight Brightness Up (Fn+F8) (Keycodes: [230, 185])
kbd_backlight_up=kbd-up

# Keyboard Backlight Brightness Down (Fn+F7) (Keycodes: [229, 184])
kbd_backlight_down=kbd-down

# ASUS Touchpad Enable/Disable Key (Fn+F6) (Keycodes: [530, 531])
touchpad_toggle=touchpad-toggle

# ASUS Microphone Mute Toggle Key (Fn+F9) (Keycodes: [248])
mic_mute=mic-mute

# ASUS Webcam Shield / Camera Toggle Key (Fn+F10) (Keycodes: [212])
camera_toggle=camera-notify

# ASUS Screen Snipping / Screenshot Key (Fn+F11) (Keycodes: [634])
selective_screenshot=screenshot
EOF

cat > "${PKG_DIR}/lib/systemd/system/asusctld.service" << 'EOF'
[Unit]
Description=ASUS Hardware Power Performance & Custom Button Daemon (debian_asusctl)
After=multi-user.target systemd-udev-settle.service
Wants=systemd-udev-settle.service

[Service]
Type=simple
ExecStart=/usr/sbin/asusctld
Restart=always
RestartSec=2
User=root
Group=root
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

cat > "${PKG_DIR}/lib/udev/rules.d/90-debian-asusctl.rules" << 'EOF'
# 90-debian-asusctl.rules — Allow members of sudo/plugdev/input to control ASUS power, fans, LEDs & ScreenPad
SUBSYSTEM=="leds", KERNEL=="*kbd_backlight*", ACTION=="add|change", RUN+="/bin/chgrp sudo /sys%p/brightness", RUN+="/bin/chmod 0664 /sys%p/brightness"
SUBSYSTEM=="backlight", KERNEL=="asus_screenpad", ACTION=="add|change", RUN+="/bin/chgrp sudo /sys%p/brightness /sys%p/bl_power", RUN+="/bin/chmod 0664 /sys%p/brightness /sys%p/bl_power"
SUBSYSTEM=="power_supply", KERNEL=="BAT*", ACTION=="add|change", RUN+="/bin/chgrp sudo /sys%p/charge_control_end_threshold", RUN+="/bin/chmod 0664 /sys%p/charge_control_end_threshold"
SUBSYSTEM=="platform", KERNEL=="asus-nb-wmi", ACTION=="add|change", RUN+="/bin/sh -c 'chgrp sudo /sys/devices/platform/asus-nb-wmi/cpufv /sys/devices/platform/asus-nb-wmi/throttle_thermal_policy /sys/devices/platform/asus-nb-wmi/hwmon/hwmon*/pwm1_enable /sys/module/asus_wmi/parameters/fnlock_default 2>/dev/null; chmod 0664 /sys/devices/platform/asus-nb-wmi/cpufv /sys/devices/platform/asus-nb-wmi/throttle_thermal_policy /sys/devices/platform/asus-nb-wmi/hwmon/hwmon*/pwm1_enable /sys/module/asus_wmi/parameters/fnlock_default 2>/dev/null || true'"
SUBSYSTEM=="input", ATTRS{name}=="Asus WMI hotkeys", GROUP="input", MODE="0660"
EOF

cat > "${PKG_DIR}/lib/udev/hwdb.d/90-debian-asusctl.hwdb" << 'EOF'
# 90-debian-asusctl.hwdb — Map ASUS WMI scancodes on UX582LR / ZenBook / VivoBook / ROG to Linux keycodes
evdev:name:Asus WMI hotkeys:dmi:bvn*:bvr*:bd*:svnASUSTeK*:pn*:*
 KEYBOARD_KEY_2a=selective_screenshot
 KEYBOARD_KEY_2d=displaytoggle
 KEYBOARD_KEY_37=displaytoggle
 KEYBOARD_KEY_38=prog3
 KEYBOARD_KEY_6a=displaytoggle
 KEYBOARD_KEY_6b=touchpad_toggle
 KEYBOARD_KEY_6e=selective_screenshot
 KEYBOARD_KEY_7c=micmute
 KEYBOARD_KEY_82=camera
 KEYBOARD_KEY_85=camera
 KEYBOARD_KEY_86=prog1
 KEYBOARD_KEY_99=prog4
 KEYBOARD_KEY_9d=prog4
 KEYBOARD_KEY_ae=prog4
 KEYBOARD_KEY_b3=prog4
 KEYBOARD_KEY_c4=kbdillumup
 KEYBOARD_KEY_c5=kbdillumdown
EOF

cat > "${PKG_DIR}/usr/share/doc/debian-asusctl/README.md" << 'EOF'
# debian_asusctl (v6.2.0-debian2)

Native Debian / Kali GNU/Linux hardware power performance and custom keyboard button controller for ASUS laptops (including ZenBook Pro Duo UX582LR, ZenBook, VivoBook, TUF, and ROG).

## Quick Commands
- `asusctl status` — Full hardware, power profile, CPU governor/RAPL TDP, fan RPM, keyboard LED, ScreenPad Plus & battery status
- `asusctl profile -l` — List available power profiles (`Quiet`, `Balanced`, `Performance`, `FullSpeed`)
- `asusctl profile -P Performance` — Switch to Performance mode (or `Quiet`, `Balanced`, `FullSpeed`)
- `asusctl profile -n` — Cycle to the next power profile
- `asusctl fan fullspeed` / `asusctl fan auto` — Toggle maximum fan boost vs automatic thermal fan curve
- `asusctl -k high` / `asusctl -n` / `asusctl -p` — Control keyboard backlight brightness (`0..3`)
- `asusctl screenpad toggle` / `asusctl screenpad brightness 200` — Control ASUS ScreenPad Plus secondary display
- `asusctl -c 80` — Limit battery charge ceiling to 80% for battery longevity
- `asusctl keys list` — View all 10 custom ASUS hardware keyboard buttons and their actions
- `asusctl keys bind <button> <action>` — Customize any ASUS hardware button (including `exec:<command>`)
EOF

echo "[4/5] Writing DEBIAN/control, conffiles, postinst, and prerm maintainer scripts..."

cat > "${PKG_DIR}/DEBIAN/control" << 'EOF'
Package: debian-asusctl
Version: 6.2.0-2
Section: utils
Priority: optional
Architecture: amd64
Provides: asusctl, debian_asusctl
Replaces: asusctl
Conflicts: asusctl
Depends: libc6 (>= 2.31), systemd, udev
Recommends: libnotify-bin, xdotool, wmctrl, xinput, pulseaudio-utils
Maintainer: Debian ASUS Hardware Team <root@localhost>
Description: ASUS Power Performance, Fan, ScreenPad & Custom Key Controller for Debian Linux
 Updated, self-contained version of asusctl and asusctld engineered specifically
 for Debian and Kali Linux on ASUS ZenBook Pro Duo (UX582LR), ZenBook, VivoBook,
 TUF, and ROG laptops.
 .
 Features:
  * Coordinated Power & Thermal Profiles (Quiet, Balanced, Performance, FullSpeed)
    spanning ASUS ATK cpufv, throttle_thermal_policy, ACPI platform_profile,
    ASUS hwmon fan PWM boost, Intel P-State Turbo/EPP, and Intel RAPL PL1/PL2 TDPs.
  * Direct evdev daemon (asusctld) handling all ASUS WMI custom hardware keys,
    including keycodes > 247 that X11 silently drops (Fan/Performance toggle,
    Task Swap, ScreenPad Plus toggle 0x6a, MyASUS hub, Mic Mute, Touchpad toggle,
    and Selective Screenshot).
  * Full control of Keyboard Backlight (0..3), ScreenPad Plus secondary display
    (with inverted bl_power=1 ON / bl_power=0 OFF fix & 3840x1100 DP-3 sync),
    Battery Charge Limit (20..100%), and ASUS Fn-Lock.
EOF

cat > "${PKG_DIR}/DEBIAN/conffiles" << 'EOF'
/etc/asusctl/asusctl.conf
/etc/asusctl/keymap.conf
EOF

cat > "${PKG_DIR}/DEBIAN/postinst" << 'EOF'
#!/bin/sh
set -e

if [ "$1" = "configure" ]; then
    # Reload udev hardware database & rules (registers KEYBOARD_KEY_6a=displaytoggle)
    if command -v systemd-hwdb >/dev/null 2>&1; then
        systemd-hwdb update 2>/dev/null || true
    fi
    if command -v udevadm >/dev/null 2>&1; then
        udevadm control --reload-rules 2>/dev/null || true
        udevadm trigger --subsystem-match=input --subsystem-match=leds --subsystem-match=backlight --subsystem-match=platform 2>/dev/null || true
    fi

    # Ensure ScreenPad Plus is powered ON (bl_power=1 in asus-wmi.ko) during install/upgrade
    if [ -f /sys/class/backlight/asus_screenpad/bl_power ]; then
        echo 1 > /sys/class/backlight/asus_screenpad/bl_power 2>/dev/null || true
    fi

    # Apply immediate group permissions on sysfs nodes so users in 'sudo' can use asusctl directly
    chgrp sudo /sys/class/leds/asus::kbd_backlight/brightness \
               /sys/class/backlight/asus_screenpad/brightness \
               /sys/class/backlight/asus_screenpad/bl_power \
               /sys/class/power_supply/BAT*/charge_control_end_threshold \
               /sys/devices/platform/asus-nb-wmi/cpufv \
               /sys/devices/platform/asus-nb-wmi/hwmon/hwmon*/pwm1_enable \
               /sys/module/asus_wmi/parameters/fnlock_default 2>/dev/null || true
    chmod 0664 /sys/class/leds/asus::kbd_backlight/brightness \
               /sys/class/backlight/asus_screenpad/brightness \
               /sys/class/backlight/asus_screenpad/bl_power \
               /sys/class/power_supply/BAT*/charge_control_end_threshold \
               /sys/devices/platform/asus-nb-wmi/cpufv \
               /sys/devices/platform/asus-nb-wmi/hwmon/hwmon*/pwm1_enable \
               /sys/module/asus_wmi/parameters/fnlock_default 2>/dev/null || true

    # Enable and start asusctld systemd service
    if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
        systemctl daemon-reload >/dev/null 2>&1 || true
        systemctl enable asusctld.service >/dev/null 2>&1 || true
        systemctl restart asusctld.service >/dev/null 2>&1 || true
    fi
fi

exit 0
EOF

cat > "${PKG_DIR}/DEBIAN/prerm" << 'EOF'
#!/bin/sh
set -e

if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
        systemctl stop asusctld.service >/dev/null 2>&1 || true
        systemctl disable asusctld.service >/dev/null 2>&1 || true
    fi
    rm -f /run/asusctl.sock
fi

exit 0
EOF

chmod 0755 "${PKG_DIR}/DEBIAN/postinst" "${PKG_DIR}/DEBIAN/prerm"
chmod 0644 "${PKG_DIR}/DEBIAN/control" "${PKG_DIR}/DEBIAN/conffiles"

echo "[5/5] Building standard Debian install package (.deb)..."
DEB_VERSIONED="${PROJECT_DIR}/debian_asusctl_6.2.0-2_amd64.deb"
DEB_STANDARD="${PROJECT_DIR}/debian_asusctl.deb"
DEB_WORKSPACE="/home/bob/Projects/debian_asusctl.deb"

dpkg-deb --root-owner-group --build "${PKG_DIR}" "${DEB_VERSIONED}"
cp -f "${DEB_VERSIONED}" "${DEB_STANDARD}"
cp -f "${DEB_VERSIONED}" "${DEB_WORKSPACE}"

echo "Build complete:"
ls -lh "${DEB_VERSIONED}" "${DEB_STANDARD}" "${DEB_WORKSPACE}"
