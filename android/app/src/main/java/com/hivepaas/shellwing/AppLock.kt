package com.hivepaas.shellwing

import android.content.SharedPreferences
import android.os.SystemClock
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_WEAK
import androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL
import androidx.biometric.BiometricPrompt
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity

/**
 * Lock shellwing when away: after this many minutes in the background
 * (0: never), it hides until the fingerprint, the face or the screen lock
 * says it is you. A light guard for a phone left unlocked; the sessions go
 * on underneath. As the iOS app's AppLock.
 */
class AppLock(private val prefs: SharedPreferences) {
    var after by mutableIntStateOf(prefs.getInt(KEY, 0))
        private set
    var locked by mutableStateOf(prefs.getInt(KEY, 0) > 0)
        private set
    private var awaySince: Long? = null

    fun lockAfter(minutes: Int) {
        after = minutes
        prefs.edit().putInt(KEY, minutes).apply()
    }

    fun lockNow() {
        if (after > 0) locked = true
    }

    fun wentAway() {
        awaySince = SystemClock.elapsedRealtime()
    }

    fun cameBack() {
        val since = awaySince ?: return
        awaySince = null
        if (after > 0 && SystemClock.elapsedRealtime() - since >= after * 60_000L) locked = true
    }

    /** The phone can say it is you: a fingerprint, a face, or a screen lock. */
    fun available(activity: FragmentActivity) =
        BiometricManager.from(activity).canAuthenticate(BIOMETRIC_WEAK or DEVICE_CREDENTIAL) ==
            BiometricManager.BIOMETRIC_SUCCESS

    /** Asks that it is you; [done] hears whether it was. With nothing to ask with, it opens. */
    fun confirm(activity: FragmentActivity, title: String, done: (Boolean) -> Unit) {
        if (!available(activity)) {
            done(true)
            return
        }
        val prompt = BiometricPrompt(
            activity, ContextCompat.getMainExecutor(activity),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) = done(true)
                override fun onAuthenticationError(code: Int, said: CharSequence) = done(false)
            },
        )
        prompt.authenticate(
            BiometricPrompt.PromptInfo.Builder()
                .setTitle(title)
                .setAllowedAuthenticators(BIOMETRIC_WEAK or DEVICE_CREDENTIAL)
                .build(),
        )
    }

    fun unlock(activity: FragmentActivity) {
        confirm(activity, tr("Unlock shellwing")) { ok -> if (ok) locked = false }
    }

    private companion object {
        const val KEY = "lock.after"
    }
}
