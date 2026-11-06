package com.breezone.app.data

/**
 * Small key/value store backed by the platform secure storage:
 * Android Keystore (AES-GCM) / iOS Keychain. Values are lost (→ the user is asked for the
 * password again) only if that secure storage is reset, e.g. on a new device.
 */
expect class SecureStore {
    fun get(key: String): String?
    fun set(key: String, value: String)
    fun remove(key: String)
}

/** Human readable name of this device, shown in the server's "connected devices" list. */
expect fun deviceName(): String

/**
 * The system blocks this app from the local network (Android 17+ "local network" permission
 * refused). Used to explain a connection failure instead of showing a timeout.
 */
expect fun localNetworkBlocked(): Boolean
