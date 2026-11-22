package com.breezone.app.data

import androidx.compose.ui.text.intl.Locale
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
import io.ktor.http.encodeURLPathPart
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
import com.breezone.app.resources.error_app_too_old
import com.breezone.app.resources.error_server_too_old
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

suspend fun incompatible(update: UpdateNeeded, serverVersion: String?) = IncompatibleException(
    update,
    serverVersion,
    when (update) {
        UpdateNeeded.App -> getString(Res.string.error_app_too_old)
        UpdateNeeded.Server -> getString(Res.string.error_server_too_old, serverVersion ?: "?")
    },
)

/** Throws [IncompatibleException] when the app and the server cannot work together. */
suspend fun checkCompatible(health: Health) {
    updateNeeded(health)?.let { throw incompatible(it, health.version) }
}

class BreezoneDiscovery(private val http: HttpClient) {
    /** Checks that `url` is a Breezone server; returns whether it still needs its first setup. */
    suspend fun probe(url: String): SetupStatus {
        val base = normalizeUrl(url)
        val response = http.request("$base/health") { header(CLIENT_API_HEADER, CLIENT_API) }
        val health = runCatching { FzJson.decodeFromString(Health.serializer(), response.bodyAsText()) }.getOrNull()
        if (!response.status.isSuccess() || health?.status != "ok") {
            throw ApiException(response.status.value, getString(Res.string.error_not_breezone))
        }
        checkCompatible(health)
        return FzJson.decodeFromString(http.request("$base/api/setup") { header(CLIENT_API_HEADER, CLIENT_API) }.bodyAsText())
    }

    suspend fun login(url: String, username: String, password: String, setup: Boolean = false): TokenPair {
        val base = normalizeUrl(url)
        val path = if (setup) "/api/setup" else "/api/auth/login"
        val response = http.request("$base$path") {
            method = HttpMethod.Post
            header(CLIENT_API_HEADER, CLIENT_API)
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
        if (response.status == HttpStatusCode.UpgradeRequired) throw incompatible(UpdateNeeded.App, null)
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
            header(CLIENT_API_HEADER, CLIENT_API)
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
            header(CLIENT_API_HEADER, CLIENT_API)
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
        if (response.status == HttpStatusCode.UpgradeRequired) throw incompatible(UpdateNeeded.App, null)
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

    /** Versions of the server, checked at every (re)connection. */
    suspend fun health(): Health =
        FzJson.decodeFromString(Health.serializer(), http.request("$base/health") { header(CLIENT_API_HEADER, CLIENT_API) }.bodyAsText())

    suspend fun me(): User = call<User>(HttpMethod.Get, "/api/me").also { user = it }
    suspend fun status(): List<Installation> = call(HttpMethod.Get, "/api/status")
    suspend fun devices(): List<DeviceView> = call(HttpMethod.Get, "/api/devices")
    suspend fun rooms(): List<Room> = call(HttpMethod.Get, "/api/rooms")
    /** The catalog of traits the server knows (their fields, actions and presentation). */
    suspend fun traits(): List<CatalogTrait> = call(HttpMethod.Get, "/api/traits")
    /** The modules of the server (administrators). */
    suspend fun modules(): List<ModuleInfo> = call(HttpMethod.Get, "/api/modules")
    /** The weather of an installation (servers before 0.11.0-beta.19 give that of the home). */
    suspend fun weather(installationId: Long): WeatherView = call(HttpMethod.Get, "/api/weather?installation_id=$installationId")
    suspend fun energyLive(): EnergyLive = call(HttpMethod.Get, "/api/energy/live")
    /** The energy of a local day ("YYYY-MM-DD"), today by default. */
    suspend fun energyDay(date: String? = null): EnergyDay = call(HttpMethod.Get, "/api/energy/day" + (date?.let { "?date=$it" } ?: ""))
    /** The energy day by day, over the last [days]. */
    suspend fun energyDays(days: Int): List<DayEnergy> = call(HttpMethod.Get, "/api/energy/days?days=$days")
    /** The first day the home has an energy measure of. */
    suspend fun energyRange(): EnergyRange = call(HttpMethod.Get, "/api/energy/range")
    /** The price of electricity of an installation now, and the Tempo colours. */
    suspend fun price(installationId: Long): PriceView = call(HttpMethod.Get, "/api/installations/$installationId/electricity/price?hours=24")
    suspend fun deviceHistoryOver(deviceId: Long, hours: Int): DeviceHistory =
        call(HttpMethod.Get, "/api/devices/$deviceId/history?hours=$hours&bucket=${Periods.bucket(hours)}")
    suspend fun deviceForecast(deviceId: Long): DeviceForecast = call(HttpMethod.Get, "/api/devices/$deviceId/forecast")
    suspend fun deviceHistory(deviceId: Long, hours: Int = 24, points: Int = 96): DeviceHistory =
        call(HttpMethod.Get, "/api/devices/$deviceId/history?hours=$hours&points=$points")
    suspend fun scenes(): List<SceneView> = call(HttpMethod.Get, "/api/scenes")
    /** The devices that keep a history, and its fields: what a chart can show. */
    suspend fun charted(): List<Charted> = call(HttpMethod.Get, "/api/devices/charted")
    /** A page of an installation as cards (the app's home: [MOBILE_PAGE]). */
    suspend fun dashboard(installationId: Long, page: String): DashboardView =
        call(HttpMethod.Get, "/api/installations/$installationId/dashboard?page=$page")
    suspend fun saveDashboard(installationId: Long, page: String, layout: DashboardLayout): DashboardView =
        call(HttpMethod.Put, "/api/installations/$installationId/dashboard?page=$page", FzJson.encodeToString(DashboardLayout.serializer(), layout))
    suspend fun resetDashboard(installationId: Long, page: String): DashboardView =
        call(HttpMethod.Delete, "/api/installations/$installationId/dashboard?page=$page")
    suspend fun act(deviceId: Long, input: DeviceActionInput): DeviceView =
        call(HttpMethod.Post, "/api/devices/$deviceId/actions", FzJson.encodeToString(DeviceActionInput.serializer(), input))
    suspend fun activateScene(sourceId: Long, id: String) =
        callUnit(HttpMethod.Post, "/api/scenes/$sourceId/${id.encodeURLPathPart()}")
    suspend fun serverUpdate(): ServerUpdate = call(HttpMethod.Get, "/api/system/update")
    suspend fun control(zone: Zone, cmd: ZoneCommand): Zone =
        call(HttpMethod.Patch, zonePath(zone), FzJson.encodeToString(ZoneCommand.serializer(), cmd))

    suspend fun batch(actions: List<Action>): RunResult =
        call(HttpMethod.Post, "/api/control/batch", FzJson.encodeToString(BatchBody.serializer(), BatchBody(actions)))

    suspend fun history(zone: Zone, hours: Int): List<Reading> = call(HttpMethod.Get, "${zonePath(zone)}/history?hours=$hours")
    /** How far back the history of a zone goes. */
    suspend fun historyRange(zone: Zone): HistoryRange = call(HttpMethod.Get, "${zonePath(zone)}/history/range")
    suspend fun daily(zone: Zone, days: Int): List<DayStats> = call(HttpMethod.Get, "${zonePath(zone)}/daily?days=$days")
    suspend fun trends(installationId: Long, hours: Int = 24): List<ZoneTrend> =
        call(HttpMethod.Get, "/api/installations/$installationId/trends?hours=$hours")
    suspend fun routines(): List<Routine> = call(HttpMethod.Get, "/api/routines")
    suspend fun createRoutine(input: RoutineInput): Routine =
        call(HttpMethod.Post, "/api/routines", FzJson.encodeToString(RoutineInput.serializer(), input))

    suspend fun updateRoutine(id: Long, input: RoutineInput): Routine =
        call(HttpMethod.Put, "/api/routines/$id", FzJson.encodeToString(RoutineInput.serializer(), input))

    suspend fun deleteRoutine(id: Long) = callUnit(HttpMethod.Delete, "/api/routines/$id")
    suspend fun runRoutine(id: Long): RunResult = call(HttpMethod.Post, "/api/routines/$id/run")
    suspend fun automations(): List<Automation> = call(HttpMethod.Get, "/api/automations")
    suspend fun enableAutomation(id: Long, enabled: Boolean): Automation =
        call(HttpMethod.Put, "/api/automations/$id/enabled", FzJson.encodeToString(EnabledBody.serializer(), EnabledBody(enabled)))
    suspend fun runAutomation(id: Long): AutomationRunResult = call(HttpMethod.Post, "/api/automations/$id/run")
    suspend fun automationRuns(id: Long): List<AutomationRun> = call(HttpMethod.Get, "/api/automations/$id/runs")
    suspend fun automationSettings(): AutomationSettings = call(HttpMethod.Get, "/api/automations/settings")
    suspend fun unreadableAutomations(): List<UnreadableAutomation> = call(HttpMethod.Get, "/api/automations/unreadable")
    suspend fun presence(): Presence = call(HttpMethod.Get, "/api/presence")
    suspend fun setPresence(home: Boolean): Presence =
        call(HttpMethod.Put, "/api/presence", FzJson.encodeToString(PresenceBody.serializer(), PresenceBody(home)))
    suspend fun schedules(): SchedulesState = call(HttpMethod.Get, "/api/airzone/schedules")
    suspend fun createSchedule(input: AirzoneScheduleInput): SchedulesState =
        call(HttpMethod.Post, "/api/airzone/schedules", FzJson.encodeToString(AirzoneScheduleInput.serializer(), input))

    suspend fun updateSchedule(s: AirzoneSchedule, input: AirzoneScheduleInput): SchedulesState =
        call(HttpMethod.Put, schedulePath(s), FzJson.encodeToString(AirzoneScheduleInput.serializer(), input.copy(cloudId = null)))

    suspend fun setScheduleEnabled(s: AirzoneSchedule, enabled: Boolean): SchedulesState =
        call(HttpMethod.Put, "${schedulePath(s)}/enabled", buildJsonObject { put("enabled", enabled) }.toString())

    /** All the Airzone schedules off, or on again as they were (administrators). */
    suspend fun setSchedulesEnabled(enabled: Boolean): SchedulesState =
        call(HttpMethod.Put, "/api/airzone/schedules/enabled", buildJsonObject { put("enabled", enabled) }.toString())

    suspend fun deleteSchedule(s: AirzoneSchedule): SchedulesState = call(HttpMethod.Delete, schedulePath(s))

    private fun schedulePath(s: AirzoneSchedule) =
        "/api/airzone/schedules/${s.cloudId.encodeURLPathPart()}/${s.id.encodeURLPathPart()}"

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

    fun live(): Flow<JsonObject> = if (base == DEMO_URL) DemoServer.live() else wsLive()

    private fun wsLive(): Flow<JsonObject> = callbackFlow {
        val wsUrl = base.replaceFirst("https://", "wss://").replaceFirst("http://", "ws://") + "/api/ws"
        val token = token()
        val job = launch {
            http.webSocket(wsUrl, request = { header(CLIENT_API_HEADER, CLIENT_API) }) {
                val auth = buildJsonObject {
                    put("type", "auth")
                    put("token", token)
                    put("lang", Locale.current.language)
                }
                send(Frame.Text(auth.toString()))
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
