package dev.drolin

import android.content.ClipData
import android.content.ClipboardManager
import android.util.Log
import android.view.KeyEvent
import android.view.MotionEvent
import com.termux.terminal.TerminalSession
import com.termux.terminal.TerminalSessionClient
import com.termux.view.TerminalView
import com.termux.view.TerminalViewClient

/**
 * The bridge Termux's emulator and view expect. It forwards screen updates to
 * the view and keeps Android policy (clipboard, haptics, logging) here rather
 * than in the vendored code.
 */
class TerminalClient(
    private val activity: MainActivity,
    private val view: TerminalView,
) : TerminalSessionClient, TerminalViewClient {

    override fun onTextChanged(changedSession: TerminalSession) {
        view.onScreenUpdated()
        if (firstOutput) {
            firstOutput = false
            activity.onSessionState("guest running")
        }
    }

    override fun onTitleChanged(changedSession: TerminalSession) = Unit

    override fun onSessionFinished(finishedSession: TerminalSession) {
        activity.onSessionFinished()
    }

    override fun onCopyTextToClipboard(session: TerminalSession, text: String) {
        val clipboard = activity.getSystemService(ClipboardManager::class.java)
        clipboard.setPrimaryClip(ClipData.newPlainText("drolin", text))
    }

    override fun onPasteTextFromClipboard(session: TerminalSession?) {
        val clipboard = activity.getSystemService(ClipboardManager::class.java)
        val text = clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text
        if (!text.isNullOrEmpty()) {
            val bytes = text.toString().toByteArray()
            session?.write(bytes, 0, bytes.size)
        }
    }

    override fun onBell(session: TerminalSession) {
        view.performHapticFeedback(android.view.HapticFeedbackConstants.VIRTUAL_KEY)
    }

    override fun onColorsChanged(session: TerminalSession) = Unit

    override fun onTerminalCursorStateChange(state: Boolean) = Unit

    override fun setTerminalShellPid(session: TerminalSession, pid: Int) = Unit

    override fun getTerminalCursorStyle(): Int? = null

    override fun logError(tag: String, message: String) { Log.e(tag, message) }
    override fun logWarn(tag: String, message: String) { Log.w(tag, message) }
    override fun logInfo(tag: String, message: String) { Log.i(tag, message) }
    override fun logDebug(tag: String, message: String) { Log.d(tag, message) }
    override fun logVerbose(tag: String, message: String) { Log.v(tag, message) }
    override fun logStackTraceWithMessage(tag: String, message: String, e: Exception) {
        Log.e(tag, message, e)
    }

    override fun logStackTrace(tag: String, e: Exception) {
        Log.e(tag, "terminal", e)
    }

    override fun onScale(scale: Float): Float {
        val clamped = scale.coerceIn(0.5f, 2.5f)
        view.setTextSize((BASE_TEXT_SIZE * clamped).toInt().coerceIn(8, 40))
        return clamped
    }

    override fun onSingleTapUp(e: MotionEvent) {
        view.requestFocus()
        activity.showKeyboard()
    }

    override fun shouldBackButtonBeMappedToEscape(): Boolean = false
    // Termux defaults to raw key events (TYPE_NULL). Forcing character-based
    // input makes many IMEs compose text locally, which delays what reaches the
    // terminal even though the guest eventually receives the same bytes.
    override fun shouldEnforceCharBasedInput(): Boolean = false
    override fun shouldUseCtrlSpaceWorkaround(): Boolean = false
    override fun isTerminalViewSelected(): Boolean = true
    override fun copyModeChanged(copyMode: Boolean) = Unit
    override fun onKeyDown(keyCode: Int, e: KeyEvent, session: TerminalSession): Boolean {
        if (GuestSession.isFinished &&
            (keyCode == KeyEvent.KEYCODE_ENTER || keyCode == KeyEvent.KEYCODE_DPAD_CENTER)
        ) {
            activity.finishAndRemoveTask()
            return true
        }
        return false
    }

    override fun onSessionFinishedEnter(): Boolean {
        activity.finishAndRemoveTask()
        return true
    }
    override fun onKeyUp(keyCode: Int, e: KeyEvent): Boolean = false
    override fun onLongPress(event: MotionEvent): Boolean = false

    override fun onMorePressed() {
        activity.showTerminalMenu()
    }
    override fun readControlKey(): Boolean = false
    override fun readAltKey(): Boolean = false
    override fun readShiftKey(): Boolean = false
    override fun readFnKey(): Boolean = false
    override fun onCodePoint(codePoint: Int, ctrlDown: Boolean, session: TerminalSession): Boolean = false
    override fun onEmulatorSet() = Unit

    private var firstOutput = true

    private companion object {
        const val BASE_TEXT_SIZE = 14
    }
}
