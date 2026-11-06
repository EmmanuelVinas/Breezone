package com.breezone.app.data

import com.russhwolf.settings.ExperimentalSettingsImplementation
import com.russhwolf.settings.KeychainSettings
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
