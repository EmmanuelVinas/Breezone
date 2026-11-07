package com.breezone.app.data

import com.breezone.app.resources.Res
import com.breezone.app.resources.error_certificate
import com.breezone.app.resources.error_certificate_changed
import com.breezone.app.resources.error_live_proxy
import com.breezone.app.resources.error_offline
import com.breezone.app.resources.error_refused
import com.breezone.app.resources.error_timeout
import com.breezone.app.resources.error_unknown_host
import org.jetbrains.compose.resources.getString

/** Network failures that can be explained to the user. */
enum class NetworkProblem { Certificate, UnknownHost, Refused, Timeout, Offline, LiveBlocked }

/**
 * Recognises the cause of a network failure from the platform exception (OkHttp on Android,
 * NSURLSession on iOS), whose class names and messages differ.
 */
fun classify(e: Throwable): NetworkProblem? {
    val text = generateSequence(e) { it.cause }.take(8).joinToString(" | ") { "${it::class.simpleName} ${it.message}" }
    val lower = text.lowercase()
    fun any(vararg needles: String) = needles.any { lower.contains(it.lowercase()) }
    return when {
        any("certificate", "not verified", "trust anchor", "certpath", "sslpeerunverified", "sslhandshake", "code=-1202", "code=-1201", "code=-1203", "code=-1204", "code=-1205", "code=-1206", "code=-1200") ->
            NetworkProblem.Certificate
        // The WebSocket handshake answered something else than 101: usually a reverse proxy
        // that does not forward the WebSocket headers.
        any("expected status code 101", "expected http 101", "101 response", "websocket upgrade", "code=-1011") -> NetworkProblem.LiveBlocked
        any("unknownhost", "unable to resolve host", "no address associated", "nodename nor servname", "code=-1003") -> NetworkProblem.UnknownHost
        any("code=-1009", "network is unreachable", "enetunreach", "not connected to the internet") -> NetworkProblem.Offline
        any("connection refused", "econnrefused", "code=-1004", "failed to connect") -> NetworkProblem.Refused
        any("timeout", "timed out", "code=-1001") -> NetworkProblem.Timeout
        else -> null
    }
}

/** Explains a network failure towards `url`, or null when the cause is not recognised. */
suspend fun explain(e: Throwable, url: String): String? {
    val host = normalizeUrl(url).substringAfter("://").substringBefore('/')
    return when (classify(e)) {
        NetworkProblem.Certificate ->
            if (TlsTrust.isPinned(url)) getString(Res.string.error_certificate_changed, host)
            else getString(Res.string.error_certificate, host)
        NetworkProblem.UnknownHost -> getString(Res.string.error_unknown_host, host)
        NetworkProblem.Refused -> getString(Res.string.error_refused, host)
        NetworkProblem.Timeout -> getString(Res.string.error_timeout, host)
        NetworkProblem.Offline -> getString(Res.string.error_offline)
        NetworkProblem.LiveBlocked -> getString(Res.string.error_live_proxy, host)
        null -> null
    }
}
