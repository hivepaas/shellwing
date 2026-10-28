package com.hivepaas.shellwing

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.hivepaas.shellwing.core.FrameData
import com.hivepaas.shellwing.core.Key
import com.hivepaas.shellwing.core.LoadedEnvRow
import com.hivepaas.shellwing.core.MobileSession
import com.hivepaas.shellwing.core.PlacedRow
import com.hivepaas.shellwing.core.Retrying
import com.hivepaas.shellwing.core.SessionListener
import com.hivepaas.shellwing.core.SessionStatus
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

/**
 * One session: its latest frame, status and title, as state the screens
 * read. The core calls [changed] from its own threads; reading happens on
 * the main one, once per wake (as ios/Shellwing/AppModel.swift does).
 */
class SessionModel(val spec: String) : SessionListener {
    val id: String = UUID.randomUUID().toString()
    var handle: MobileSession? = null
        private set

    var frame by mutableStateOf<FrameData?>(null)
        private set
    var status by mutableStateOf(SessionStatus.CONNECTING)
        private set
    var title by mutableStateOf(spec)
        private set
    var ended by mutableStateOf<String?>(null)
        private set
    /** A password that worked, offered to keep: `the password for deploy@web-1`. */
    var offer by mutableStateOf<String?>(null)
        private set
    /** Something said once: an environment loaded, a load given up. */
    var notice by mutableStateOf<String?>(null)
    var envs by mutableStateOf<List<LoadedEnvRow>>(emptyList())
        private set
    /** A lost connection about to be tried again: in how long, which try. */
    var retrying by mutableStateOf<Retrying?>(null)
        private set
    var placed by mutableStateOf<List<PlacedRow>>(emptyList())
        private set
    /** Ctrl or Alt, pressed on the row above the keyboard: held for the next key. */
    var ctrl by mutableStateOf(false)
    var alt by mutableStateOf(false)

    /** Called on the main thread each time the session connects. */
    var connected: (() -> Unit)? = null
    /** Called on the main thread when the status changes. */
    var statusChanged: (() -> Unit)? = null

    private val main = Handler(Looper.getMainLooper())
    private val scheduled = AtomicBoolean(false)

    fun attach(session: MobileSession) {
        handle = session
        changed()
    }

    // SessionListener: from the core's threads. One refresh at a time is enough.
    override fun changed() {
        if (scheduled.compareAndSet(false, true)) main.post(::refresh)
    }

    private fun refresh() {
        scheduled.set(false)
        val session = handle ?: return
        session.frame()?.let { if (it.seq != frame?.seq) frame = it }
        val s = session.status()
        if (s != status) {
            status = s
            if (s == SessionStatus.CONNECTED) connected?.invoke()
            statusChanged?.invoke()
        }
        session.title().let { if (it != title) title = it }
        session.ended().let { if (it != ended) ended = it }
        session.offer().let { if (it != offer) offer = it }
        session.notice()?.let { notice = it }
        session.loadedEnvs().let { if (it != envs) envs = it }
        session.retrying().let { if (it != retrying) retrying = it }
        session.placed().let { if (it != placed) placed = it }
    }

    // What the keyboard and the keys row send.

    fun type(text: String) {
        val session = handle ?: return
        // Ctrl or Alt held from the keys row: a letter goes with it.
        val c = text.singleOrNull()
        if ((ctrl || alt) && c != null && c.isLetter() && c.code < 128) {
            session.key(Key.Letter(c.toString()), ctrl, alt, false)
            ctrl = false
            alt = false
            return
        }
        if (text == "\n") {
            press(Key.Enter)
            return
        }
        session.text(text)
        ctrl = false
        alt = false
    }

    /** A key: Ctrl and Alt held from the row, or from a keyboard. */
    fun press(key: Key, ctrl: Boolean = this.ctrl, alt: Boolean = this.alt, shift: Boolean = false) {
        handle?.key(key, ctrl, alt, shift)
        this.ctrl = false
        this.alt = false
    }

    fun paste(text: String) = handle?.paste(text)

    fun resize(cols: Int, rows: Int, cellW: Int, cellH: Int) =
        handle?.resize(cols.toUShort(), rows.toUShort(), cellW.toUInt(), cellH.toUInt())

    fun scroll(lines: Float) = handle?.scroll(lines)

    fun scrollToBottom() = handle?.scrollToBottom()

    fun reconnect() = handle?.reconnect()

    fun stopRetrying() {
        handle?.stopRetrying()
        retrying = null
    }

    /** Back from the background, or the network moved: checked, made again if it no longer answers. */
    fun resumed() = handle?.resumed()

    fun unload(env: String?) {
        try {
            handle?.unloadEnv(env)
        } catch (e: Exception) {
            notice = e.said()
        }
        handle?.let { envs = it.loadedEnvs() }
    }

    fun keepOffer() {
        handle?.saveOffer()
        offer = null
    }

    fun dismissOffer() {
        handle?.dismissOffer()
        offer = null
    }

    val isLive get() = status == SessionStatus.CONNECTED || status == SessionStatus.CONNECTING
}
