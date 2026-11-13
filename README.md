# Meta Fixer

Open-source Windows app for using a **Rift or Quest on PC** without Meta login, Horizon Home, or the store.

**Rift CV1 · Rift S · Quest · Quest 2 · Quest 3 / 3S · Quest Pro**

| Download | File |
| --- | --- |
| Run | [`dist/MetaFixer.exe`](dist/MetaFixer.exe) |
| Install | [`dist/MetaFixer-Setup.exe`](dist/MetaFixer-Setup.exe) |

Double-click either. Windows will ask for Administrator once.

Repo: [https://origin.cursor.com/mizukos/MetaFixer](https://origin.cursor.com/mizukos/MetaFixer)

## How to use

1. Copy one of those two files out of this repo.
2. Open it as Administrator (the exe asks).
3. Leave the folder as `C:\MetaFixer\Oculus`, or **Browse** if the PC runtime is already installed.
4. Rift S / CV1: rear **USB 3** + **GPU DisplayPort**. Quest: USB Link cable (or Air Link).
5. Press **Install**, then **Start**.
6. **Fix** if USB errors, disconnects, or tracking die.

**Fix freezes the PC for a few seconds.** It restarts Meta USB devices and may power-cycle the USB root hub, which briefly disconnects everything on that controller (including mouse and keyboard). That is normal. Wait; do not mash the power button.

The in-app **How to use** and **How it works** tabs cover the same steps. Toggle **Logs** on the right to watch Install / Start / Stop / Fix.

| Button | What it does |
| --- | --- |
| **Install** | Meta Quest Link / Oculus PC runtime, drivers, `OVRService`, Steam, SteamVR, then the Meta-block. |
| **Start** | Starts `OVRService`, and SteamVR if the checkbox is on. Runs **Fix** first if USB already looks bad. |
| **Stop** | Kills SteamVR and stops `OVRService`. |
| **Fix** | Closes SteamVR, bounces Meta USB devices, power-cycles the USB root hub if ghosts remain, restarts `OVRService`. |

Headset → local Oculus runtime (`OVRService`) → SteamVR → the game.

## License

[MIT](LICENSE). USB repair uses [TrackFix](https://github.com/Kilvoctu/rift_s_no_account) by Kilvoctu (MIT). Not affiliated with Meta.

## Rebuild the Windows exe

```text
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64 nsis
cargo build --release --target x86_64-pc-windows-gnu
cp target/x86_64-pc-windows-gnu/release/metafixer.exe dist/MetaFixer.exe
makensis installer.nsi
```
