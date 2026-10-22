# Android app

The app is a window over the Rust engine. It boots the bundled Ubuntu image on
launch and gives it a vendored Termux terminal; it does not build boot arguments
or duplicate validation.

```sh
ANDROID_HOME=$HOME/Library/Android/sdk ./build-apk.sh
```

The script uses `kotlinc`, `aapt2`, `d8` and `apksigner` directly, so it needs
no Gradle project and no network beyond the pinned Kotlin compiler. It
cross-compiles the engine with the NDK, packages:

* `lib/arm64-v8a/libdrolin.so` — the Rust CLI;
* `lib/arm64-v8a/liblinux.so` — the bionic UML kernel;
* `lib/arm64-v8a/libstub.so` — the standalone UML stub;
* `lib/arm64-v8a/libpasst.so` — static bionic passt;
* `assets/initramfs.cpio.gz` — demo guest;
* `assets/rootfs.ext4.gz` — Ubuntu 26.04.1 arm64.

`build-apk.sh` takes `PASST=...` (default `net/dist/passt-android`); the app
passes `--net --passt` to the engine. The manifest requests `INTERNET`, without
which Android refuses socket creation in the app's uid.

The names end in `.so` because `nativeLibraryDir` is the only location Android
lets an unprivileged app execute from. The files are ordinary executables.

`com/termux/terminal`, `com/termux/view` and `jni/termux.c` are vendored from
termux-app at the commit recorded in `VENDORED-TERMUX.md`. The view is given a
black background explicitly: the renderer only paints non-default cell
backgrounds, so without it the default white foreground disappears into the
light activity theme. `libtermux.so` is compiled from `termux.c` by the build
script.

`GuestSession` owns the single `TerminalSession`. It sets `TMPDIR` to the app
cache, passes `stub_exe=`, and starts the engine with `--inherit`, so UML's fd
console is the Termux pty. The image's `/sbin/init` is systemd; the console
getty autologs in root. `GuestService` is a foreground service so the guest
survives the Activity going away; a finished session (shell exit or poweroff)
stops it.
