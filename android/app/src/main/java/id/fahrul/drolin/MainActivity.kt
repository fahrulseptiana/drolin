package id.fahrul.drolin

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.Spinner
import android.widget.TextView
import android.widget.Toast
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

    fun showTerminalMenu() {
        // A PopupMenu anchored to the full-screen TerminalView is placed under
        // the navigation bar on edge-to-edge devices. A dialog list is the same
        // menu semantics without the clipping.
        AlertDialog.Builder(this)
            .setTitle("More")
            .setItems(arrayOf("Settings", "Port Forwarding")) { _, which ->
                if (which == 0) showSettings() else showPortForwarding()
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun showPortForwarding() {
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(36, 12, 36, 12)
        }
        val list = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        val protocol = Spinner(this).apply {
            adapter = ArrayAdapter(
                this@MainActivity,
                android.R.layout.simple_spinner_dropdown_item,
                listOf("TCP", "UDP"),
            )
        }
        val host = EditText(this).apply {
            hint = "host port (>= 1024)"
            inputType = InputType.TYPE_CLASS_NUMBER
            filters = arrayOf(android.text.InputFilter.LengthFilter(5))
        }
        val guest = EditText(this).apply {
            hint = "guest port"
            inputType = InputType.TYPE_CLASS_NUMBER
            filters = arrayOf(android.text.InputFilter.LengthFilter(5))
        }
        fun refresh() {
            list.removeAllViews()
            val forwards = GuestSettings.forwards(this)
            if (forwards.isEmpty()) {
                list.addView(TextView(this).apply { text = "No forwards" })
            }
            for (forward in forwards) {
                val row = LinearLayout(this).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = android.view.Gravity.CENTER_VERTICAL
                }
                row.addView(
                    TextView(this).apply { text = forward.toString() },
                    LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f),
                )
                row.addView(Button(this).apply {
                    text = "Remove"
                    setOnClickListener {
                        GuestSettings.removeForward(this@MainActivity, forward)
                        refresh()
                    }
                })
                list.addView(row)
            }
        }
        val add = Button(this).apply {
            text = "Add forward"
            setOnClickListener {
                val proto = if (protocol.selectedItemPosition == 0) "tcp" else "udp"
                val hostPort = host.text.toString().toIntOrNull()
                val guestPort = guest.text.toString().toIntOrNull()
                if (hostPort == null || guestPort == null) {
                    Toast.makeText(this@MainActivity, "Enter both ports", Toast.LENGTH_SHORT).show()
                    return@setOnClickListener
                }
                GuestSettings.addForward(this@MainActivity, proto, hostPort, guestPort)
                    .onSuccess {
                        host.setText("")
                        guest.setText("")
                        Toast.makeText(
                            this@MainActivity,
                            "Forward added; restart the guest to apply",
                            Toast.LENGTH_SHORT,
                        ).show()
                        refresh()
                    }
                    .onFailure {
                        Toast.makeText(this@MainActivity, it.message, Toast.LENGTH_LONG).show()
                    }
            }
        }
        content.addView(TextView(this).apply { text = "Active forwards" })
        content.addView(list)
        content.addView(TextView(this).apply { text = "Add TCP/UDP forward" })
        content.addView(protocol)
        content.addView(host)
        content.addView(guest)
        content.addView(add)
        content.addView(TextView(this).apply {
            text = "Host ports are on the Android side. Ports below 1024 need root."
        })
        refresh()

        AlertDialog.Builder(this)
            .setTitle("Port Forwarding")
            .setView(ScrollView(this).apply { addView(content) })
            .setPositiveButton("Close", null)
            .show()
    }

    private fun showSettings() {
        val cpus = intArrayOf(1, 2, 4, 8)
        val memory = intArrayOf(256, 512, 1024, 2048)
        val storage = intArrayOf(1, 2, 4, 8)

        fun spinner(values: IntArray, labels: List<String>, selected: Int): Spinner =
            Spinner(this).apply {
                adapter = ArrayAdapter(
                    this@MainActivity,
                    android.R.layout.simple_spinner_dropdown_item,
                    labels,
                )
                setSelection(values.indexOf(selected).let { if (it < 0) 0 else it })
            }

        val cpu = spinner(cpus, cpus.map { it.toString() }, GuestSettings.cpus(this))
        val mem = spinner(
            memory,
            memory.map { if (it < 1024) "$it MiB" else "${it / 1024} GiB" },
            GuestSettings.memoryMib(this),
        )
        val disk = spinner(storage, storage.map { "$it GB" }, 2)
        val current = TextView(this).apply {
            text = "Current rootfs: ${GuestSettings.currentBytes(this@MainActivity) / (1024 * 1024)} MiB"
            setPadding(0, 12, 0, 12)
        }
        val expand = Button(this).apply {
            text = "Expand sparse storage"
            setOnClickListener {
                if (GuestSession.isRunning) {
                    Toast.makeText(
                        this@MainActivity,
                        "Stop the guest first (exit or poweroff)",
                        Toast.LENGTH_SHORT,
                    ).show()
                    return@setOnClickListener
                }
                val gib = storage[disk.selectedItemPosition]
                GuestSettings.expandStorage(this@MainActivity, gib)
                    .onSuccess {
                        current.text = "Current rootfs: ${it / (1024 * 1024)} MiB"
                        Toast.makeText(
                            this@MainActivity,
                            "Expanded to $gib GB; systemd grows ext4 on next boot",
                            Toast.LENGTH_LONG,
                        ).show()
                    }
                    .onFailure {
                        Toast.makeText(this@MainActivity, it.message, Toast.LENGTH_LONG).show()
                    }
            }
        }

        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(36, 12, 36, 12)
            addView(TextView(this@MainActivity).apply { text = "CPU count" })
            addView(cpu)
            addView(TextView(this@MainActivity).apply { text = "Memory" })
            addView(mem)
            addView(TextView(this@MainActivity).apply { text = "Storage size" })
            addView(disk)
            addView(current)
            addView(expand)
        }
        val scroll = ScrollView(this).apply { addView(content) }

        AlertDialog.Builder(this)
            .setTitle("Settings")
            .setView(scroll)
            .setPositiveButton("Save") { _, _ ->
                GuestSettings.setCpus(this, cpus[cpu.selectedItemPosition])
                GuestSettings.setMemoryMib(this, memory[mem.selectedItemPosition])
                Toast.makeText(
                    this,
                    "CPU and memory apply on the next boot",
                    Toast.LENGTH_SHORT,
                ).show()
            }
            .setNegativeButton("Cancel", null)
            .show()
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
