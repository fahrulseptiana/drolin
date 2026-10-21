package dev.drolin

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.view.Gravity
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

class MainActivity : Activity(), GuestRuntime.Listener {
    private lateinit var status: TextView
    private lateinit var output: TextView
    private lateinit var input: EditText

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 24, 24, 24)
        }
        status = TextView(this).apply {
            text = "idle"
            textSize = 14f
        }
        output = TextView(this).apply {
            typeface = android.graphics.Typeface.MONOSPACE
            textSize = 12f
            setTextIsSelectable(true)
        }
        val scroll = ScrollView(this).apply {
            addView(output, ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                0,
                1f,
            )
        }
        input = EditText(this).apply {
            hint = "command (when the guest provides a shell)"
            inputType = InputType.TYPE_CLASS_TEXT
        }
        val send = Button(this).apply {
            text = "Send"
            setOnClickListener {
                val line = input.text.toString()
                if (line.isNotEmpty()) {
                    GuestRuntime.send(line)
                    input.setText("")
                }
            }
        }
        val boot = Button(this).apply {
            text = "Boot demo"
            setOnClickListener {
                startForegroundService()
                Thread({ GuestRuntime.start(this@MainActivity) }, "drolin-start").start()
            }
        }
        val stop = Button(this).apply {
            text = "Stop"
            setOnClickListener {
                GuestRuntime.stop()
                stopService(Intent(this@MainActivity, GuestService::class.java))
            }
        }

        root.addView(status)
        root.addView(scroll)
        root.addView(
            LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                addView(input, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                addView(send)
            }
        )
        root.addView(
            LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                addView(boot)
                addView(stop)
            }
        )
        setContentView(root)

        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        }
    }

    override fun onResume() {
        super.onResume()
        GuestRuntime.attach(this)
    }

    override fun onPause() {
        GuestRuntime.detach(this)
        super.onPause()
    }

    override fun onOutput(text: String) {
        runOnUiThread {
            output.append(text)
            (output.parent as? ScrollView)?.post {
                (output.parent as? ScrollView)?.fullScroll(ScrollView.FOCUS_DOWN)
            }
        }
    }

    override fun onState(text: String) {
        runOnUiThread { status.text = text }
    }

    private fun startForegroundService() {
        val intent = Intent(this, GuestService::class.java).setAction(GuestService.ACTION_START)
        if (Build.VERSION.SDK_INT >= 26) startForegroundService(intent) else startService(intent)
    }
}
