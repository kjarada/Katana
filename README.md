# Katana

Katana is agentic CAD for survey and civil engineering. You can do everything
in its desktop window with the mouse, and an AI agent can do everything
through a command. It draws and edits plans; runs survey calculations;
builds terrain models, alignments, parcels and grading; handles buried
services by AS 5488 quality level; and reads and writes GIS files, point
clouds, DXF and IFC.

It ships as three programs:

| Program | What it is |
|---|---|
| `katana` | the desktop application |
| `katana_cli` | the same engine on the command line, for scripts and batch work |
| `katana_mcp` | the engine served to Claude (or any MCP client) over the Model Context Protocol |

Published by **Jarada**.

---

## Download

Each release on the repository's **Releases** page has one package per platform:

| Platform | File | Runs on |
|---|---|---|
| Windows, Intel/AMD 64-bit | `Katana-X.Y.Z-win64.exe` (installer) or `Katana-X.Y.Z-win64.zip` | Windows 10 and 11, x64 |
| Windows on ARM | `Katana-X.Y.Z-win-arm64.zip` | Windows 11 on ARM64 (Snapdragon and similar) |
| Linux, Intel/AMD 64-bit | `Katana-X.Y.Z-linux-x86_64.tar.gz` | glibc 2.28 or later: Ubuntu 20.04+, Debian 10+, RHEL/Rocky/Alma 8+, Fedora 29+ |
| Linux on ARM | `Katana-X.Y.Z-linux-aarch64.tar.gz` | the same distributions on 64-bit ARM (AWS Graviton, Ampere, Raspberry Pi 4/5 with a 64-bit OS) |
| macOS | `Katana-X.Y.Z-macos-arm64.tar.gz` | macOS 13 Ventura or later on Apple silicon (M1 and later) |
| all | `SHA256SUMS.txt` | the checksum of every file above |
| all, when signed | `*.asc`, `katana-signing-key.asc` | OpenPGP signatures, and the key they verify against |

Every package is self-contained. It carries Qt, GDAL, PDAL, PROJ and the
C++ runtime, so you don't need to install anything else except where a
platform below says so. Nothing is written outside the folder you unpack it
into, apart from your own projects and settings.

---

## Install on Windows

### With the installer (x64)

1. Download `Katana-X.Y.Z-win64.exe` and run it.
2. If Windows SmartScreen says "Windows protected your PC", choose
   **More info**, then **Run anyway**. This appears while the installer is
   not signed with a certificate Windows already trusts; see
   [Verify a download](#verify-a-download).
3. Choose a folder (the default is `C:\Program Files\Katana`) and finish.
   Katana appears in the Start menu. Uninstall it from
   **Settings > Apps**, where the publisher is shown as Jarada.

### From the zip (x64 or ARM64)

1. Download `Katana-X.Y.Z-win64.zip` (Intel/AMD) or
   `Katana-X.Y.Z-win-arm64.zip` (ARM). Check which one you need in
   **Settings > System > About > System type**.
2. Right-click the zip, choose **Properties**, tick **Unblock** if it is
   there, then **OK**. This stops Windows from asking about every file
   inside.
3. Right-click it again and choose **Extract All...** into a folder you own,
   for example `C:\Katana`.
4. Run `C:\Katana\Katana-X.Y.Z-win64\bin\katana.exe`. To pin it, right-click
   `katana.exe` and choose **Pin to Start**.

To remove it, delete the folder.

### From a terminal

```powershell
C:\Katana\Katana-X.Y.Z-win64\bin\katana_cli.exe --help
C:\Katana\Katana-X.Y.Z-win64\bin\katana_cli.exe -c "RECT 0,0 30,20" -c "SAVE C:\work\first"
```

---

## Install on Linux

1. Pick the package for your processor (`uname -m` prints `x86_64` or
   `aarch64`) and unpack it wherever you like:

   ```sh
   mkdir -p ~/opt
   tar -xzf Katana-X.Y.Z-linux-x86_64.tar.gz -C ~/opt
   ~/opt/Katana-X.Y.Z-linux-x86_64/bin/katana
   ```

   For everyone on the machine, unpack it into `/opt` with `sudo` instead.

2. Katana leaves the graphics driver, the font setup and X11 to your system,
   as every desktop program does. A desktop install already has them; a
   minimal or server install may need:

   | Distribution | Command |
   |---|---|
   | Debian, Ubuntu | `sudo apt install libgl1 libegl1 libopengl0 libfontconfig1 libfreetype6 libx11-6 libxcb1 libx11-xcb1 libxkbcommon-x11-0` |
   | Fedora, RHEL, Rocky, Alma | `sudo dnf install mesa-libGL mesa-libEGL libglvnd-opengl fontconfig freetype libX11 libxcb libX11-xcb libxkbcommon-x11` |
   | openSUSE | `sudo zypper install Mesa-libGL1 Mesa-libEGL1 libOpenGL0 fontconfig libfreetype6 libX11-6 libxcb1 libX11-xcb1 libxkbcommon-x11-0` |
   | Arch | `sudo pacman -S libglvnd fontconfig freetype2 libx11 libxcb libxkbcommon-x11` |

   The 3D view draws on the GPU through Vulkan when the machine has a Vulkan
   driver (Mesa's `mesa-vulkan-drivers` on Debian/Ubuntu, `mesa-vulkan-drivers`
   on Fedora). Without one it draws in software, and everything still works.

3. Optional, a menu entry. Save this as
   `~/.local/share/applications/katana.desktop`, with the path changed to
   yours:

   ```ini
   [Desktop Entry]
   Type=Application
   Name=Katana
   Comment=Survey and CAD
   Exec=/home/you/opt/Katana-X.Y.Z-linux-x86_64/bin/katana %F
   Terminal=false
   Categories=Graphics;Engineering;
   ```

4. Optional, the command line on your `PATH`:

   ```sh
   ln -s ~/opt/Katana-X.Y.Z-linux-x86_64/bin/katana_cli ~/.local/bin/katana_cli
   ```

To remove it, delete the folder (and the `.desktop` file and link if you
made them).

---

## Install on macOS

1. Download `Katana-X.Y.Z-macos-arm64.tar.gz` (Apple silicon only; check
   **Apple menu > About This Mac**: the chip is "Apple M...").
2. Unpack it into Applications or anywhere else. Double-clicking the file in
   Finder does it, or in Terminal:

   ```sh
   tar -xzf ~/Downloads/Katana-X.Y.Z-macos-arm64.tar.gz -C ~/Applications
   ```

3. If the release was not notarised by Apple, macOS says "Katana can't be
   opened because Apple cannot check it". Clear the download quarantine
   once, and it opens normally from then on:

   ```sh
   xattr -dr com.apple.quarantine ~/Applications/Katana-X.Y.Z-macos-arm64
   ```

   A notarised release (see [Verify a download](#verify-a-download)) opens
   without this step.

4. Run it:

   ```sh
   ~/Applications/Katana-X.Y.Z-macos-arm64/bin/katana
   ```

   Or double-click `bin/katana` in Finder. To keep it in the Dock, right-click
   its Dock icon while it runs and choose **Options > Keep in Dock**.

To remove it, delete the folder.

---

## Verify a download

**Checksums, on every platform.** Put the downloaded files beside
`SHA256SUMS.txt` and compare them:

```sh
sha256sum --check --ignore-missing SHA256SUMS.txt       # Linux
shasum -a 256 --check --ignore-missing SHA256SUMS.txt   # macOS
```

```powershell
Get-FileHash Katana-X.Y.Z-win64.zip -Algorithm SHA256   # Windows: compare with SHA256SUMS.txt
```

**Signatures.** Releases are signed under the publisher's name, Jarada, with
whatever signing keys the repository holds when the release is made:

| Signature | Where | How to check |
|---|---|---|
| OpenPGP | a `.asc` beside every file | `gpg --import katana-signing-key.asc`, then `gpg --verify Katana-X.Y.Z-linux-x86_64.tar.gz.asc` |
| Authenticode (Windows) | inside `katana.exe`, `katana_cli.exe`, `katana_mcp.exe` and the installer | right-click the file > **Properties** > **Digital Signatures** |
| Developer ID and notarisation (macOS) | inside every program and library | `codesign --verify --deep --strict --verbose=2 bin/katana` and `spctl --assess --type execute -v bin/katana` |

A release made without a key has only the checksums, and the platforms
behave as described in the install steps above: SmartScreen on Windows,
quarantine on macOS. How the keys are set up is in `docs/release.md`,
"Signing".

---

## First steps

**The window.** Open a project folder by passing it, or use **File > Open**.
The repository's `samples/` folder holds example projects. Open a copy,
since opening a project can update it in place.

```sh
katana path/to/a/copy/of/site_plan
```

**The command line.** It runs the same commands as the window's command bar:

```sh
katana_cli                                        # interactive; HELP lists the commands, QUIT leaves
katana_cli drawing.kcs                            # run a script
katana_cli -c "RECT 0,0 30,20" -c "SAVE site"     # one command after another
katana_cli -c "INFO parcels.geojson"              # what a GIS file holds
```

A script stops at the first command that fails and exits with 1, so it
composes with shell scripts.

**Claude.** Point an MCP client at `katana_mcp`. For example, with Claude Code:

```sh
claude mcp add katana -- /path/to/Katana/bin/katana_mcp --project /path/to/a/project
```

Then ask for the work in plain words. `docs/mcp.md` has the Claude Desktop
set-up and the tools it offers.

**Performance.** Katana uses the processor's vector instructions (AVX2 on
Intel and AMD, NEON on ARM) and draws the 3D view on the GPU: Direct3D 11 on
Windows, Vulkan on Linux, Metal on macOS. It falls back to exact
software paths where either is missing. `KATANA_SIMD=scalar` and
`KATANA_RENDERER=software` force the fallbacks, for comparison or
trouble-shooting.

---

## Build from source

The toolchains, presets and options are in `docs/building.md`; in short:

| Platform | Toolchain | Commands |
|---|---|---|
| Windows | MSYS2 UCRT64 (x64) or CLANGARM64 (ARM64) | `cmake --preset release` then `cmake --build build/release --parallel` |
| Linux (x64, ARM64) | `python3 tools/setup_linux_toolchain.py` installs GCC 16 and the libraries from conda-forge | `cmake --preset linux-release` then `cmake --build --preset linux-release --parallel` |
| macOS (Apple silicon) | the same script installs clang and the libraries | `cmake --preset macos-release` then `cmake --build --preset macos-release --parallel` |

`cmake --build <build> --target package` makes the same package a release
ships. The tests run with `ctest --preset <preset>`; `docs/testing.md` covers
them.

## Make a release

Releases are built by the **Release** workflow, which runs only when started
by hand: **Actions > Release > Run workflow**. Given a tag `vX.Y.Z` matching
`project(Katana VERSION ...)` in `CMakeLists.txt`, it builds all five
packages, starts each one from its unpacked archive, signs them, and
publishes the release. Without a tag it builds and tests them only.
`docs/release.md` has the details, including how to add the signing keys.

## Documentation

`docs/index.md` lists every document: what each covers and when to update
it.
