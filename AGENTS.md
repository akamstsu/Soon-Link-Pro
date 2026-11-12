# Developer and coding-agent guide

These instructions apply to the entire repository. Read `README.md`,
`CONTRIBUTING.md`, and the relevant documents under `docs/` before changing code.

## Build and check

Use Windows x64 with Rust MSVC, Visual Studio C++ Build Tools and Windows SDK.
Run commands at the repository root:

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace
python scripts/update-notices.py --check
cargo build --locked --release --workspace
```

To make the installer, use `scripts/build.ps1` with the pinned original driver
installer and Inno Setup 6.4.3; see `README.md`. Do not install or register audio
drivers, change system audio settings, launch DAWs, or operate the desktop just
to check a source change. Ask for authorization before these system/device checks
unless the current task already explicitly authorizes them.

## Code boundaries

- `src/main.rs`, `ui_shell.rs`, `widgets.rs`: egui interface and drawing.
- `src/audio.rs`, `asio.rs`, `process_capture.rs`: routing, hardware and capture.
- `bridge/`: shared memory, queues, host lifecycle and public layout constants.
- `driver/`: ASIO COM DLL; update it together with the application.
- `vendor/usbip-audio/`: pinned upstream code. Preserve its license, document
  modifications in `UPSTREAM.md`, and keep modifications easy to review.

## Audio and compatibility requirements

- Audio callbacks must not allocate, wait on locks, log, access files or network,
  enumerate devices, or call the UI. Prepare route plans on control threads.
- Preserve queue ownership and Acquire/Release ordering. See `docs/AUDIO_DESIGN.md`.
- Shared-memory layout changes require a protocol version change on both ends.
- Preserve saved profiles, scenes, routing, gains, hidden channels and notes.
  The JSON `card_names` field is a compatibility key for channel-group notes.
- Keep at least one virtual input/output group. Hiding a group keeps its routing.
- A note is an internal annotation displayed as `original name (note)`.
  Do not write notes to Windows device properties, USB descriptors or DAW names.
- Call UI groups “通道组”, never “卡片”. Input/output notes are independent.
- Keep the application and `patchbay_asio.dll` together. Existing registration
  identifiers and the DLL filename are installation compatibility interfaces.
- UI animation must stop when hidden; honor reduced animation and motion off.
  Do not introduce device polling, allocations or UI locks into audio callbacks.

## Contributions and release evidence

Keep changes focused. Add meaningful tests for changes to audio, migration,
serialization, IPC or concurrency. Do not add tests that only repeat constants
or implementation details of a simple cosmetic edit.

Explain behavior changes, checks performed and remaining uncertainty. A passing
build does not prove installation, DAW compatibility, listening quality, measured
round-trip latency or Live stability. Report those checks separately.

Use MIT for original contributions; preserve third-party notices and upstream
licenses. Do not add proprietary binaries, user settings, credentials, build
caches, screenshots of private work, or unsupported provenance claims. Do not
describe source scans as proof of legal clearance or independent authorship.

For dependency updates, run `python scripts/update-notices.py` (Python 3.11+),
review newly bundled licenses and upstream modifications, then run its `--check`.
The check covers Windows runtime, build and test dependencies. Existing entries
are preserved for review; remove obsolete entries only after checking all targets.

`main` pushes and pull requests build test installers. Matching `v<version>` tags
publish releases using `.github/workflows/windows-build.yml`. Validate version
numbers and test on actual hardware before tagging. Do not commit, push, create
a repository or publish a release when the current task asks for review first.
