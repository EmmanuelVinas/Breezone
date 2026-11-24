package com.breezone.app.data

import io.ktor.http.Url
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

/** A server to add, from the QR code the web UI shows (`breezone://connect?url=…&username=…`). */
data class ConnectLink(val url: String, val username: String)

/** The connection links the system hands the app (QR code scanned with the camera, link tapped). */
object ConnectLinks {
    private val _pending = MutableStateFlow<ConnectLink?>(null)
    val pending: StateFlow<ConnectLink?> = _pending

    /** Keeps [link] for the app to open, if it is a connection link. */
    fun open(link: String): Boolean {
        val parsed = parse(link) ?: return false
        _pending.value = parsed
        return true
    }

    fun consume() {
        _pending.value = null
    }

    /** The server and the user of a link; never a password, which the link must not carry. */
    fun parse(link: String): ConnectLink? {
        val u = runCatching { Url(link.trim()) }.getOrNull() ?: return null
        if (!u.protocol.name.equals("breezone", true) || !u.host.equals("connect", true)) return null
        val server = u.parameters["url"]?.trim()?.trimEnd('/').orEmpty()
        val address = runCatching { Url(server) }.getOrNull()
        if (address == null || address.protocol.name !in setOf("http", "https") || address.host.isBlank() || !server.contains("://")) return null
        val username = u.parameters["username"]?.trim().orEmpty().take(64)
        return ConnectLink(server, username)
    }
}
