package com.breezone.app.data

import com.russhwolf.settings.ExperimentalSettingsImplementation
import com.russhwolf.settings.KeychainSettings
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.usePinned
import platform.UIKit.UIDevice

/**
 * Values are stored in the iOS Keychain. Keychain failures (e.g. an unsigned build without the
 * keychain entitlement) never crash the app: the session is simply not kept, and the user is
 * asked for the password again.
 */
@OptIn(ExperimentalSettingsImplementation::class)
actual class SecureStore {
    private val keychain = KeychainSettings(service = "com.breezone.app")

    actual fun get(key: String): String? = runCatching { keychain.getStringOrNull(key) }
        .onFailure { println("Breezone: keychain read failed: ${it.message}") }
        .getOrNull()

    actual fun set(key: String, value: String) {
        runCatching { keychain.putString(key, value) }.onFailure { println("Breezone: keychain write failed: ${it.message}") }
    }

    actual fun remove(key: String) {
        runCatching { keychain.remove(key) }
    }
}

actual fun deviceName(): String = "${UIDevice.currentDevice.name} (iOS)"

/** iOS asks for the local network itself (NSLocalNetworkUsageDescription) and cannot be checked. */
actual fun localNetworkBlocked(): Boolean = false

@OptIn(kotlinx.cinterop.ExperimentalForeignApi::class)
actual fun sha256(bytes: ByteArray): ByteArray {
    val out = UByteArray(platform.CoreCrypto.CC_SHA256_DIGEST_LENGTH)
    out.usePinned { o ->
        if (bytes.isEmpty()) {
            platform.CoreCrypto.CC_SHA256(null, 0u, o.addressOf(0))
        } else {
            bytes.usePinned { b -> platform.CoreCrypto.CC_SHA256(b.addressOf(0), bytes.size.toUInt(), o.addressOf(0)) }
        }
    }
    return out.toByteArray()
}
