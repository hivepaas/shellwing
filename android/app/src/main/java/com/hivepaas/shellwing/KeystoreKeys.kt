package com.hivepaas.shellwing

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import com.hivepaas.shellwing.core.MobileException
import com.hivepaas.shellwing.core.PlatformKeys
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The vault's key, kept as the iOS Keychain keeps it: on this device only.
 * An AES key that never leaves Android's keystore encrypts it into a file in
 * the app's no-backup folder, so a backup restored elsewhere holds nothing
 * that opens the vault. The vault asks from its own thread.
 */
class KeystoreKeys(context: Context) : PlatformKeys {
    private val file = File(context.noBackupFilesDir, "vault.key")

    override fun load(): ByteArray? = failing {
        if (!file.exists()) return@failing null
        val bytes = file.readBytes()
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.DECRYPT_MODE, wrapper(), GCMParameterSpec(128, bytes, 0, IV_SIZE))
        cipher.doFinal(bytes, IV_SIZE, bytes.size - IV_SIZE)
    }

    override fun save(key: ByteArray) = failing {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, wrapper())
        val sealed = cipher.iv + cipher.doFinal(key)
        val tmp = File(file.parentFile, "vault.key.new")
        tmp.writeBytes(sealed)
        check(tmp.renameTo(file)) { "cannot write ${file.path}" }
    }

    override fun forget() = failing {
        file.delete()
        keystore().deleteEntry(ALIAS)
    }

    /** The AES key in the keystore, made the first time. */
    private fun wrapper(): SecretKey {
        (keystore().getKey(ALIAS, null) as SecretKey?)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    private fun keystore() = KeyStore.getInstance(KEYSTORE).apply { load(null) }

    private inline fun <T> failing(work: () -> T): T =
        try {
            work()
        } catch (e: MobileException) {
            throw e
        } catch (e: Exception) {
            throw MobileException.Failed(e.message ?: e.toString())
        }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "shellwing.vault"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV_SIZE = 12
    }
}
