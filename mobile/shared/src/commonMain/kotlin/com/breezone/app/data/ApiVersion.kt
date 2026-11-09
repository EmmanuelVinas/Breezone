package com.breezone.app.data

import kotlinx.serialization.Serializable

/**
 * API version this app speaks, sent in [CLIENT_API_HEADER] with every request. Raise it with the
 * server's `API_VERSION` when the app starts relying on a breaking change (policy in docs/API.md).
 */
const val CLIENT_API = 1

/** Oldest server API this app still works with. */
const val MIN_SERVER_API = 1

const val CLIENT_API_HEADER = "Breezone-Api"

/** `GET /health`. Servers up to 0.4.x send no API version: they speak API 1. */
@Serializable
data class Health(
    val status: String = "",
    val version: String? = null,
    val apiVersion: Int = 1,
    val minClientApi: Int = 1,
)

enum class UpdateNeeded { App, Server }

fun updateNeeded(health: Health): UpdateNeeded? = when {
    health.apiVersion < MIN_SERVER_API -> UpdateNeeded.Server
    CLIENT_API < health.minClientApi -> UpdateNeeded.App
    else -> null
}

/** The server speaks a newer API than the app: still compatible, but an app update would add features. */
fun appUpdateAvailable(health: Health) = health.apiVersion > CLIENT_API && updateNeeded(health) == null

class IncompatibleException(val update: UpdateNeeded, val serverVersion: String?, message: String) : Exception(message)
