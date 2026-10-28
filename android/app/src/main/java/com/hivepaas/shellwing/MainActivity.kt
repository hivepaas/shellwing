package com.hivepaas.shellwing

import android.Manifest
import android.content.pm.PackageManager
import android.os.Bundle
import android.graphics.Color
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.fragment.app.FragmentActivity
import com.hivepaas.shellwing.ui.AppRoot
import com.hivepaas.shellwing.ui.LockScreen
import com.hivepaas.shellwing.ui.ShellwingTheme

/**
 * The one activity: the app's screens, and the lock over them. The state is
 * the process's (ShellwingApp), so a session outlives the activity.
 */
class MainActivity : FragmentActivity() {
    private lateinit var lock: AppLock
    // Allowed: the notification the service posted before it could show is posted again.
    private val askNotifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        (application as ShellwingApp).model.sessionsChanged()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Light icons over the dark app.
        enableEdgeToEdge(SystemBarStyle.dark(Color.TRANSPARENT), SystemBarStyle.dark(Color.TRANSPARENT))
        val model = (application as ShellwingApp).model
        lock = AppLock(model.prefs)
        setContent {
            ShellwingTheme {
                // The first session opened asks to show the notification that keeps sessions connected.
                LaunchedEffect(Unit) {
                    snapshotFlow { model.sessions.isNotEmpty() }.collect { open ->
                        if (open) askForNotifications()
                    }
                }
                Box(Modifier.fillMaxSize()) {
                    AppRoot(model, lock, onLockNow = { lock.lockNow() })
                    if (lock.locked) {
                        LockScreen(model.sessions.size, onUnlock = { lock.unlock(this@MainActivity) })
                        LaunchedEffect(Unit) { lock.unlock(this@MainActivity) }
                    }
                }
            }
        }
    }

    override fun onStart() {
        super.onStart()
        lock.cameBack()
    }

    override fun onStop() {
        super.onStop()
        lock.wentAway()
    }

    private fun askForNotifications() {
        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            askNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
}
