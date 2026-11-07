package com.breezone.app.data

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

private const val KEY_ALIAS = "breezone_secure_store"
private const val PREFS = "breezone_secure"

/** Values are encrypted with an AES-256 key that never leaves the Android Keystore. */
actual class SecureStore(context: Context) {
    private val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    actual fun get(key: String): String? {
        val stored = prefs.getString(key, null) ?: return null
        return runCatching {
            val raw = Base64.decode(stored, Base64.NO_WRAP)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, raw, 0, 12))
            String(cipher.doFinal(raw, 12, raw.size - 12), Charsets.UTF_8)
        }.getOrElse {
            // The Keystore key changed (restore on a new device, reset…): forget the value.
            prefs.edit().remove(key).apply()
            null
        }
    }

    actual fun set(key: String, value: String) {
        runCatching {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key())
            val encrypted = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
            prefs.edit().putString(key, Base64.encodeToString(encrypted, Base64.NO_WRAP)).apply()
        }.onFailure { android.util.Log.w("Breezone", "secure store write failed", it) }
    }

    actual fun remove(key: String) {
        prefs.edit().remove(key).apply()
    }
}

actual fun deviceName(): String {
    val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
    val model = Build.MODEL
    return (if (model.startsWith(maker, ignoreCase = true)) model else "$maker $model") + " (Android)"
}

actual fun localNetworkBlocked(): Boolean {
    if (Build.VERSION.SDK_INT < 37) return false
    val context = runCatching { org.koin.core.context.GlobalContext.get().get<Context>() }.getOrNull() ?: return false
    return context.checkSelfPermission(android.Manifest.permission.ACCESS_LOCAL_NETWORK) != android.content.pm.PackageManager.PERMISSION_GRANTED
}

actual fun sha256(bytes: ByteArray): ByteArray = java.security.MessageDigest.getInstance("SHA-256").digest(bytes)
