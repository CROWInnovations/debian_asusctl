#!/usr/bin/env bash
# Comprehensive Sandbox Test Suite for debian_asusctl (v6.2.0-debian3)
# Executes entirely inside an isolated ASUSCTL_SANDBOX_DIR without touching real hardware.
set -euo pipefail

PROJECT_DIR="/home/bob/Projects/debian_asusctl"
ASUSCTL_BIN="${PROJECT_DIR}/asusctl"
ASUSCTLD_BIN="${PROJECT_DIR}/asusctld"
SANDBOX="/tmp/asusctl_sandbox_test_$$"

cleanup() {
    if [ -n "${DAEMON_PID:-}" ] && kill -0 "${DAEMON_PID}" 2>/dev/null; then
        kill "${DAEMON_PID}" 2>/dev/null || true
    fi
    rm -rf "${SANDBOX}"
}
trap cleanup EXIT

export ASUSCTL_SANDBOX_DIR="${SANDBOX}"

init_sandbox() {
    rm -rf "${SANDBOX}"
    mkdir -p \
        "${SANDBOX}/etc/asusctl" \
        "${SANDBOX}/run" \
        "${SANDBOX}/var/lib/asusctl/previous_driver_backup" \
        "${SANDBOX}/usr/bin" \
        "${SANDBOX}/sys/class/dmi/id" \
        "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5" \
        "${SANDBOX}/sys/firmware/acpi" \
        "${SANDBOX}/sys/devices/system/cpu/intel_pstate" \
        "${SANDBOX}/sys/devices/system/cpu/cpu0/cpufreq" \
        "${SANDBOX}/sys/devices/system/cpu/cpu1/cpufreq" \
        "${SANDBOX}/sys/class/powercap/intel-rapl:0" \
        "${SANDBOX}/sys/class/leds/asus::kbd_backlight" \
        "${SANDBOX}/sys/class/backlight/asus_screenpad" \
        "${SANDBOX}/sys/class/backlight/intel_backlight" \
        "${SANDBOX}/sys/class/power_supply/BAT0" \
        "${SANDBOX}/sys/module/asus_wmi/parameters"

    # Initial simulated hardware state (representing pre-install system state)
    echo "ZenBook Pro Duo UX582LR_UX582LR" > "${SANDBOX}/sys/class/dmi/id/product_name"
    echo "1.0" > "${SANDBOX}/sys/class/dmi/id/board_version"
    echo "UX582LR.306" > "${SANDBOX}/sys/class/dmi/id/bios_version"
    echo "0" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv"
    echo "0" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/throttle_thermal_policy"
    echo "balanced" > "${SANDBOX}/sys/firmware/acpi/platform_profile"
    echo "low-power balanced performance" > "${SANDBOX}/sys/firmware/acpi/platform_profile_choices"
    echo "2" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/pwm1_enable"
    echo "2400" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/fan1_input"
    echo "CPU Fan" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/fan1_label"
    echo "2500" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/fan2_input"
    echo "GPU Fan" > "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/fan2_label"
    echo "0" > "${SANDBOX}/sys/devices/system/cpu/intel_pstate/no_turbo"
    echo "100" > "${SANDBOX}/sys/devices/system/cpu/intel_pstate/max_perf_pct"
    for cpu in cpu0 cpu1; do
        echo "powersave" > "${SANDBOX}/sys/devices/system/cpu/${cpu}/cpufreq/scaling_governor"
        echo "balance_performance" > "${SANDBOX}/sys/devices/system/cpu/${cpu}/cpufreq/energy_performance_preference"
    done
    echo "45000000" > "${SANDBOX}/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw"
    echo "70000000" > "${SANDBOX}/sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw"
    echo "2" > "${SANDBOX}/sys/class/leds/asus::kbd_backlight/brightness"
    echo "3" > "${SANDBOX}/sys/class/leds/asus::kbd_backlight/max_brightness"
    echo "1" > "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power"
    echo "200" > "${SANDBOX}/sys/class/backlight/asus_screenpad/brightness"
    echo "255" > "${SANDBOX}/sys/class/backlight/asus_screenpad/max_brightness"
    echo "264" > "${SANDBOX}/sys/class/backlight/intel_backlight/brightness"
    echo "528" > "${SANDBOX}/sys/class/backlight/intel_backlight/max_brightness"
    echo "100" > "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold"
    echo "85" > "${SANDBOX}/sys/class/power_supply/BAT0/capacity"
    echo "Discharging" > "${SANDBOX}/sys/class/power_supply/BAT0/status"
    echo "Y" > "${SANDBOX}/sys/module/asus_wmi/parameters/fnlock_default"
}

assert_eq() {
    local actual="$1"
    local expected="$2"
    local msg="$3"
    if [ "${actual}" != "${expected}" ]; then
        echo "[FAIL] ${msg}: expected '${expected}', got '${actual}'" >&2
        exit 1
    fi
    echo "[PASS] ${msg} (${actual})"
}

assert_contains() {
    local haystack="$1"
    local needle="$2"
    local msg="$3"
    if [[ "${haystack}" != *"${needle}"* ]]; then
        echo "[FAIL] ${msg}: output did not contain '${needle}'" >&2
        echo "Actual output: ${haystack}" >&2
        exit 1
    fi
    echo "[PASS] ${msg}"
}

echo "====================================================================="
echo " Starting debian_asusctl Isolated Sandbox Verification Suite"
echo " Sandbox Root: ${SANDBOX}"
echo "====================================================================="

init_sandbox

# 1. Version, Help, Status, and Backup Snapshot
out_ver="$("${ASUSCTL_BIN}" --version)"
assert_contains "${out_ver}" "6.2.0-debian3" "CLI --version switch"

out_help="$("${ASUSCTL_BIN}" --help)"
assert_contains "${out_help}" "confirm" "CLI --help includes confirm subcommand"
assert_contains "${out_help}" "revert" "CLI --help includes revert subcommand"

out_backup="$("${ASUSCTL_BIN}" backup)"
assert_contains "${out_backup}" "Saved current hardware/driver state snapshot" "CLI backup snapshot creation"

out_status="$("${ASUSCTL_BIN}" -s)"
assert_contains "${out_status}" "ZenBook Pro Duo UX582LR_UX582LR" "CLI -s / status report"
assert_contains "${out_status}" "ScreenPad Plus     : ENABLED (Brightness: 200/255)" "CLI status reports ScreenPad ENABLED when bl_power=1"

# 2. Power Profiles (-l, -p, -P Quiet/Balanced/Performance/FullSpeed, -n)
out_prof_list="$("${ASUSCTL_BIN}" profile -l)"
assert_contains "${out_prof_list}" "* Balanced" "CLI profile -l shows initial Balanced profile"

"${ASUSCTL_BIN}" profile -P Quiet >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "2" "Profile Quiet sets cpufv=2"
assert_eq "$(cat "${SANDBOX}/sys/devices/system/cpu/intel_pstate/no_turbo")" "1" "Profile Quiet disables turbo (no_turbo=1)"
assert_eq "$(cat "${SANDBOX}/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw")" "28000000" "Profile Quiet sets PL1=28W"

"${ASUSCTL_BIN}" profile -P Performance >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "1" "Profile Performance sets cpufv=1"
assert_eq "$(cat "${SANDBOX}/sys/devices/system/cpu/intel_pstate/no_turbo")" "0" "Profile Performance enables turbo (no_turbo=0)"
assert_eq "$(cat "${SANDBOX}/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")" "performance" "Profile Performance sets scaling_governor=performance"
assert_eq "$(cat "${SANDBOX}/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw")" "70000000" "Profile Performance sets PL1=70W"

"${ASUSCTL_BIN}" profile -P FullSpeed >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/pwm1_enable")" "0" "Profile FullSpeed sets pwm1_enable=0 (Max Fan Boost)"
assert_eq "$(cat "${SANDBOX}/sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw")" "70000000" "Profile FullSpeed sets PL1=70W"

"${ASUSCTL_BIN}" profile -n >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "2" "Profile -n cycles FullSpeed -> Quiet"

# 3. Fan Control Switches (fan status, fan fullspeed, fan auto)
"${ASUSCTL_BIN}" fan fullspeed >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/pwm1_enable")" "0" "fan fullspeed sets pwm1_enable=0"
"${ASUSCTL_BIN}" fan auto >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/pwm1_enable")" "2" "fan auto sets pwm1_enable=2"
out_fan="$("${ASUSCTL_BIN}" fan status)"
assert_contains "${out_fan}" "2400 RPM" "fan status reports RPM"

# 4. Keyboard Backlight LED Switches (-k, -n, -p, led)
"${ASUSCTL_BIN}" -k off >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/leds/asus::kbd_backlight/brightness")" "0" "-k off sets kbd_backlight=0"
"${ASUSCTL_BIN}" -n >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/leds/asus::kbd_backlight/brightness")" "1" "-n increments kbd_backlight=1"
"${ASUSCTL_BIN}" -k high >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/leds/asus::kbd_backlight/brightness")" "3" "-k high sets kbd_backlight=3"
"${ASUSCTL_BIN}" -p >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/leds/asus::kbd_backlight/brightness")" "2" "-p decrements kbd_backlight=2"

# 5. ScreenPad Plus Switches (off, on, toggle, up, down, brightness, and bl_power polarity + intel_backlight guard)
"${ASUSCTL_BIN}" screenpad off >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "0" "screenpad off writes bl_power=0 (OFF in Linux 7.1 asus-wmi.ko)"
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/intel_backlight/brightness")" "264" "intel_backlight brightness preserved at 264 across screenpad off"

"${ASUSCTL_BIN}" screenpad on >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "1" "screenpad on writes bl_power=1 (ON in Linux 7.1 asus-wmi.ko)"

"${ASUSCTL_BIN}" screenpad brightness 200 >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "1" "screenpad brightness 200 keeps bl_power=1 (ON)"
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/brightness")" "200" "screenpad brightness 200 writes brightness=200"
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/intel_backlight/brightness")" "264" "intel_backlight brightness preserved at 264 across screenpad brightness 200"

"${ASUSCTL_BIN}" screenpad up >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/brightness")" "225" "screenpad up steps brightness 200 -> 225"
"${ASUSCTL_BIN}" screenpad down >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/brightness")" "200" "screenpad down steps brightness 225 -> 200"
"${ASUSCTL_BIN}" screenpad toggle >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "0" "screenpad toggle turns ON -> OFF (bl_power=0)"
"${ASUSCTL_BIN}" screenpad toggle >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "1" "screenpad toggle turns OFF -> ON (bl_power=1)"

# 6. Battery Charge Limit & Fn-Lock Switches (-c, -o, battery, fnlock)
"${ASUSCTL_BIN}" -c 80 >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "80" "-c 80 sets battery limit to 80%"
"${ASUSCTL_BIN}" -o >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "100" "-o sets one-shot battery limit to 100%"
"${ASUSCTL_BIN}" battery 85 >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "85" "battery 85 sets battery limit to 85%"

"${ASUSCTL_BIN}" fnlock off >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/module/asus_wmi/parameters/fnlock_default")" "N" "fnlock off sets fnlock_default=N"
"${ASUSCTL_BIN}" fnlock toggle >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/module/asus_wmi/parameters/fnlock_default")" "Y" "fnlock toggle sets fnlock_default=Y"

# 7. Custom Keys & Security Hardening Verification
out_keys="$("${ASUSCTL_BIN}" keys list)"
assert_contains "${out_keys}" "screenpad_toggle" "keys list displays screenpad_toggle"
"${ASUSCTL_BIN}" keys bind myasus_hub "exec:echo hello_sandbox" >/dev/null
out_trig="$("${ASUSCTL_BIN}" keys trigger myasus_hub)"
assert_contains "${out_trig}" "Executed custom command: echo hello_sandbox" "keys bind & trigger custom command"

# Verify invalid/malicious action is rejected
if "${ASUSCTL_BIN}" keys bind myasus_hub "invalid-malicious-action" 2>/dev/null; then
    echo "[FAIL] Security check: invalid action should have been rejected" >&2
    exit 1
else
    echo "[PASS] Security check: rejected invalid action 'invalid-malicious-action'"
fi

# Verify world-writable keymap.conf is ignored by load_key_bindings()
chmod 0666 "${SANDBOX}/etc/asusctl/keymap.conf"
out_insecure="$("${ASUSCTL_BIN}" keys list)"
assert_contains "${out_insecure}" "status-notify" "Security check: world-writable keymap.conf ignored in favor of safe defaults"
"${ASUSCTL_BIN}" keys reset >/dev/null

# 8. Manual Revert (asusctl revert) Verification
# Mutate multiple hardware settings away from the initial backup snapshot
"${ASUSCTL_BIN}" profile -P FullSpeed >/dev/null
"${ASUSCTL_BIN}" -k off >/dev/null
"${ASUSCTL_BIN}" screenpad off >/dev/null
"${ASUSCTL_BIN}" -c 60 >/dev/null
"${ASUSCTL_BIN}" fnlock off >/dev/null

"${ASUSCTL_BIN}" revert >/dev/null
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "0" "revert restored cpufv=0"
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/pwm1_enable")" "2" "revert restored pwm1_enable=2"
assert_eq "$(cat "${SANDBOX}/sys/class/leds/asus::kbd_backlight/brightness")" "2" "revert restored kbd_brightness=2"
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "1" "revert restored screenpad bl_power=1 (ON)"
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/brightness")" "200" "revert restored screenpad brightness=200"
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "100" "revert restored battery charge_limit=100%"
assert_eq "$(cat "${SANDBOX}/sys/module/asus_wmi/parameters/fnlock_default")" "Y" "revert restored fnlock=Y"

# 9. Automatic Crash Rollback (--simulate-crash panic hook) Verification
"${ASUSCTL_BIN}" profile -P FullSpeed >/dev/null
"${ASUSCTL_BIN}" screenpad off >/dev/null
"${ASUSCTL_BIN}" -c 70 >/dev/null
set +e
"${ASUSCTL_BIN}" --simulate-crash >/dev/null 2>&1
crash_rc=$?
set -e
if [ "${crash_rc}" -eq 0 ]; then
    echo "[FAIL] --simulate-crash should exit non-zero" >&2
    exit 1
fi
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "0" "Crash handler auto-reverted cpufv=0"
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/hwmon/hwmon5/pwm1_enable")" "2" "Crash handler auto-reverted pwm1_enable=2"
assert_eq "$(cat "${SANDBOX}/sys/class/backlight/asus_screenpad/bl_power")" "1" "Crash handler auto-reverted screenpad bl_power=1 (ON)"
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "100" "Crash handler auto-reverted charge_limit=100%"

# 10. Unconfirmed Installation Watchdog Auto-Revert Test (including previous driver binary restoration)
echo "#!/bin/sh" > "${SANDBOX}/var/lib/asusctl/previous_driver_backup/asusctl.orig"
echo "echo previous_driver_v1" >> "${SANDBOX}/var/lib/asusctl/previous_driver_backup/asusctl.orig"
chmod 0755 "${SANDBOX}/var/lib/asusctl/previous_driver_backup/asusctl.orig"

# Mutate state and start asusctld with pending confirmation and a 2-second timeout
"${ASUSCTL_BIN}" profile -P Performance >/dev/null
"${ASUSCTL_BIN}" -c 75 >/dev/null
echo "pending" > "${SANDBOX}/run/asusctl.pending_confirm"
ASUSCTL_CONFIRM_TIMEOUT_SECS=2 "${ASUSCTLD_BIN}" >/dev/null 2>&1 &
DAEMON_PID=$!
sleep 3
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "0" "Watchdog timeout auto-reverted cpufv=0 when unconfirmed"
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "100" "Watchdog timeout auto-reverted charge_limit=100% when unconfirmed"
assert_eq "$("${SANDBOX}/usr/bin/asusctl")" "previous_driver_v1" "Watchdog timeout restored previous driver binary (asusctl.orig)"

# 11. User Confirmation Test (asusctl confirm disarms watchdog and preserves new settings)
rm -f "${SANDBOX}/var/lib/asusctl/previous_driver_backup/asusctl.orig"
echo "pending" > "${SANDBOX}/run/asusctl.pending_confirm"
ASUSCTL_CONFIRM_TIMEOUT_SECS=3 "${ASUSCTLD_BIN}" >/dev/null 2>&1 &
DAEMON_PID=$!
sleep 0.5
"${ASUSCTL_BIN}" profile -P Performance >/dev/null
"${ASUSCTL_BIN}" -c 80 >/dev/null
"${ASUSCTL_BIN}" confirm >/dev/null
sleep 3
assert_eq "$(cat "${SANDBOX}/sys/devices/platform/asus-nb-wmi/cpufv")" "1" "Confirmed driver kept Performance profile (cpufv=1) after watchdog window"
assert_eq "$(cat "${SANDBOX}/sys/class/power_supply/BAT0/charge_control_end_threshold")" "80" "Confirmed driver kept charge_limit=80% after watchdog window"
kill "${DAEMON_PID}" 2>/dev/null || true

echo "====================================================================="
echo " ALL SANDBOX TESTS PASSED SUCCESSFULLY"
echo "====================================================================="
