package com.breezone.app.data

import io.ktor.client.HttpClient
import io.ktor.client.plugins.websocket.webSocket
import io.ktor.client.request.header
import io.ktor.client.request.request
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.contentType
import io.ktor.http.isSuccess
import io.ktor.websocket.Frame
import io.ktor.websocket.readText
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import com.breezone.app.resources.Res
import com.breezone.app.resources.error_invalid_credentials
import com.breezone.app.resources.error_not_breezone
import com.breezone.app.resources.error_session_expired
import org.jetbrains.compose.resources.getString
import kotlin.time.Clock

/** The server refused our credentials: the user must sign in again. */
class AuthRequiredException(message: String = "session expired") : Exception(message)

class ApiException(val status: Int, message: String) : Exception(message)

fun normalizeUrl(url: String): String {
    var u = url.trim().trimEnd('/')
    if (!u.startsWith("http://") && !u.startsWith("https://")) u = "https://$u"
    return u
}

private fun errorMessage(body: String, status: HttpStatusCode): String =
    runCatching { FzJson.parseToJsonElement(body).jsonObject["error"]?.toString()?.trim('"') }.getOrNull()
        ?: status.description

/** Calls that do not need an account (onboarding). */
class BreezoneDiscovery(private val http: HttpClient) {
    /** Checks that `url` is a Breezone server; returns whether it still needs its first setup. */
    suspend fun probe(url: String): SetupStatus {
        val base = normalizeUrl(url)
        val health = http.request("$base/health")
        if (!health.status.isSuccess() || !health.bodyAsText().contains("\"ok\"")) {
            throw ApiException(health.status.value, getString(Res.string.error_not_breezone))
        }
        return FzJson.decodeFromString(http.request("$base/api/setup").bodyAsText())
    }

    suspend fun login(url: String, username: String, password: String, setup: Boolean = false): TokenPair {
        val base = normalizeUrl(url)
        val path = if (setup) "/api/setup" else "/api/auth/login"
        val response = http.request("$base$path") {
            method = HttpMethod.Post
            contentType(ContentType.Application.Json)
            setBody(
                buildJsonObject {
                    put("username", username.trim())
                    put("password", password)
                    put("device_name", deviceName())
                }.toString(),
            )
        }
        val body = response.bodyAsText()
        if (!response.status.isSuccess()) {
            val msg = errorMessage(body, response.status)
            throw ApiException(
                response.status.value,
                when (msg) {
                    "invalid username or password" -> getString(Res.string.error_invalid_credentials)
                    else -> msg
                },
            )
        }
        return FzJson.decodeFromString(body)
    }
}

/**
 * Client of one Breezone server. Keeps a short-lived access token in memory and refreshes it
 * with the long-lived refresh token, which is rotated by the server and persisted immediately
 * in the secure store (`onRefreshTokenRotated`).
 */
class BreezoneApi(
    private val http: HttpClient,
    val account: ServerAccount,
    private val onRefreshTokenRotated: (String) -> Unit,
) {
    val base = normalizeUrl(account.url)
    private val mutex = Mutex()
    private var accessToken: String? = null
    private var accessExpiresAt = 0L
    private var refreshToken: String? = account.refreshToken

    var user: User? = null
        private set

    fun seed(pair: TokenPair) {
        accessToken = pair.accessToken
        accessExpiresAt = now() + pair.expiresIn - 30
        refreshToken = pair.refreshToken
        user = pair.user
    }

    private fun now() = Clock.System.now().epochSeconds

    private suspend fun token(forceRefresh: Boolean = false): String = mutex.withLock {
        val current = accessToken
        if (!forceRefresh && current != null && now() < accessExpiresAt) return current
        val refresh = refreshToken ?: throw AuthRequiredException(getString(Res.string.error_session_expired))
        val response = http.request("$base/api/auth/refresh") {
            method = HttpMethod.Post
            contentType(ContentType.Application.Json)
            setBody(buildJsonObject { put("refresh_token", refresh) }.toString())
        }
        val body = response.bodyAsText()
        if (response.status == HttpStatusCode.Unauthorized) {
            refreshToken = null
            throw AuthRequiredException(getString(Res.string.error_session_expired))
        }
        if (!response.status.isSuccess()) throw ApiException(response.status.value, errorMessage(body, response.status))
        val pair = FzJson.decodeFromString<TokenPair>(body)
        seed(pair)
        onRefreshTokenRotated(pair.refreshToken)
        pair.accessToken
    }

    private suspend fun send(method: HttpMethod, path: String, body: String?, retry: Boolean = true): HttpResponse {
        val response = http.request("$base$path") {
            this.method = method
            header("Authorization", "Bearer ${token()}")
            if (body != null) {
                contentType(ContentType.Application.Json)
                setBody(body)
            }
        }
        if (response.status == HttpStatusCode.Unauthorized && retry) {
            token(forceRefresh = true)
            return send(method, path, body, retry = false)
        }
        if (response.status == HttpStatusCode.Unauthorized) throw AuthRequiredException(getString(Res.string.error_session_expired))
        return response
    }

    private suspend inline fun <reified T> call(method: HttpMethod, path: String, body: String? = null): T {
        val response = send(method, path, body)
        val text = response.bodyAsText()
        if (!response.status.isSuccess()) throw ApiException(response.status.value, errorMessage(text, response.status))
        return FzJson.decodeFromString(text)
    }

    private suspend fun callUnit(method: HttpMethod, path: String, body: String? = null) {
        val response = send(method, path, body)
        if (!response.status.isSuccess()) throw ApiException(response.status.value, errorMessage(response.bodyAsText(), response.status))
    }

    private fun zonePath(z: Zone) = "/api/installations/${z.installationId}/zones/${z.systemId}/${z.zoneId}"

    suspend fun me(): User = call<User>(HttpMethod.Get, "/api/me").also { user = it }
    suspend fun status(): List<Installation> = call(HttpMethod.Get, "/api/status")
    suspend fun control(zone: Zone, cmd: ZoneCommand): Zone =
        call(HttpMethod.Patch, zonePath(zone), FzJson.encodeToString(ZoneCommand.serializer(), cmd))

    suspend fun batch(actions: List<Action>): RunResult =
        call(HttpMethod.Post, "/api/control/batch", FzJson.encodeToString(BatchBody.serializer(), BatchBody(actions)))

    suspend fun history(zone: Zone, hours: Int): List<Reading> = call(HttpMethod.Get, "${zonePath(zone)}/history?hours=$hours")
    suspend fun routines(): List<Routine> = call(HttpMethod.Get, "/api/routines")
    suspend fun createRoutine(input: RoutineInput): Routine =
        call(HttpMethod.Post, "/api/routines", FzJson.encodeToString(RoutineInput.serializer(), input))

    suspend fun updateRoutine(id: Long, input: RoutineInput): Routine =
        call(HttpMethod.Put, "/api/routines/$id", FzJson.encodeToString(RoutineInput.serializer(), input))

    suspend fun deleteRoutine(id: Long) = callUnit(HttpMethod.Delete, "/api/routines/$id")
    suspend fun runRoutine(id: Long): RunResult = call(HttpMethod.Post, "/api/routines/$id/run")
    suspend fun cloud(): CloudStatus = call(HttpMethod.Get, "/api/cloud")
    suspend fun cloudConnect(email: String, password: String, remember: Boolean): CloudResult =
        call(
            HttpMethod.Put,
            "/api/cloud",
            buildJsonObject {
                put("email", email.trim())
                put("password", password)
                put("remember", remember)
            }.toString(),
        )

    suspend fun cloudSync(): CloudResult = call(HttpMethod.Post, "/api/cloud/sync")

    /** Revokes this device on the server (sign out). */
    suspend fun logout() {
        val refresh = refreshToken ?: return
        runCatching {
            http.request("$base/api/auth/logout") {
                method = HttpMethod.Post
                contentType(ContentType.Application.Json)
                setBody(buildJsonObject { put("refresh_token", refresh) }.toString())
            }
        }
    }

    /** Push channel: emits every message of the server WebSocket until it closes. */
    fun live(): Flow<JsonObject> = callbackFlow {
        val wsUrl = base.replaceFirst("https://", "wss://").replaceFirst("http://", "ws://") + "/api/ws"
        val token = token()
        val job = launch {
            http.webSocket(wsUrl) {
                send(Frame.Text(buildJsonObject { put("type", "auth"); put("token", token) }.toString()))
                for (frame in incoming) {
                    if (frame is Frame.Text) {
                        val text = frame.readText()
                        if (text != "pong") send(FzJson.parseToJsonElement(text).jsonObject)
                    }
                }
            }
            close()
        }
        job.invokeOnCompletion { cause -> if (cause != null) close(cause) }
        awaitClose { job.cancel() }
    }
}

@kotlinx.serialization.Serializable
private data class BatchBody(val actions: List<Action>)
