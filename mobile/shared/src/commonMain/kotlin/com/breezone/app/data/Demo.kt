package com.breezone.app.data

import com.breezone.app.resources.Res
import com.breezone.app.resources.demo_group_ground
import com.breezone.app.resources.demo_group_upstairs
import com.breezone.app.resources.demo_home
import com.breezone.app.resources.demo_routine_evening
import com.breezone.app.resources.demo_routine_leaving
import com.breezone.app.resources.demo_schedule_night
import com.breezone.app.resources.demo_schedule_wake
import com.breezone.app.resources.demo_unavailable
import com.breezone.app.resources.demo_workshop
import com.breezone.app.resources.demo_zone_bedroom1
import com.breezone.app.resources.demo_zone_bedroom2
import com.breezone.app.resources.demo_zone_dressing
import com.breezone.app.resources.demo_zone_living
import com.breezone.app.resources.demo_zone_meeting
import com.breezone.app.resources.demo_zone_office
import com.breezone.app.resources.demo_zone_openspace
import com.breezone.app.resources.demo_zone_reception
import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.client.request.HttpResponseData
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.content.OutgoingContent
import io.ktor.http.headersOf
import io.ktor.serialization.kotlinx.json.json
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.jetbrains.compose.resources.getString
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.round
import kotlin.math.sin
import kotlin.random.Random
import kotlin.time.Clock
import kotlin.time.Instant
import kotlinx.datetime.TimeZone
import kotlinx.datetime.toLocalDateTime

/**
 * Built-in demonstration server: a simulated Breezone server living in the app, so that anyone
 * (including store reviewers) can try every screen without a server nor Airzone hardware.
 * It answers the same API as a real server (`docs/API.md`); nothing leaves the phone.
 */
const val DEMO_URL = "https://demo.breezone.invalid"

fun isDemo(url: String) = normalizeUrl(url) == DEMO_URL

/**
 * Demo account given to store reviewers: user `demo`, password `demo` opens the built-in
 * demonstration, with this address shown (read-only) in the server field. No real Breezone
 * account can match: server passwords need at least 8 characters. Nothing is sent anywhere.
 */
const val DEMO_LOGIN_URL = "https://breezone.cloud"

fun isDemoLogin(username: String, password: String): Boolean =
    username.trim().equals("demo", ignoreCase = true) && password == "demo"

fun demoHttpClient(): HttpClient = HttpClient(MockEngine { request -> DemoServer.handle(this, request) }) {
    install(ContentNegotiation) { json(FzJson) }
}

object DemoServer {
    private val mutex = Mutex()
    private var installations: List<Installation>? = null
    private var routines = mutableListOf<Routine>()
    private var nextRoutineId = 3L
    private var schedules = mutableListOf<AirzoneSchedule>()
    private var nextScheduleId = 3L
    private val user = User(1, "demo", "admin")

    private fun now() = Clock.System.now().epochSeconds

    /** Builds the demo data on first use, in the phone's language. */
    private suspend fun data(): List<Installation> {
        installations?.let { return it }
        val upstairs = getString(Res.string.demo_group_upstairs)
        val ground = getString(Res.string.demo_group_ground)
        val heatModes = listOf(Mode.Stop, Mode.Cooling, Mode.Heating, Mode.Fan, Mode.Dry)
        fun zone(inst: Long, id: Long, name: String, group: String?, on: Boolean, temp: Double, setpoint: Double, humidity: Long, master: Boolean = false) = Zone(
            installationId = inst, systemId = 1, zoneId = id, name = name, group = group, position = id,
            on = on, roomTemp = temp, humidity = humidity, setpoint = setpoint, minTemp = 15.0, maxTemp = 30.0,
            mode = Mode.Heating, modes = heatModes, isMaster = master, speed = 0, speeds = listOf(0, 1, 2, 3), sleep = 0,
            demand = on && temp < setpoint,
        )
        val online = Connection(state = "online", lastSeen = now(), latencyMs = 4)
        val list = listOf(
            Installation(
                id = 1, name = getString(Res.string.demo_home), location = "Annecy", connection = online,
                zones = listOf(
                    zone(1, 1, getString(Res.string.demo_zone_living), ground, true, 19.4, 20.0, 41, master = true)
                        .copy(airQuality = AirQuality("good", 1, "auto", 25, 50)),
                    zone(1, 2, getString(Res.string.demo_zone_office), ground, true, 19.1, 19.5, 46)
                        .copy(airQuality = AirQuality("regular", 2, "auto", 25, 50)),
                    zone(1, 3, getString(Res.string.demo_zone_bedroom1), upstairs, true, 18.2, 18.5, 45),
                    zone(1, 4, getString(Res.string.demo_zone_bedroom2), upstairs, false, 16.8, 18.0, 52),
                    zone(1, 5, getString(Res.string.demo_zone_dressing), upstairs, false, 16.1, 17.0, 55),
                ),
            ),
            Installation(
                id = 2, name = getString(Res.string.demo_workshop), location = "Lyon", connection = online,
                zones = listOf(
                    zone(2, 1, getString(Res.string.demo_zone_openspace), null, true, 20.6, 21.0, 40, master = true),
                    zone(2, 2, getString(Res.string.demo_zone_meeting), null, false, 18.3, 20.0, 44),
                    zone(2, 3, getString(Res.string.demo_zone_reception), null, true, 19.7, 20.0, 43),
                ),
            ),
        )
        routines = mutableListOf(
            Routine(1, getString(Res.string.demo_routine_leaving), actions = listOf(
                Action(1, 1, 1, on = false), Action(1, 1, 2, on = false), Action(1, 1, 3, on = false, delayMinutes = 60),
            )),
            Routine(2, getString(Res.string.demo_routine_evening), actions = listOf(
                Action(1, 1, 1, on = true, setpoint = 21.0), Action(1, 1, 3, on = true, setpoint = 19.0),
            )),
        )
        schedules = mutableListOf(
            AirzoneSchedule(
                cloudId = "demo-1", installation = list[0].name, installationId = 1, id = "1",
                name = getString(Res.string.demo_schedule_wake), days = listOf(1, 2, 3, 4, 5), time = "06:30",
                zones = listOf(ScheduleZone(1, 3), ScheduleZone(1, 4)), action = ZoneCommand(on = true, setpoint = 19.5),
            ),
            AirzoneSchedule(
                cloudId = "demo-1", installation = list[0].name, installationId = 1, id = "2",
                name = getString(Res.string.demo_schedule_night), days = (1..7).toList(), time = "23:00",
                zones = listOf(ScheduleZone(1, 1), ScheduleZone(1, 2)), action = ZoneCommand(on = false),
            ),
        )
        installations = list
        return list
    }

    /** Applies a command to a zone (the mode applies to the whole system, as on Airzone). */
    private fun apply(installationId: Long, systemId: Long, zoneId: Long, cmd: ZoneCommand): Zone? {
        var result: Zone? = null
        installations = installations?.map { inst ->
            if (inst.id != installationId) inst else inst.copy(zones = inst.zones.map { z ->
                when {
                    z.systemId == systemId && z.zoneId == zoneId -> z.copy(
                        on = cmd.on ?: z.on,
                        setpoint = cmd.setpoint?.coerceIn(15.0, 30.0) ?: z.setpoint,
                        mode = cmd.mode ?: z.mode,
                        speed = cmd.speed ?: z.speed,
                        sleep = cmd.sleep ?: z.sleep,
                    ).also { result = it }
                    z.systemId == systemId && cmd.mode != null -> z.copy(mode = cmd.mode)
                    else -> z
                }
            })
        }
        return result
    }

    /** Rooms slowly move towards their setpoint (or cool down when off). */
    private fun tick() {
        installations = installations?.map { inst ->
            inst.copy(connection = inst.connection.copy(lastSeen = now()), zones = inst.zones.map { z ->
                val temp = z.roomTemp ?: 18.0
                val target = if (z.on && z.mode != Mode.Stop) z.setpoint ?: temp else 15.5
                val next = temp + (target - temp) * 0.08 + Random.nextDouble(-0.03, 0.03)
                val t = round(next * 10) / 10
                z.copy(roomTemp = t, demand = z.on && t < (z.setpoint ?: t))
            })
        }
    }

    fun live(): Flow<JsonObject> = flow {
        emit(buildJsonObject { put("type", "hello") })
        while (true) {
            val snapshot = mutex.withLock { data(); FzJson.encodeToJsonElement(ListSerializer(Installation.serializer()), installations!!) }
            emit(buildJsonObject { put("type", "snapshot"); put("installations", snapshot) })
            delay(5_000)
            mutex.withLock { tick() }
        }
    }

    private fun history(zone: Zone, hours: Int): List<Reading> {
        val end = now()
        val step = if (hours <= 24) 300L else 1800L
        val seed = (zone.installationId * 31 + zone.zoneId).toInt()
        return (0 until (hours * 3600 / step).toInt()).map { i ->
            val ts = end - (hours * 3600L) + i * step
            val hour = ((ts / 3600) % 24).toInt()
            val on = hour in 6..8 || hour in 17..22
            val base = (zone.setpoint ?: 19.0) - if (on) 0.3 else 2.2
            val wave = sin((ts / 3600.0 + seed) / 24.0 * 2 * PI) * 0.8
            Reading(
                ts = ts,
                roomTemp = round((base + wave) * 10) / 10,
                humidity = 42 + ((i + seed) % 9).toLong(),
                setpoint = if (on) zone.setpoint else null,
                isOn = on,
                mode = 3,
                // Heating while below the setpoint, then at it.
                demandSecs = if (on && base + wave < (zone.setpoint ?: 19.0) - 0.2) step else 0,
                // Mostly good, regular in the evening, a bad moment around dinner.
                airQuality = zone.airQuality?.let { if (hour == 19) 3 else if (hour in 17..22) 2 else 1 },
            )
        }
    }

    private fun daily(zone: Zone, days: Int): List<DayStats> =
        history(zone, days * 24)
            .groupBy { Instant.fromEpochSeconds(it.ts).toLocalDateTime(TimeZone.currentSystemDefault()).date.toString() }
            .map { (day, readings) ->
                val temps = readings.mapNotNull { it.roomTemp }
                val on = readings.filter { it.isOn == true }
                val comfortable = on.count { r -> r.roomTemp != null && r.setpoint != null && abs(r.roomTemp - r.setpoint) <= 0.5 }
                // Same spacing as `history`.
                val step = if (days <= 1) 300L else 1800L
                DayStats(
                    day = day,
                    minTemp = temps.minOrNull(),
                    maxTemp = temps.maxOrNull(),
                    avgTemp = temps.takeIf { it.isNotEmpty() }?.let { round(it.average() * 10) / 10 },
                    recordedSecs = readings.size * step,
                    onSecs = on.size * step,
                    comfortSecs = comfortable * step,
                    comfort = if (on.isEmpty()) null else comfortable.toDouble() / on.size,
                    demandSecs = on.size * step / 2,
                    airQualitySecs = zone.airQuality?.let {
                        AirQualitySecs(
                            good = readings.count { it.airQuality == 1L } * step,
                            regular = readings.count { it.airQuality == 2L } * step,
                            bad = readings.count { it.airQuality == 3L } * step,
                        )
                    },
                )
            }

    private fun trends(installation: Installation): List<ZoneTrend> = installation.zones.map { z ->
        ZoneTrend(z.systemId, z.zoneId, history(z, 24).chunked(6).map { c -> listOf(c.first().ts.toDouble(), round(c.mapNotNull { it.roomTemp }.average() * 10) / 10) })
    }

    private suspend fun body(request: HttpRequestData): String =
        (request.body as? OutgoingContent.ByteArrayContent)?.bytes()?.decodeToString() ?: ""

    suspend fun handle(scope: MockRequestHandleScope, request: HttpRequestData): HttpResponseData = mutex.withLock {
        val list = data()
        val path = request.url.encodedPath
        val parts = path.trim('/').split('/')
        fun json(text: String, status: HttpStatusCode = HttpStatusCode.OK) =
            scope.respond(text, status, headersOf("Content-Type", "application/json"))
        fun <T> ok(serializer: kotlinx.serialization.KSerializer<T>, value: T) = json(FzJson.encodeToString(serializer, value))
        suspend fun unavailable() = json(buildJsonObject { put("error", getString(Res.string.demo_unavailable)) }.toString(), HttpStatusCode.Forbidden)
        val tokens = TokenPair("demo-access", "demo-refresh", 3600, user)
        when {
            path == "/health" -> json("""{"status":"ok","version":"demo"}""")
            path == "/api/setup" && request.method == HttpMethod.Get -> json("""{"needs_setup":false}""")
            path == "/api/auth/login" || path == "/api/auth/refresh" -> ok(TokenPair.serializer(), tokens)
            path == "/api/auth/logout" -> scope.respond("", HttpStatusCode.NoContent)
            path == "/api/me" -> ok(User.serializer(), user)
            path == "/api/status" -> ok(ListSerializer(Installation.serializer()), installations ?: list)
            path == "/api/cloud" && request.method == HttpMethod.Get -> ok(
                CloudStatus.serializer(),
                CloudStatus(
                    email = "demo@breezone.cloud",
                    lastSync = now(),
                    installations = list.map {
                        CloudInstallation("demo-${it.id}", it.name, it.location, it.zones.size.toLong(), it.id, reachable = true)
                    },
                ),
            )
            path.startsWith("/api/cloud") -> unavailable()
            path == "/api/control/batch" -> {
                val actions = FzJson.parseToJsonElement(body(request)).jsonObject["actions"]
                    ?.let { FzJson.decodeFromJsonElement(ListSerializer(Action.serializer()), it) }.orEmpty()
                val results = actions.map { a -> ActionResult(apply(a.installationId, a.systemId, a.zoneId, ZoneCommand(on = a.on, setpoint = a.setpoint, mode = a.mode)) != null) }
                ok(RunResult.serializer(), RunResult(results))
            }
            parts.size == 4 && parts[1] == "installations" && parts[3] == "trends" -> {
                val inst = installations?.firstOrNull { it.id == parts[2].toLongOrNull() }
                    ?: return@withLock json("""{"error":"installation not found"}""", HttpStatusCode.NotFound)
                ok(ListSerializer(ZoneTrend.serializer()), trends(inst))
            }
            parts.size >= 6 && parts[1] == "installations" && parts[3] == "zones" -> {
                val (inst, sys, zone) = Triple(parts[2].toLong(), parts[4].toLong(), parts[5].toLong())
                val z = installations?.firstOrNull { it.id == inst }?.zone(sys, zone)
                    ?: return@withLock json("""{"error":"zone not found"}""", HttpStatusCode.NotFound)
                if (parts.size == 7 && parts[6] == "history") {
                    val hours = request.url.parameters["hours"]?.toIntOrNull() ?: 24
                    ok(ListSerializer(Reading.serializer()), history(z, hours.coerceIn(1, 24 * 30)))
                } else if (parts.size == 7 && parts[6] == "daily") {
                    val days = request.url.parameters["days"]?.toIntOrNull() ?: 14
                    ok(ListSerializer(DayStats.serializer()), daily(z, days.coerceIn(1, 30)))
                } else {
                    val cmd = FzJson.decodeFromString(ZoneCommand.serializer(), body(request))
                    ok(Zone.serializer(), apply(inst, sys, zone, cmd)!!)
                }
            }
            path == "/api/routines" && request.method == HttpMethod.Get -> ok(ListSerializer(Routine.serializer()), routines)
            path == "/api/routines" && request.method == HttpMethod.Post -> {
                val input = FzJson.decodeFromString(RoutineInput.serializer(), body(request))
                val r = Routine(nextRoutineId++, input.name, input.enabled, input.actions)
                routines += r
                ok(Routine.serializer(), r)
            }
            parts.size >= 3 && parts[1] == "routines" -> {
                val id = parts[2].toLong()
                val index = routines.indexOfFirst { it.id == id }
                if (index < 0) return@withLock json("""{"error":"routine not found"}""", HttpStatusCode.NotFound)
                when {
                    parts.size == 4 && parts[3] == "run" -> {
                        val r = routines[index]
                        val results = r.actions.filter { it.delayMinutes == null }.map { a ->
                            ActionResult(apply(a.installationId, a.systemId, a.zoneId, ZoneCommand(on = a.on, setpoint = a.setpoint, mode = a.mode)) != null)
                        }
                        routines[index] = r.copy(lastRunAt = now())
                        ok(RunResult.serializer(), RunResult(results))
                    }
                    request.method == HttpMethod.Put -> {
                        val input = FzJson.decodeFromString(RoutineInput.serializer(), body(request))
                        routines[index] = routines[index].copy(name = input.name, enabled = input.enabled, actions = input.actions)
                        ok(Routine.serializer(), routines[index])
                    }
                    request.method == HttpMethod.Delete -> {
                        routines.removeAt(index)
                        scope.respond("", HttpStatusCode.NoContent)
                    }
                    else -> ok(Routine.serializer(), routines[index])
                }
            }
            path == "/api/airzone/schedules" && request.method == HttpMethod.Get -> scope.schedulesState()
            path == "/api/airzone/schedules" && request.method == HttpMethod.Post -> {
                val input = FzJson.decodeFromString(AirzoneScheduleInput.serializer(), body(request))
                val inst = list.firstOrNull { "demo-${it.id}" == input.cloudId }
                schedules += AirzoneSchedule(
                    cloudId = input.cloudId.orEmpty(), installation = inst?.name.orEmpty(), installationId = inst?.id,
                    id = (nextScheduleId++).toString(), name = input.name, enabled = input.enabled, days = input.days,
                    time = input.time, zones = input.zones, action = input.action,
                )
                scope.schedulesState()
            }
            parts.size >= 5 && parts[1] == "airzone" && parts[2] == "schedules" -> {
                val index = schedules.indexOfFirst { it.cloudId == parts[3] && it.id == parts[4] }
                if (index < 0) return@withLock json("""{"error":"schedule not found"}""", HttpStatusCode.NotFound)
                when {
                    parts.size == 6 && parts[5] == "enabled" -> {
                        val enabled = FzJson.parseToJsonElement(body(request)).jsonObject["enabled"]?.jsonPrimitive?.booleanOrNull ?: true
                        schedules[index] = schedules[index].copy(enabled = enabled)
                    }
                    request.method == HttpMethod.Put -> {
                        val input = FzJson.decodeFromString(AirzoneScheduleInput.serializer(), body(request))
                        schedules[index] = schedules[index].copy(
                            name = input.name, enabled = input.enabled, days = input.days, time = input.time,
                            zones = input.zones, action = input.action,
                        )
                    }
                    request.method == HttpMethod.Delete -> schedules.removeAt(index)
                }
                scope.schedulesState()
            }
            else -> unavailable()
        }
    }

    private fun MockRequestHandleScope.schedulesState() = respond(
        FzJson.encodeToString(SchedulesState.serializer(), SchedulesState(lastSync = now(), account = true, schedules = schedules)),
        HttpStatusCode.OK,
        headersOf("Content-Type", "application/json"),
    )
}
