package dev.drolin

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.TextView
import com.termux.view.TerminalView

class MainActivity : Activity() {
    private lateinit var root: LinearLayout
    private lateinit var terminal: TerminalView
    private lateinit var status: TextView
    private lateinit var client: TerminalClient

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        configureWindow()

        root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(Color.BLACK)
        }
        status = TextView(this).apply {
            text = ""
            visibility = View.GONE
            textSize = 12f
            setTextColor(Color.WHITE)
            setBackgroundColor(Color.BLACK)
            setPadding(24, 10, 24, 10)
            setOnClickListener {
                if (!GuestSession.isRunning) bootGuest()
            }
        }
        terminal = TerminalView(this, null).apply {
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                0,
                1f,
            )
            // The renderer paints only non-default cell backgrounds; the view
            // supplies the terminal's default black background.
            setBackgroundColor(Color.BLACK)
            isFocusable = true
            isFocusableInTouchMode = true
        }
        client = TerminalClient(this, terminal)
        terminal.setTextSize(14)

        root.addView(status, LinearLayout.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
        root.addView(terminal)
        setContentView(root)

        // Safe area: the window is edge-to-edge and black behind the bars; the
        // content starts below the status bar/cutout and above the nav bar.
        root.setOnApplyWindowInsetsListener { view, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                // The IME is not part of systemBars. Without it the terminal
                // keeps its full height behind the keyboard and the prompt in a
                // long scrollback is hidden. adjustResize + this inset keeps the
                // visible terminal above the IME.
                val bars = insets.getInsets(
                    WindowInsets.Type.systemBars() or
                        WindowInsets.Type.displayCutout() or
                        WindowInsets.Type.ime(),
                )
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            } else {
                @Suppress("DEPRECATION")
                view.setPadding(
                    insets.systemWindowInsetLeft,
                    insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight,
                    insets.systemWindowInsetBottom,
                )
            }
            insets
        }
        root.requestApplyInsets()

        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }

        GuestSession.attach(this, terminal, client)
        if (!GuestSession.isRunning) bootGuest()
    }

    override fun onResume() {
        super.onResume()
        GuestSession.attach(this, terminal, client)
    }

    override fun onDestroy() {
        GuestSession.detach()
        super.onDestroy()
    }

    fun onSessionState(text: String) {
        runOnUiThread {
            val startup = text.startsWith("preparing") ||
                text.startsWith("booting") ||
                text.startsWith("guest running") ||
                text.startsWith("guest already running")
            if (startup) {
                status.visibility = View.GONE
            } else {
                status.text = if (text == "guest stopped") "guest stopped (tap to boot)" else text
                status.visibility = View.VISIBLE
            }
            if (text.startsWith("guest running")) showKeyboard()
        }
    }

    fun onSessionFinished() {
        GuestSession.onSessionFinished()
        runOnUiThread {
            status.text = "guest stopped (tap to boot)"
            status.visibility = View.VISIBLE
            stopService(Intent(this, GuestService::class.java))
        }
    }

    fun showKeyboard() {
        terminal.post {
            terminal.requestFocus()
            val imm = getSystemService(android.view.inputmethod.InputMethodManager::class.java)
            imm.showSoftInput(terminal, android.view.inputmethod.InputMethodManager.SHOW_IMPLICIT)
        }
    }

    private fun bootGuest() {
        Thread({
            try {
                GuestSession.start(this, terminal, client)
            } catch (error: Exception) {
                onSessionState("boot failed: ${error.message}")
            }
        }, "drolin-boot").start()
    }

    private fun configureWindow() {
        window.statusBarColor = Color.BLACK
        window.navigationBarColor = Color.BLACK
        window.decorView.setBackgroundColor(Color.BLACK)
        if (Build.VERSION.SDK_INT >= 28) {
            window.attributes = window.attributes.apply {
                layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES
            }
        }
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
            window.insetsController?.setSystemBarsAppearance(
                0,
                android.view.WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
                    android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS,
            )
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility =
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE or
                    View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                    View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        }
    }
}
