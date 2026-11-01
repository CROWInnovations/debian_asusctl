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
    "${PKG_DIR}/etc/xdg/autostart" \
    "${PKG_DIR}/var/lib/asusctl/previous_driver_backup" \
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
screenpad_brightness=200
fnlock_enabled=true
confirmed=true
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
ExecStopPost=/bin/sh -c 'if [ "$EXIT_STATUS" != "0" ] && [ -n "$EXIT_STATUS" ]; then /usr/bin/asusctl revert --on-crash; fi'
Restart=on-failure
RestartSec=3
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

cp -f "${PROJECT_DIR}/README.md" "${PKG_DIR}/usr/share/doc/debian-asusctl/README.md"

cat > "${PKG_DIR}/etc/xdg/autostart/debian-asusctl-cursor-guard.desktop" << 'EOF'
[Desktop Entry]
Type=Application
Name=ASUS ScreenPad Hardware Cursor Guard
Comment=Ensure DP-3 uses 1x1 scale so Intel GPU Hardware Cursor remains active without flicker
Exec=/bin/sh -c "sleep 2; if xrandr --listmonitors 2>/dev/null | grep -q 'DP-3'; then edp_h=$(xrandr --listmonitors 2>/dev/null | awk '/eDP-1/{print $3}' | awk -F'[x/+]' '{print $3}'); edp_h=${edp_h:-1440}; xrandr --output DP-3 --mode 3840x1100 --pos 0x${edp_h} --scale 1x1 2>/dev/null || true; fi"
Hidden=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF

echo "[4/5] Writing DEBIAN/control, conffiles, preinst, postinst, and prerm maintainer scripts..."

cat > "${PKG_DIR}/DEBIAN/control" << 'EOF'
Package: debian-asusctl
Version: 6.2.0-3
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
 Self-contained asusctl and asusctld driver for Debian and Kali Linux on ASUS
 ZenBook Pro Duo (UX582LR), ZenBook, VivoBook, TUF, and ROG laptops.
 Includes automatic backup, user confirmation watchdog, and crash rollback to the
 previous system driver/hardware state.
EOF

cat > "${PKG_DIR}/DEBIAN/conffiles" << 'EOF'
/etc/asusctl/asusctl.conf
/etc/asusctl/keymap.conf
EOF

cat > "${PKG_DIR}/DEBIAN/preinst" << 'EOF'
#!/bin/sh
set -e

mkdir -p /etc/asusctl /var/lib/asusctl/previous_driver_backup

# Back up any pre-existing non-debian_asusctl /usr/bin/asusctl binary
if [ -x /usr/bin/asusctl ] && [ ! -f /var/lib/asusctl/previous_driver_backup/asusctl.orig ]; then
    if ! /usr/bin/asusctl --version 2>/dev/null | grep -q "debian"; then
        cp -pf /usr/bin/asusctl /var/lib/asusctl/previous_driver_backup/asusctl.orig 2>/dev/null || true
    fi
fi

# Snapshot current kernel/hardware state prior to installing or upgrading driver
if [ ! -f /etc/asusctl/backup_state.conf ]; then
    {
        echo "# /etc/asusctl/backup_state.conf — Pre-install system driver/hardware snapshot"
        [ -f /sys/devices/platform/asus-nb-wmi/cpufv ] && echo "cpufv=$(cat /sys/devices/platform/asus-nb-wmi/cpufv 2>/dev/null || echo 0)"
        for pwm in /sys/devices/platform/asus-nb-wmi/hwmon/hwmon*/pwm1_enable; do
            [ -f "$pwm" ] && echo "pwm1_enable=$(cat "$pwm" 2>/dev/null || echo 2)" && break
        done
        [ -f /sys/devices/system/cpu/intel_pstate/no_turbo ] && echo "no_turbo=$(cat /sys/devices/system/cpu/intel_pstate/no_turbo 2>/dev/null || echo 0)"
        [ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ] && echo "governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo powersave)"
        [ -f /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference ] && echo "epp=$(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null || echo balance_performance)"
        [ -f /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw ] && echo "pl1_uw=$(cat /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw 2>/dev/null || echo 45000000)"
        [ -f /sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw ] && echo "pl2_uw=$(cat /sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw 2>/dev/null || echo 70000000)"
        [ -f /sys/class/leds/asus::kbd_backlight/brightness ] && echo "kbd_brightness=$(cat /sys/class/leds/asus::kbd_backlight/brightness 2>/dev/null || echo 2)"
        [ -f /sys/class/backlight/asus_screenpad/bl_power ] && echo "screenpad_bl_power=$(cat /sys/class/backlight/asus_screenpad/bl_power 2>/dev/null || echo 1)"
        [ -f /sys/class/backlight/asus_screenpad/brightness ] && echo "screenpad_brightness=$(cat /sys/class/backlight/asus_screenpad/brightness 2>/dev/null || echo 200)"
        [ -f /sys/class/backlight/intel_backlight/brightness ] && echo "intel_backlight_brightness=$(cat /sys/class/backlight/intel_backlight/brightness 2>/dev/null || echo 264)"
        for bat in /sys/class/power_supply/BAT*/charge_control_end_threshold; do
            [ -f "$bat" ] && echo "charge_limit=$(cat "$bat" 2>/dev/null || echo 100)" && break
        done
        [ -f /sys/module/asus_wmi/parameters/fnlock_default ] && echo "fnlock=$(cat /sys/module/asus_wmi/parameters/fnlock_default 2>/dev/null || echo Y)"
    } > /etc/asusctl/backup_state.conf
    chmod 0644 /etc/asusctl/backup_state.conf
fi

exit 0
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

    chgrp sudo /etc/asusctl /etc/asusctl/asusctl.conf /etc/asusctl/backup_state.conf 2>/dev/null || true
    chmod 0775 /etc/asusctl 2>/dev/null || true
    chmod 0664 /etc/asusctl/asusctl.conf /etc/asusctl/backup_state.conf 2>/dev/null || true
    chmod 0644 /etc/asusctl/keymap.conf 2>/dev/null || true
    chown root:root /etc/asusctl/keymap.conf 2>/dev/null || true

    # Arm confirmation watchdog unless already confirmed or ASUSCTL_AUTO_CONFIRM=1
    if [ "${ASUSCTL_AUTO_CONFIRM:-0}" = "1" ]; then
        /usr/bin/asusctl confirm >/dev/null 2>&1 || true
    elif [ "${ASUSCTL_REQUIRE_CONFIRM:-0}" = "1" ] || ! grep -q "^confirmed=true" /etc/asusctl/asusctl.conf 2>/dev/null; then
        echo "pending" > /run/asusctl.pending_confirm 2>/dev/null || true
    fi

    # Enable and start asusctld systemd service
    if command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ]; then
        systemctl daemon-reload >/dev/null 2>&1 || true
        systemctl enable asusctld.service >/dev/null 2>&1 || true
        systemctl restart asusctld.service >/dev/null 2>&1 || true
    fi

    # Interactive confirmation prompt when a terminal is attached and confirmation is pending
    if [ -f /run/asusctl.pending_confirm ] && [ -t 0 ]; then
        printf "\n[debian_asusctl] Confirm driver installation and display/power state? [Y/n] (auto-reverts in 90s if unconfirmed): "
        if read -r -t 30 ans; then
            case "$ans" in
                [nN]*)
                    echo "[debian_asusctl] Reverting to previous system driver/hardware state..."
                    /usr/bin/asusctl revert || true
                    ;;
                *)
                    /usr/bin/asusctl confirm || true
                    ;;
            esac
        else
            echo "\n[debian_asusctl] No input received yet. Run 'sudo asusctl confirm' within 90s to keep changes, or it will auto-revert."
        fi
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
    /usr/bin/asusctl revert >/dev/null 2>&1 || true
    rm -f /run/asusctl.sock /run/asusctl.pending_confirm
fi

exit 0
EOF

chmod 0755 "${PKG_DIR}/DEBIAN/preinst" "${PKG_DIR}/DEBIAN/postinst" "${PKG_DIR}/DEBIAN/prerm"
chmod 0644 "${PKG_DIR}/DEBIAN/control" "${PKG_DIR}/DEBIAN/conffiles"

echo "[5/5] Building standard Debian install package (.deb)..."
DEB_VERSIONED="${PROJECT_DIR}/debian_asusctl_6.2.0-3_amd64.deb"
DEB_STANDARD="${PROJECT_DIR}/debian_asusctl.deb"
DEB_WORKSPACE="/home/bob/Projects/debian_asusctl.deb"

dpkg-deb --root-owner-group --build "${PKG_DIR}" "${DEB_VERSIONED}"
cp -f "${DEB_VERSIONED}" "${DEB_STANDARD}"
cp -f "${DEB_VERSIONED}" "${DEB_WORKSPACE}"

echo "Build complete:"
ls -lh "${DEB_VERSIONED}" "${DEB_STANDARD}" "${DEB_WORKSPACE}"
