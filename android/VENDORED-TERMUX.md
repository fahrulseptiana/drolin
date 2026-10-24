# Vendored Termux terminal

Source: https://github.com/termux/termux-app
Commit: `8629e632fcb95da272221be327db653fb24befe9` (2026-09-26)

Only the two terminal libraries are vendored, under
`terminal-emulator/src/main/java/com/termux/terminal` and
`terminal-view/src/main/java/com/termux/view`, plus the view resources and
`terminal-emulator/src/main/jni/termux.c`. The rest of Termux is not included.

Both libraries derive from Android Terminal Emulator and are Apache-2.0; see
`LICENSE-TERMUX` in this directory. The app module is Apache-2.0 as a combined
work; the Rust engine and kernel patches remain GPL-2.0-only and communicate
with the app by exec, not by linking.

Local changes:
* `com.termux.view.R` imports point at the app's `id.fahrul.drolin.R`, because this is
  one APK with one resource namespace rather than two Gradle library modules.
* The four `androidx.annotation` usages are satisfied by three local annotation
  stubs, so the app has no AndroidX dependency.
* `TerminalViewClient` has a default `onSessionFinishedEnter()` hook, called by
  the input connection and key handling when Enter is pressed after the process
  exits. It exists so the host can close the app instead of just printing
  "[Process completed - press Enter]".
