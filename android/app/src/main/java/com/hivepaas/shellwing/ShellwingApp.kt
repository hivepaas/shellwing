package com.hivepaas.shellwing

import android.app.Application
import android.system.Os
import android.util.Base64
import java.io.File
import java.security.KeyStore
import java.security.cert.X509Certificate

/**
 * The process: the core's environment set once, before anything of it runs,
 * and the app's state, which lives as long as the process does (sessions
 * outlast the activity, kept by [SessionService]).
 */
class ShellwingApp : Application() {
    lateinit var model: AppModel
        private set

    override fun onCreate() {
        super.onCreate()
        prepareCore()
        model = AppModel(this)
    }

    /**
     * What the core reads from its environment, which an Android app does
     * not have: a home (the Git sync's clone goes in its cache), known_hosts
     * beside the configuration, and the certificates libgit2's OpenSSL
     * checks HTTPS against, taken from Android's trust store (the user's own
     * CAs included).
     */
    private fun prepareCore() {
        val config = configDir()
        config.mkdirs()
        Os.setenv("HOME", filesDir.path, true)
        Os.setenv("XDG_CACHE_HOME", cacheDir.path, true)
        Os.setenv("XDG_DATA_HOME", filesDir.path, true)
        Os.setenv("SHELLWING_KNOWN_HOSTS", File(config, "known_hosts").path, true)
        Os.setenv("SSL_CERT_FILE", writeCertificates().path, true)
        Native.init(this)
    }

    /** filesDir/shellwing: the configuration, the vault, hosts.toml. */
    fun configDir() = File(filesDir, "shellwing")

    private fun writeCertificates(): File {
        val out = File(noBackupFilesDir, "cacerts.pem")
        runCatching {
            val store = KeyStore.getInstance("AndroidCAStore").apply { load(null) }
            val pem = StringBuilder()
            for (alias in store.aliases()) {
                val cert = store.getCertificate(alias) as? X509Certificate ?: continue
                pem.append("-----BEGIN CERTIFICATE-----\n")
                pem.append(Base64.encodeToString(cert.encoded, Base64.DEFAULT))
                pem.append("-----END CERTIFICATE-----\n")
            }
            out.writeText(pem.toString())
        }
        return out
    }
}
