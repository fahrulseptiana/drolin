package dev.drolin

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.Gravity
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import com.termux.view.TerminalView

class MainActivity : Activity() {
    private lateinit var terminal: TerminalView
    private lateinit var status: TextView
    private lateinit var client: TerminalClient

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
        }
        status = TextView(this).apply {
            text = "idle"
            textSize = 12f
            setPadding(24, 12, 24, 12)
        }
        terminal = TerminalView(this, null).apply {
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                0,
                1f,
            )
            // The renderer only paints non-default cell backgrounds; the view
            // itself must supply the terminal's default background. Termux's
            // dark theme does this for its own activity.
            setBackgroundColor(0xFF000000.toInt())
            isFocusable = true
            isFocusableInTouchMode = true
        }
        client = TerminalClient(this, terminal)
        terminal.setTextSize(14)

        val boot = Button(this).apply {
            text = "Boot Ubuntu"
            setOnClickListener { bootGuest() }
        }
        val stop = Button(this).apply {
            text = "Stop"
            setOnClickListener {
                GuestSession.stop()
                stopService(Intent(this@MainActivity, GuestService::class.java))
                status.text = "stopped"
            }
        }
        val toolbar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            addView(boot)
            addView(stop)
        }

        root.addView(toolbar)
        root.addView(status)
        root.addView(terminal)
        setContentView(root)

        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }

        GuestSession.attach(this, terminal, client)
        if (!GuestSession.isRunning) {
            bootGuest()
        }
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
            status.text = text
            if (text.startsWith("guest") || text.startsWith("booting")) {
                val intent = Intent(this, GuestService::class.java)
                    .setAction(GuestService.ACTION_START)
                if (Build.VERSION.SDK_INT >= 26) startForegroundService(intent) else startService(intent)
            }
        }
    }

    fun onSessionFinished() {
        GuestSession.onSessionFinished()
        runOnUiThread {
            status.text = "guest stopped"
            stopService(Intent(this, GuestService::class.java))
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
}
