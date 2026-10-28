package com.hivepaas.shellwing

import android.content.Context
import android.content.ContextWrapper
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

    fun unlock(activity: FragmentActivity) {
        confirmOwner(activity, tr("Unlock shellwing")) { ok -> if (ok) locked = false }
    }

    private companion object {
        const val KEY = "lock.after"
    }
}

/** The phone can say it is you: a fingerprint, a face, or a screen lock. */
fun ownerCheckAvailable(context: Context) =
    BiometricManager.from(context).canAuthenticate(BIOMETRIC_WEAK or DEVICE_CREDENTIAL) ==
        BiometricManager.BIOMETRIC_SUCCESS

/**
 * Asks that it is you (unlocking, showing a secret: an environment's values,
 * a sensitive note, the recovery key); [done] hears whether it was. With
 * nothing to ask with, it opens, as on iOS.
 */
fun confirmOwner(context: Context, title: String, done: (Boolean) -> Unit) {
    val activity = context.findActivity()
    if (activity == null || !ownerCheckAvailable(context)) {
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

private tailrec fun Context.findActivity(): FragmentActivity? = when (this) {
    is FragmentActivity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
