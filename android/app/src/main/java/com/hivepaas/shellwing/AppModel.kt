package com.hivepaas.shellwing

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.hivepaas.shellwing.core.HostEdit
import com.hivepaas.shellwing.core.HostSection
import com.hivepaas.shellwing.core.MobileException
import com.hivepaas.shellwing.core.OnDisconnectWay
import com.hivepaas.shellwing.core.SessionStatus
import com.hivepaas.shellwing.core.Shellwing
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * The app's state: shellwing-core, the hosts it lists, and the sessions
 * open. One for the process (ShellwingApp), so sessions outlive the
 * activity while [SessionService] keeps the process.
 */
class AppModel(private val app: ShellwingApp) {
    val core: Shellwing?
    /** Why the core could not start, if it did not. */
    val problem: String?

    var sections by mutableStateOf<List<HostSection>>(emptyList())
        private set
    val sessions = mutableStateListOf<SessionModel>()
    var error by mutableStateOf<String?>(null)

    val prefs = app.getSharedPreferences("shellwing", Context.MODE_PRIVATE)
    /** Kept when away: a notification shows the sessions (Settings). */
    var keepInBackground by mutableStateOf(prefs.getBoolean(KEEP, true))
        private set

    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val main = Handler(Looper.getMainLooper())
    private var network: Network? = null

    init {
        var made: Shellwing? = null
        var why: String? = null
        try {
            made = Shellwing(app.configDir().path, KeystoreKeys(app))
        } catch (e: Exception) {
            why = e.said()
        }
        core = made
        problem = why
        reload()
        watchNetwork()
        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) = cameBack()
        })
    }

    // Back on screen, or the network moved (Wi-Fi to mobile): each session is
    // checked, and made again if it no longer answers (MobileSession.resumed,
    // as the setting says).

    private fun cameBack() {
        for (s in sessions) s.resumed()
    }

    private fun watchNetwork() {
        val cm = app.getSystemService(ConnectivityManager::class.java) ?: return
        cm.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(now: Network) {
                main.post {
                    val was = network
                    network = now
                    if (was != null && was != now) cameBack()
                }
            }
        })
    }

    /** What a session does when its connection ends (the desktop's setting). */
    val onDisconnect: OnDisconnectWay
        get() = core?.settings()?.onDisconnect ?: OnDisconnectWay.RECONNECT

    fun reload() {
        val core = core ?: return
        try {
            sections = core.sections()
        } catch (e: Exception) {
            error = e.said()
        }
    }

    fun groups(): List<String> = core?.groups() ?: emptyList()

    fun host(id: String): HostEdit? = core?.host(id)

    /** Saves the host: its id, or why not. */
    fun save(edit: HostEdit): String {
        val core = core ?: throw MobileException.Failed(problem ?: "")
        val id = core.saveHost(edit)
        reload()
        return id
    }

    fun delete(hostId: String) {
        try {
            core?.deleteHost(hostId)
            reload()
        } catch (e: Exception) {
            error = e.said()
        }
    }

    /** The session open to [spec], else a new one. */
    fun session(spec: String): SessionModel? =
        sessions.firstOrNull {
            it.spec == spec && it.status != SessionStatus.CLOSED && it.status != SessionStatus.FAILED
        } ?: open(spec)

    /** A new session to [spec], even with one open already. */
    fun open(spec: String): SessionModel? {
        val core = core ?: return null
        val model = SessionModel(spec)
        return try {
            val session = core.open(spec, model)
            model.attach(session)
            // Config files left on the host by a session that could not take
            // them away (the app was stopped): tidied once it connects.
            model.connected = {
                scope.launch { core.tidyPlaced(session)?.let { model.notice = it } }
            }
            model.statusChanged = ::sessionsChanged
            sessions.add(model)
            sessionsChanged()
            model
        } catch (e: Exception) {
            error = e.said()
            null
        }
    }

    /** Closes a session: the config files it placed until it ends go first. */
    fun close(model: SessionModel) {
        sessions.remove(model)
        sessionsChanged()
        val core = core ?: return
        val handle = model.handle ?: return
        if (model.placed.isEmpty()) return
        scope.launch { runCatching { core.removePlaced(handle, null) } }
    }

    fun closeAll() {
        for (s in sessions.toList()) close(s)
    }

    fun keepWhenAway(on: Boolean) {
        keepInBackground = on
        prefs.edit().putBoolean(KEEP, on).apply()
        sessionsChanged()
    }

    /** The service runs while a session is live and the setting says so; its notification follows them. */
    fun sessionsChanged() {
        if (keepInBackground && sessions.any { it.isLive }) {
            SessionService.show(app)
        } else {
            SessionService.stop(app)
        }
    }

    private companion object {
        const val KEEP = "background.keep"
    }
}

/** What an error says: the core's own words, without UniFFI's `v1=`. */
fun Throwable.said(): String = (this as? MobileException.Failed)?.v1 ?: message ?: toString()
