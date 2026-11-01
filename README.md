# debian_asusctl

Native Debian / Kali Linux power performance, fan, ScreenPad Plus, and custom hardware key driver for ASUS laptops (`v6.2.0-3`).

## Install & Confirm

```bash
sudo dpkg -i debian_asusctl.deb
sudo asusctl confirm
```

> **Automatic Rollback & Crash Protection:** Pre-installation hardware and driver state is saved to `/etc/asusctl/backup_state.conf`. If `asusctl confirm` is not run within the confirmation window after installation, or if the daemon crashes, `debian_asusctl` automatically reverts all hardware and driver settings to the previous system state. You can also manually revert at any time with `sudo asusctl revert`.

## CLI Reference

| Command | Description |
| :--- | :--- |
| `asusctl status` | Show hardware, profile, fan RPM, LED, ScreenPad, and battery status |
| `asusctl confirm` | Confirm active driver configuration (disarms auto-revert watchdog) |
| `asusctl revert` | Revert all hardware and driver settings to previous system backup |
| `asusctl backup` | Save a fresh backup snapshot to `/etc/asusctl/backup_state.conf` |
| `asusctl profile -l` | List profiles (`Quiet`, `Balanced`, `Performance`, `FullSpeed`) |
| `asusctl profile -P <Profile>` | Set active power/thermal profile |
| `asusctl profile -n` | Cycle to next power profile |
| `asusctl fan [status\|auto\|fullspeed]` | Query fan RPMs or set PWM mode (`auto` curve vs `fullspeed`) |
| `asusctl -k [off\|low\|med\|high]` | Set keyboard backlight brightness (`0..3`) |
| `asusctl -n` / `asusctl -p` | Step keyboard backlight up / down |
| `asusctl screenpad [on\|off\|toggle]` | Power ScreenPad Plus (`DP-3`) on, off, or toggle |
| `asusctl screenpad brightness <0..255>` | Set ScreenPad Plus brightness |
| `asusctl -c <20..100>` / `asusctl -o` | Set battery charge limit (`20..100%`) or `100%` |
| `asusctl fnlock [on\|off\|toggle]` | Set or toggle ASUS Fn-Lock |
| `asusctl keys [list\|bind\|trigger\|reset]` | List, rebind (`exec:<cmd>`), trigger, or reset custom ASUS keys |

## Build & Sandbox Test

```bash
./build_deb.sh
./tests/sandbox_test.sh
```
